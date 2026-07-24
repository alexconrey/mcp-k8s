use std::collections::BTreeMap;

use k8s_openapi::api::core::v1::Service;
use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;
use kube::api::{DeleteParams, ListParams, Patch, PatchParams};
use serde::Serialize;

use crate::client::K8sClient;

fn api(client: &K8sClient, ns: &str) -> Result<kube::Api<Service>, String> {
    if !client.is_namespace_allowed(ns) {
        return Err(format!("Namespace '{ns}' is not in the allowed list"));
    }
    Ok(kube::Api::namespaced(client.inner().clone(), ns))
}

#[derive(Serialize, Debug)]
pub struct ServicePortSummary {
    pub port: i32,
    pub target_port: Option<String>,
    pub protocol: String,
    pub name: Option<String>,
}

#[derive(Serialize, Debug)]
pub struct ServiceSummary {
    pub name: String,
    pub namespace: String,
    pub service_type: String,
    pub cluster_ip: Option<String>,
    pub external_ips: Vec<String>,
    pub ports: Vec<ServicePortSummary>,
    pub created_at: Option<String>,
    pub labels: BTreeMap<String, String>,
}

fn extract_port_summary(
    port: &k8s_openapi::api::core::v1::ServicePort,
) -> ServicePortSummary {
    let target_port = port.target_port.as_ref().map(|tp| match tp {
        IntOrString::Int(i) => i.to_string(),
        IntOrString::String(s) => s.clone(),
    });
    ServicePortSummary {
        port: port.port,
        target_port,
        protocol: port.protocol.clone().unwrap_or_else(|| "TCP".to_string()),
        name: port.name.clone(),
    }
}

fn extract_summary(svc: &Service) -> ServiceSummary {
    let meta = &svc.metadata;
    let spec = svc.spec.as_ref();

    let service_type = spec
        .and_then(|s| s.type_.clone())
        .unwrap_or_else(|| "ClusterIP".to_string());

    let cluster_ip = spec.and_then(|s| s.cluster_ip.clone());

    let external_ips = extract_external_ips(svc);

    let ports: Vec<ServicePortSummary> = spec
        .and_then(|s| s.ports.as_ref())
        .map(|ports| ports.iter().map(extract_port_summary).collect())
        .unwrap_or_default();

    ServiceSummary {
        name: meta.name.clone().unwrap_or_default(),
        namespace: meta.namespace.clone().unwrap_or_default(),
        service_type,
        cluster_ip,
        external_ips,
        ports,
        created_at: meta.creation_timestamp.as_ref().map(|t| t.0.to_string()),
        labels: meta.labels.clone().unwrap_or_default(),
    }
}

fn extract_external_ips(svc: &Service) -> Vec<String> {
    let mut ips = Vec::new();

    // Check status.loadBalancer.ingress for LB-assigned addresses
    if let Some(status) = &svc.status {
        if let Some(lb) = &status.load_balancer {
            if let Some(ingresses) = &lb.ingress {
                for ing in ingresses {
                    if let Some(ip) = &ing.ip {
                        ips.push(ip.clone());
                    } else if let Some(hostname) = &ing.hostname {
                        ips.push(hostname.clone());
                    }
                }
            }
        }
    }

    // Also include spec.externalIPs if set
    if let Some(spec) = &svc.spec {
        if let Some(external) = &spec.external_ips {
            for ip in external {
                if !ips.contains(ip) {
                    ips.push(ip.clone());
                }
            }
        }
    }

    ips
}

pub fn tool_definitions() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "name": "list_services",
            "description": "List services in a namespace. Returns name, namespace, type, cluster IP, external IPs, ports, created_at, and labels.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "label_selector": { "type": "string", "description": "Label selector to filter results (e.g. app=nginx)" },
                    "field_selector": { "type": "string", "description": "Field selector to filter results (e.g. metadata.name=foo)" }
                },
                "required": ["namespace"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "get_service",
            "description": "Get detailed info for a single service including selector, session affinity, and annotations.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "Service name" }
                },
                "required": ["namespace", "name"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "update_service",
            "description": "Patch a service. Accepts optional fields: ports (array of {port, target_port, protocol}), selector (object), type.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "Service name" },
                    "ports": {
                        "type": "array",
                        "description": "Array of port definitions",
                        "items": {
                            "type": "object",
                            "properties": {
                                "port": { "type": "integer", "description": "Service port" },
                                "target_port": { "type": "integer", "description": "Container target port" },
                                "protocol": { "type": "string", "description": "Protocol (TCP, UDP, SCTP). Defaults to TCP." }
                            },
                            "required": ["port"]
                        }
                    },
                    "selector": {
                        "type": "object",
                        "description": "Label selector for pods",
                        "additionalProperties": { "type": "string" }
                    },
                    "type": {
                        "type": "string",
                        "description": "Service type (ClusterIP, NodePort, LoadBalancer, ExternalName)",
                        "enum": ["ClusterIP", "NodePort", "LoadBalancer", "ExternalName"]
                    }
                },
                "required": ["namespace", "name"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "delete_service",
            "description": "Delete a service by name from a namespace.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "Service name" }
                },
                "required": ["namespace", "name"],
                "additionalProperties": false
            }
        }),
    ]
}

pub async fn handle_tool(
    client: &K8sClient,
    name: &str,
    args: &serde_json::Value,
) -> Option<Result<String, String>> {
    let result = match name {
        "list_services" => list_services(client, args).await,
        "get_service" => get_service(client, args).await,
        "update_service" => update_service(client, args).await,
        "delete_service" => delete_service(client, args).await,
        _ => return None,
    };
    Some(result)
}

async fn list_services(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let svc_api = api(client, ns)?;
    let label_selector = args["label_selector"].as_str();
    let field_selector = args["field_selector"].as_str();
    let mut lp = ListParams::default();
    if let Some(sel) = label_selector {
        lp = lp.labels(sel);
    }
    if let Some(sel) = field_selector {
        lp = lp.fields(sel);
    }
    let list = svc_api
        .list(&lp)
        .await
        .map_err(|e| e.to_string())?;

    let summaries: Vec<serde_json::Value> = list
        .iter()
        .map(|svc| {
            let s = extract_summary(svc);
            serde_json::to_value(s).unwrap_or_default()
        })
        .collect();

    serde_json::to_string_pretty(&summaries).map_err(|e| e.to_string())
}

async fn get_service(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;

    let svc_api = api(client, ns)?;
    let svc = svc_api.get(name).await.map_err(|e| e.to_string())?;

    let summary = extract_summary(&svc);
    let spec = svc.spec.as_ref();
    let meta = &svc.metadata;

    let selector = spec
        .and_then(|s| s.selector.clone())
        .unwrap_or_default();

    let session_affinity = spec
        .and_then(|s| s.session_affinity.clone())
        .unwrap_or_else(|| "None".to_string());

    let result = serde_json::json!({
        "name": summary.name,
        "namespace": summary.namespace,
        "service_type": summary.service_type,
        "cluster_ip": summary.cluster_ip,
        "external_ips": summary.external_ips,
        "ports": serde_json::to_value(&summary.ports).unwrap_or_default(),
        "created_at": summary.created_at,
        "labels": summary.labels,
        "selector": selector,
        "session_affinity": session_affinity,
        "annotations": meta.annotations.clone().unwrap_or_default(),
    });

    serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
}

async fn update_service(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;

    let mut spec_patch = serde_json::Map::new();

    if let Some(ports_val) = args.get("ports") {
        if let Some(ports_arr) = ports_val.as_array() {
            let ports: Vec<serde_json::Value> = ports_arr
                .iter()
                .map(|p| {
                    let port = p["port"].as_i64().unwrap_or(80);
                    let target_port = p.get("target_port").and_then(|v| v.as_i64()).unwrap_or(port);
                    let protocol = p["protocol"].as_str().unwrap_or("TCP");
                    serde_json::json!({
                        "port": port,
                        "targetPort": target_port,
                        "protocol": protocol,
                    })
                })
                .collect();
            spec_patch.insert(
                "ports".to_string(),
                serde_json::Value::Array(ports),
            );
        }
    }

    if let Some(selector) = args.get("selector") {
        spec_patch.insert("selector".to_string(), selector.clone());
    }

    if let Some(svc_type) = args.get("type").and_then(|v| v.as_str()) {
        spec_patch.insert(
            "type".to_string(),
            serde_json::Value::String(svc_type.to_string()),
        );
    }

    if spec_patch.is_empty() {
        return Err("At least one of ports, selector, or type must be provided".to_string());
    }

    let patch = serde_json::json!({
        "spec": spec_patch,
    });

    let svc_api = api(client, ns)?;
    let patched = svc_api
        .patch(
            name,
            &PatchParams::apply("mcp-k8s"),
            &Patch::Merge(&patch),
        )
        .await
        .map_err(|e| e.to_string())?;

    let summary = extract_summary(&patched);
    serde_json::to_string_pretty(&summary).map_err(|e| e.to_string())
}

async fn delete_service(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;

    let svc_api = api(client, ns)?;
    svc_api
        .delete(name, &DeleteParams::default())
        .await
        .map_err(|e| e.to_string())?;

    let result = serde_json::json!({
        "deleted": true,
        "name": name,
        "namespace": ns,
    });
    serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::api::core::v1::{ServicePort, ServiceSpec, ServiceStatus};
    use k8s_openapi::api::core::v1::LoadBalancerStatus;
    use k8s_openapi::api::core::v1::LoadBalancerIngress;
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

    #[test]
    fn tool_definitions_returns_four_unique_tools() {
        let defs = tool_definitions();
        assert_eq!(defs.len(), 4);

        let names: Vec<&str> = defs
            .iter()
            .map(|d| d["name"].as_str().unwrap())
            .collect();

        let mut unique = names.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(names.len(), unique.len(), "tool names must be unique");

        assert!(names.contains(&"list_services"));
        assert!(names.contains(&"get_service"));
        assert!(names.contains(&"update_service"));
        assert!(names.contains(&"delete_service"));
    }

    #[test]
    fn tool_definitions_have_input_schema() {
        let defs = tool_definitions();
        for def in &defs {
            assert!(def.get("name").is_some(), "tool must have a name");
            assert!(
                def.get("description").is_some(),
                "tool must have a description"
            );
            let schema = def.get("inputSchema").expect("tool must have inputSchema");
            assert_eq!(schema["type"], "object");
            assert_eq!(schema["additionalProperties"], false);
        }
    }

    #[test]
    fn service_summary_serialization() {
        let summary = ServiceSummary {
            name: "my-svc".to_string(),
            namespace: "default".to_string(),
            service_type: "ClusterIP".to_string(),
            cluster_ip: Some("10.96.0.1".to_string()),
            external_ips: vec![],
            ports: vec![ServicePortSummary {
                port: 80,
                target_port: Some("8080".to_string()),
                protocol: "TCP".to_string(),
                name: Some("http".to_string()),
            }],
            created_at: Some("2024-01-01T00:00:00Z".to_string()),
            labels: BTreeMap::from([("app".to_string(), "web".to_string())]),
        };

        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["name"], "my-svc");
        assert_eq!(json["namespace"], "default");
        assert_eq!(json["service_type"], "ClusterIP");
        assert_eq!(json["cluster_ip"], "10.96.0.1");
        assert!(json["external_ips"].as_array().unwrap().is_empty());
        assert_eq!(json["ports"].as_array().unwrap().len(), 1);
        assert_eq!(json["ports"][0]["port"], 80);
        assert_eq!(json["ports"][0]["target_port"], "8080");
        assert_eq!(json["ports"][0]["protocol"], "TCP");
        assert_eq!(json["ports"][0]["name"], "http");
        assert_eq!(json["created_at"], "2024-01-01T00:00:00Z");
        assert_eq!(json["labels"]["app"], "web");
    }

    #[test]
    fn service_summary_serialization_empty_fields() {
        let summary = ServiceSummary {
            name: "empty".to_string(),
            namespace: "ns".to_string(),
            service_type: "ClusterIP".to_string(),
            cluster_ip: None,
            external_ips: vec![],
            ports: vec![],
            created_at: None,
            labels: BTreeMap::new(),
        };

        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["name"], "empty");
        assert!(json["cluster_ip"].is_null());
        assert!(json["ports"].as_array().unwrap().is_empty());
        assert!(json["created_at"].is_null());
        assert!(json["labels"].as_object().unwrap().is_empty());
    }

    fn make_test_service() -> Service {
        let mut labels = BTreeMap::new();
        labels.insert("app".to_string(), "web".to_string());
        labels.insert(
            "app.kubernetes.io/managed-by".to_string(),
            "mcp-k8s".to_string(),
        );

        let mut selector = BTreeMap::new();
        selector.insert("app".to_string(), "web".to_string());

        let mut annotations = BTreeMap::new();
        annotations.insert(
            "service.beta.kubernetes.io/aws-load-balancer-type".to_string(),
            "nlb".to_string(),
        );

        Service {
            metadata: ObjectMeta {
                name: Some("test-svc".to_string()),
                namespace: Some("prod".to_string()),
                labels: Some(labels),
                annotations: Some(annotations),
                ..Default::default()
            },
            spec: Some(ServiceSpec {
                type_: Some("LoadBalancer".to_string()),
                cluster_ip: Some("10.96.100.50".to_string()),
                selector: Some(selector),
                session_affinity: Some("None".to_string()),
                ports: Some(vec![
                    ServicePort {
                        name: Some("http".to_string()),
                        port: 80,
                        target_port: Some(IntOrString::Int(8080)),
                        protocol: Some("TCP".to_string()),
                        ..Default::default()
                    },
                    ServicePort {
                        name: Some("https".to_string()),
                        port: 443,
                        target_port: Some(IntOrString::Int(8443)),
                        protocol: Some("TCP".to_string()),
                        ..Default::default()
                    },
                ]),
                ..Default::default()
            }),
            status: Some(ServiceStatus {
                load_balancer: Some(LoadBalancerStatus {
                    ingress: Some(vec![LoadBalancerIngress {
                        ip: Some("203.0.113.10".to_string()),
                        ..Default::default()
                    }]),
                }),
                ..Default::default()
            }),
        }
    }

    #[test]
    fn extract_summary_from_service() {
        let svc = make_test_service();
        let summary = extract_summary(&svc);

        assert_eq!(summary.name, "test-svc");
        assert_eq!(summary.namespace, "prod");
        assert_eq!(summary.service_type, "LoadBalancer");
        assert_eq!(summary.cluster_ip.as_deref(), Some("10.96.100.50"));
        assert_eq!(summary.external_ips, vec!["203.0.113.10".to_string()]);
        assert_eq!(summary.ports.len(), 2);

        assert_eq!(summary.ports[0].port, 80);
        assert_eq!(summary.ports[0].target_port.as_deref(), Some("8080"));
        assert_eq!(summary.ports[0].protocol, "TCP");
        assert_eq!(summary.ports[0].name.as_deref(), Some("http"));

        assert_eq!(summary.ports[1].port, 443);
        assert_eq!(summary.ports[1].target_port.as_deref(), Some("8443"));
        assert_eq!(summary.ports[1].protocol, "TCP");
        assert_eq!(summary.ports[1].name.as_deref(), Some("https"));

        assert_eq!(summary.labels.get("app").unwrap(), "web");
        assert!(summary.created_at.is_none()); // no timestamp set
    }

    #[test]
    fn extract_summary_from_minimal_service() {
        let svc = Service {
            metadata: ObjectMeta {
                name: Some("minimal".to_string()),
                namespace: Some("default".to_string()),
                ..Default::default()
            },
            ..Default::default()
        };

        let summary = extract_summary(&svc);
        assert_eq!(summary.name, "minimal");
        assert_eq!(summary.namespace, "default");
        assert_eq!(summary.service_type, "ClusterIP");
        assert!(summary.cluster_ip.is_none());
        assert!(summary.external_ips.is_empty());
        assert!(summary.ports.is_empty());
        assert!(summary.labels.is_empty());
    }

    #[test]
    fn extract_external_ips_from_lb_hostname() {
        let svc = Service {
            metadata: ObjectMeta {
                name: Some("lb-svc".to_string()),
                namespace: Some("default".to_string()),
                ..Default::default()
            },
            spec: Some(ServiceSpec {
                type_: Some("LoadBalancer".to_string()),
                ..Default::default()
            }),
            status: Some(ServiceStatus {
                load_balancer: Some(LoadBalancerStatus {
                    ingress: Some(vec![LoadBalancerIngress {
                        hostname: Some("a1234.elb.amazonaws.com".to_string()),
                        ..Default::default()
                    }]),
                }),
                ..Default::default()
            }),
        };

        let ips = extract_external_ips(&svc);
        assert_eq!(ips, vec!["a1234.elb.amazonaws.com".to_string()]);
    }

    #[test]
    fn extract_port_with_string_target() {
        let port = ServicePort {
            name: Some("grpc".to_string()),
            port: 9090,
            target_port: Some(IntOrString::String("grpc-port".to_string())),
            protocol: Some("TCP".to_string()),
            ..Default::default()
        };

        let summary = extract_port_summary(&port);
        assert_eq!(summary.port, 9090);
        assert_eq!(summary.target_port.as_deref(), Some("grpc-port"));
        assert_eq!(summary.protocol, "TCP");
        assert_eq!(summary.name.as_deref(), Some("grpc"));
    }
}
