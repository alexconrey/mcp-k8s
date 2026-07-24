use k8s_openapi::api::apps::v1::ReplicaSet;
use k8s_openapi::api::core::v1::{Service, ServicePort, ServiceSpec};
use k8s_openapi::api::networking::v1::{
    HTTPIngressPath, HTTPIngressRuleValue, Ingress, IngressBackend, IngressRule,
    IngressServiceBackend, IngressSpec, ServiceBackendPort,
};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;
use kube::api::{ListParams, Patch, PatchParams, PostParams};
use kube::Api;
use serde::Deserialize;

use crate::client::K8sClient;
use crate::extract;

/// Try to handle a tool call. Returns `Some(result)` if the tool name is
/// recognized, `None` if the caller should handle it.
pub async fn handle_tool(
    client: &K8sClient,
    name: &str,
    args: &serde_json::Value,
) -> Option<Result<String, String>> {
    // Check permissions before dispatching
    if !client.permissions().is_tool_allowed(name) {
        let action = crate::permissions::ActionPermissions::action_for_tool(name);
        return Some(Err(format!(
            "action '{action}' is not allowed on this tool: {name}"
        )));
    }

    // Try resource module handlers first
    if let result @ Some(_) = crate::resources::handle_tool(client, name, args).await {
        return result;
    }

    let result = match name {
        "list_namespaces" => list_namespaces(client).await,
        "list_deployments" => list_deployments(client, args).await,
        "get_deployment" => get_deployment(client, args).await,
        "get_pod_logs" => get_pod_logs(client, args).await,
        "get_events" => get_events(client, args).await,
        "get_deployment_history" => get_deployment_history(client, args).await,
        "get_build_logs" => get_build_logs(client, args).await,
        "list_ingresses" => list_ingresses(client, args).await,
        "get_metrics" => get_metrics(client, args).await,
        "create_service" => create_service(client, args).await,
        "create_ingress" => create_ingress(client, args).await,
        "update_ingress" => update_ingress(client, args).await,
        _ => return None,
    };
    Some(result)
}

async fn list_namespaces(client: &K8sClient) -> Result<String, String> {
    let ns_api = client.namespaces_api();
    let list = ns_api
        .list(&ListParams::default())
        .await
        .map_err(|e| e.to_string())?;

    let names: Vec<String> = list
        .iter()
        .filter_map(|ns| ns.metadata.name.clone())
        .filter(|name| client.is_namespace_allowed(name))
        .collect();

    Ok(names.join("\n"))
}

async fn list_deployments(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let api = client.deployments_api(ns).map_err(|e| e.to_string())?;
    let label_selector = args["label_selector"].as_str();
    let field_selector = args["field_selector"].as_str();
    let mut lp = ListParams::default();
    if let Some(sel) = label_selector {
        lp = lp.labels(sel);
    }
    if let Some(sel) = field_selector {
        lp = lp.fields(sel);
    }
    let list = api.list(&lp).await.map_err(|e| e.to_string())?;

    let summaries: Vec<serde_json::Value> = list
        .iter()
        .map(|dep| {
            let s = extract::deployment_summary(dep);
            serde_json::to_value(s).unwrap_or_default()
        })
        .collect();

    serde_json::to_string_pretty(&summaries).map_err(|e| e.to_string())
}

async fn get_deployment(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;

    let dep_api = client.deployments_api(ns).map_err(|e| e.to_string())?;
    let dep = dep_api.get(name).await.map_err(|e| e.to_string())?;
    let detail = extract::deployment_detail(&dep);

    let pods = {
        let pods_api = client.pods_api(ns).map_err(|e| e.to_string())?;
        let selector = dep
            .spec
            .as_ref()
            .and_then(|s| s.selector.match_labels.as_ref())
            .map(|labels| {
                labels
                    .iter()
                    .map(|(k, v)| format!("{k}={v}"))
                    .collect::<Vec<_>>()
                    .join(",")
            })
            .unwrap_or_default();
        let lp = ListParams::default().labels(&selector);
        let pod_list = pods_api.list(&lp).await.map_err(|e| e.to_string())?;
        let summaries: Vec<serde_json::Value> = pod_list
            .iter()
            .map(|p| serde_json::to_value(extract::pod_summary(p)).unwrap_or_default())
            .collect();
        summaries
    };

    let ingresses = {
        let ing_api = client.ingresses_api(ns).map_err(|e| e.to_string())?;
        let all = ing_api
            .list(&ListParams::default())
            .await
            .map_err(|e| e.to_string())?;
        let matching: Vec<serde_json::Value> = all
            .iter()
            .filter(|ing| {
                ing.spec
                    .as_ref()
                    .and_then(|s| s.rules.as_ref())
                    .map(|rules| {
                        rules.iter().any(|r| {
                            r.http
                                .as_ref()
                                .map(|http| {
                                    http.paths.iter().any(|p| {
                                        p.backend
                                            .service
                                            .as_ref()
                                            .map(|s| s.name == name)
                                            .unwrap_or(false)
                                    })
                                })
                                .unwrap_or(false)
                        })
                    })
                    .unwrap_or(false)
            })
            .map(|ing| serde_json::to_value(extract::ingress_summary(ing)).unwrap_or_default())
            .collect();
        matching
    };

    let result = serde_json::json!({
        "detail": serde_json::to_value(detail).unwrap_or_default(),
        "pods": pods,
        "ingresses": ingresses,
    });

    serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
}

async fn get_pod_logs(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let pod = args["pod_name"].as_str().ok_or("pod_name is required")?;
    let tail = args["tail_lines"].as_i64();
    let container = args["container"].as_str().map(|s| s.to_string());

    let pods_api = client.pods_api(ns).map_err(|e| e.to_string())?;
    let params = kube::api::LogParams {
        tail_lines: tail,
        container,
        timestamps: true,
        ..Default::default()
    };
    let logs = pods_api
        .logs(pod, &params)
        .await
        .map_err(|e| e.to_string())?;
    Ok(logs)
}

async fn get_events(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let resource_name = args["resource_name"].as_str();
    let label_selector = args["label_selector"].as_str();
    let field_selector = args["field_selector"].as_str();

    let api = client.events_api(ns).map_err(|e| e.to_string())?;
    let mut lp = ListParams::default();

    // Build combined field selector from resource_name shorthand and explicit field_selector
    let mut field_parts: Vec<String> = Vec::new();
    if let Some(name) = resource_name {
        field_parts.push(format!("involvedObject.name={name}"));
    }
    if let Some(sel) = field_selector {
        field_parts.push(sel.to_string());
    }
    if !field_parts.is_empty() {
        lp = lp.fields(&field_parts.join(","));
    }

    if let Some(sel) = label_selector {
        lp = lp.labels(sel);
    }
    let list = api.list(&lp).await.map_err(|e| e.to_string())?;

    let mut events: Vec<String> = list
        .iter()
        .map(|e| {
            let s = extract::event_summary(e);
            format!(
                "[{}] {} {} {}: {}",
                s.last_timestamp
                    .as_deref()
                    .or(s.first_timestamp.as_deref())
                    .unwrap_or("?"),
                s.event_type,
                s.involved_object_kind,
                s.involved_object_name,
                s.message.as_deref().unwrap_or("(no message)"),
            )
        })
        .collect();

    events.sort_by(|a, b| b.cmp(a));

    if events.is_empty() {
        Ok("No events found.".to_string())
    } else {
        Ok(events.join("\n"))
    }
}

async fn get_deployment_history(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;

    let dep_api = client.deployments_api(ns).map_err(|e| e.to_string())?;
    let dep = dep_api.get(name).await.map_err(|e| e.to_string())?;

    let selector = dep
        .spec
        .as_ref()
        .and_then(|s| s.selector.match_labels.as_ref())
        .map(|labels| {
            labels
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join(",")
        })
        .unwrap_or_default();

    let rs_api: Api<ReplicaSet> = Api::namespaced(client.inner().clone(), ns);
    let lp = ListParams::default().labels(&selector);
    let rs_list = rs_api.list(&lp).await.map_err(|e| e.to_string())?;

    let mut revisions: Vec<String> = rs_list
        .items
        .iter()
        .filter_map(|rs| {
            let annotations = rs.metadata.annotations.as_ref()?;
            let revision: i64 = annotations
                .get("deployment.kubernetes.io/revision")?
                .parse()
                .ok()?;
            let image = rs
                .spec
                .as_ref()
                .and_then(|s| s.template.as_ref())
                .and_then(|t| t.spec.as_ref())
                .and_then(|s| s.containers.first())
                .and_then(|c| c.image.clone())
                .unwrap_or_else(|| "<unknown>".to_string());
            let replicas = rs.spec.as_ref().and_then(|s| s.replicas).unwrap_or(0);
            let ready = rs
                .status
                .as_ref()
                .and_then(|s| s.ready_replicas)
                .unwrap_or(0);
            let created = rs
                .metadata
                .creation_timestamp
                .as_ref()
                .map(|t| t.0.to_string())
                .unwrap_or_else(|| "?".to_string());
            let change_cause = annotations
                .get("kubernetes.io/change-cause")
                .cloned()
                .unwrap_or_default();

            Some(format!(
                "Rev {revision}: image={image} replicas={replicas}/{ready} created={created} cause={change_cause}"
            ))
        })
        .collect();

    revisions.sort_by(|a, b| b.cmp(a));

    if revisions.is_empty() {
        Ok("No revision history found.".to_string())
    } else {
        Ok(revisions.join("\n"))
    }
}

async fn get_build_logs(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let job_name = args["job_name"].as_str().ok_or("job_name is required")?;

    let pods_api = client.pods_api(ns).map_err(|e| e.to_string())?;
    let lp = ListParams::default().labels(&format!("job-name={job_name}"));
    let pods = pods_api.list(&lp).await.map_err(|e| e.to_string())?;

    let pod_name = pods
        .items
        .first()
        .and_then(|p| p.metadata.name.clone())
        .ok_or_else(|| format!("No pod found for job {job_name}"))?;

    let params = kube::api::LogParams {
        timestamps: true,
        ..Default::default()
    };
    let logs = pods_api
        .logs(&pod_name, &params)
        .await
        .map_err(|e| e.to_string())?;
    Ok(logs)
}

async fn list_ingresses(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let api = client.ingresses_api(ns).map_err(|e| e.to_string())?;
    let label_selector = args["label_selector"].as_str();
    let mut lp = ListParams::default();
    if let Some(sel) = label_selector {
        lp = lp.labels(sel);
    }
    let list = api.list(&lp).await.map_err(|e| e.to_string())?;

    let summaries: Vec<serde_json::Value> = list
        .iter()
        .map(|ing| serde_json::to_value(extract::ingress_summary(ing)).unwrap_or_default())
        .collect();

    serde_json::to_string_pretty(&summaries).map_err(|e| e.to_string())
}

async fn get_metrics(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let label_selector = args["label_selector"].as_str();

    if !client.is_namespace_allowed(ns) {
        return Err(format!("Namespace '{ns}' is not in the allowed list"));
    }

    let mut uri = format!("/apis/metrics.k8s.io/v1beta1/namespaces/{ns}/pods");
    if let Some(sel) = label_selector.filter(|s| !s.is_empty()) {
        uri.push_str("?labelSelector=");
        for b in sel.bytes() {
            match b {
                b'A'..=b'Z'
                | b'a'..=b'z'
                | b'0'..=b'9'
                | b'-'
                | b'_'
                | b'.'
                | b'~'
                | b'='
                | b',' => uri.push(b as char),
                _ => uri.push_str(&format!("%{b:02X}")),
            }
        }
    }

    let req = http::Request::builder()
        .uri(&uri)
        .body(Vec::<u8>::new())
        .map_err(|e: http::Error| e.to_string())?;

    #[derive(Deserialize)]
    struct MetricsList {
        items: Vec<RawPodMetrics>,
    }
    #[derive(Deserialize)]
    struct RawPodMetrics {
        metadata: MetricsMeta,
        #[allow(dead_code)]
        timestamp: String,
        containers: Vec<RawContainerMetrics>,
    }
    #[derive(Deserialize)]
    struct MetricsMeta {
        name: String,
    }
    #[derive(Deserialize)]
    struct RawContainerMetrics {
        name: String,
        usage: Usage,
    }
    #[derive(Deserialize)]
    struct Usage {
        cpu: String,
        memory: String,
    }

    let result: Result<MetricsList, kube::Error> = client.inner().request(req).await;

    match result {
        Ok(list) => {
            let mut lines: Vec<String> = Vec::new();
            for pod in list.items {
                for c in pod.containers {
                    lines.push(format!(
                        "pod={} container={} cpu={} memory={}",
                        pod.metadata.name, c.name, c.usage.cpu, c.usage.memory,
                    ));
                }
            }
            if lines.is_empty() {
                Ok("No pod metrics found. Is metrics-server installed?".to_string())
            } else {
                Ok(lines.join("\n"))
            }
        }
        Err(kube::Error::Api(api_err)) if api_err.code == 404 => {
            Ok("metrics-server does not appear to be installed in this cluster.".to_string())
        }
        Err(kube::Error::Api(api_err)) if api_err.code == 503 => {
            Ok("metrics-server is installed but not ready. Give it 60s after startup.".to_string())
        }
        Err(e) => Err(e.to_string()),
    }
}

async fn create_service(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;
    let port = args["port"].as_i64().unwrap_or(80) as i32;
    let target_port = args["target_port"].as_i64().unwrap_or(port as i64) as i32;

    let mut labels = std::collections::BTreeMap::new();
    labels.insert("app".to_string(), name.to_string());
    labels.insert(
        "app.kubernetes.io/managed-by".to_string(),
        "mcp-k8s".to_string(),
    );

    let mut selector = std::collections::BTreeMap::new();
    selector.insert("app".to_string(), name.to_string());

    let svc = Service {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            namespace: Some(ns.to_string()),
            labels: Some(labels),
            ..Default::default()
        },
        spec: Some(ServiceSpec {
            selector: Some(selector),
            ports: Some(vec![ServicePort {
                port,
                target_port: Some(IntOrString::Int(target_port)),
                protocol: Some("TCP".to_string()),
                ..Default::default()
            }]),
            ..Default::default()
        }),
        ..Default::default()
    };

    let svc_api = client.services_api(ns).map_err(|e| e.to_string())?;
    let created = svc_api
        .create(&PostParams::default(), &svc)
        .await
        .map_err(|e| e.to_string())?;

    let result = serde_json::json!({
        "name": created.metadata.name,
        "namespace": ns,
        "cluster_ip": created.spec.as_ref().and_then(|s| s.cluster_ip.clone()),
        "port": port,
        "target_port": target_port,
    });
    serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
}

async fn ensure_service(
    client: &K8sClient,
    ns: &str,
    service_name: &str,
    service_port: i32,
) -> Result<(), String> {
    let svc_api = client.services_api(ns).map_err(|e| e.to_string())?;
    if svc_api.get(service_name).await.is_ok() {
        return Ok(());
    }

    let mut labels = std::collections::BTreeMap::new();
    labels.insert("app".to_string(), service_name.to_string());
    labels.insert(
        "app.kubernetes.io/managed-by".to_string(),
        "mcp-k8s".to_string(),
    );

    let mut selector = std::collections::BTreeMap::new();
    selector.insert("app".to_string(), service_name.to_string());

    let svc = Service {
        metadata: ObjectMeta {
            name: Some(service_name.to_string()),
            namespace: Some(ns.to_string()),
            labels: Some(labels),
            ..Default::default()
        },
        spec: Some(ServiceSpec {
            selector: Some(selector),
            ports: Some(vec![ServicePort {
                port: service_port,
                target_port: Some(IntOrString::Int(service_port)),
                protocol: Some("TCP".to_string()),
                ..Default::default()
            }]),
            ..Default::default()
        }),
        ..Default::default()
    };

    svc_api
        .create(&PostParams::default(), &svc)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

async fn create_ingress(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;
    let service_name = args["service_name"]
        .as_str()
        .ok_or("service_name is required")?;
    let service_port = args["service_port"].as_i64().unwrap_or(80) as i32;
    let path = args["path"].as_str().unwrap_or("/").to_string();
    let path_type = args["path_type"].as_str().unwrap_or("Prefix").to_string();
    let host = args["host"].as_str().map(|s| s.to_string());
    let ingress_class = args["ingress_class"].as_str().map(|s| s.to_string());
    let annotations: Option<std::collections::BTreeMap<String, String>> = args
        .get("annotations")
        .and_then(|v| serde_json::from_value(v.clone()).ok());

    ensure_service(client, ns, service_name, service_port).await?;

    let mut labels = std::collections::BTreeMap::new();
    labels.insert(
        "app.kubernetes.io/managed-by".to_string(),
        "mcp-k8s".to_string(),
    );

    let ingress = Ingress {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            namespace: Some(ns.to_string()),
            labels: Some(labels),
            annotations,
            ..Default::default()
        },
        spec: Some(IngressSpec {
            ingress_class_name: ingress_class,
            rules: Some(vec![IngressRule {
                host,
                http: Some(HTTPIngressRuleValue {
                    paths: vec![HTTPIngressPath {
                        path: Some(path),
                        path_type,
                        backend: IngressBackend {
                            service: Some(IngressServiceBackend {
                                name: service_name.to_string(),
                                port: Some(ServiceBackendPort {
                                    number: Some(service_port),
                                    ..Default::default()
                                }),
                            }),
                            ..Default::default()
                        },
                    }],
                }),
            }]),
            ..Default::default()
        }),
        ..Default::default()
    };

    let ing_api = client.ingresses_api(ns).map_err(|e| e.to_string())?;
    let created = ing_api
        .create(&PostParams::default(), &ingress)
        .await
        .map_err(|e| e.to_string())?;

    let detail = extract::ingress_detail(&created);
    serde_json::to_string_pretty(&serde_json::to_value(detail).unwrap_or_default())
        .map_err(|e| e.to_string())
}

async fn update_ingress(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;
    let service_name = args["service_name"]
        .as_str()
        .ok_or("service_name is required")?;
    let service_port = args["service_port"].as_i64().unwrap_or(80) as i32;
    let path = args["path"].as_str().unwrap_or("/").to_string();
    let path_type = args["path_type"].as_str().unwrap_or("Prefix").to_string();
    let host = args["host"].as_str().map(|s| s.to_string());
    let ingress_class = args["ingress_class"].as_str().map(|s| s.to_string());
    let annotations: Option<std::collections::BTreeMap<String, String>> = args
        .get("annotations")
        .and_then(|v| serde_json::from_value(v.clone()).ok());

    let patch = serde_json::json!({
        "metadata": {
            "annotations": annotations,
        },
        "spec": {
            "ingressClassName": ingress_class,
            "rules": [{
                "host": host,
                "http": {
                    "paths": [{
                        "path": path,
                        "pathType": path_type,
                        "backend": {
                            "service": {
                                "name": service_name,
                                "port": { "number": service_port }
                            }
                        }
                    }]
                }
            }]
        }
    });

    let ing_api = client.ingresses_api(ns).map_err(|e| e.to_string())?;
    let patched = ing_api
        .patch(name, &PatchParams::apply("mcp-k8s"), &Patch::Merge(&patch))
        .await
        .map_err(|e| e.to_string())?;

    let detail = extract::ingress_detail(&patched);
    serde_json::to_string_pretty(&serde_json::to_value(detail).unwrap_or_default())
        .map_err(|e| e.to_string())
}
