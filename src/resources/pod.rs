use std::collections::BTreeMap;

use k8s_openapi::api::core::v1::{Container, Pod, PodSpec};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::api::{AttachParams, DeleteParams, EvictParams, ListParams, PostParams};
use serde::Serialize;
use tokio::io::AsyncReadExt;

use crate::client::K8sClient;
use crate::extract::pod_summary;
use crate::types::PodSummary;

fn api(client: &K8sClient, ns: &str) -> Result<kube::Api<Pod>, String> {
    if !client.is_namespace_allowed(ns) {
        return Err(format!("Namespace '{ns}' is not in the allowed list"));
    }
    Ok(kube::Api::namespaced(client.inner().clone(), ns))
}

#[derive(Serialize, Debug)]
pub struct PodDetail {
    #[serde(flatten)]
    pub summary: PodSummary,
    pub namespace: String,
    pub volumes: Vec<String>,
    pub service_account: Option<String>,
    pub node_selector: BTreeMap<String, String>,
    pub tolerations: Vec<TolerationSummary>,
    pub annotations: BTreeMap<String, String>,
}

#[derive(Serialize, Debug)]
pub struct TolerationSummary {
    pub key: Option<String>,
    pub operator: Option<String>,
    pub value: Option<String>,
    pub effect: Option<String>,
    pub toleration_seconds: Option<i64>,
}

fn extract_detail(pod: &Pod) -> PodDetail {
    let summary = pod_summary(pod);
    let meta = &pod.metadata;
    let spec = pod.spec.as_ref();

    let volumes = spec
        .map(|s| {
            s.volumes
                .as_ref()
                .map(|vols| vols.iter().map(|v| v.name.clone()).collect())
                .unwrap_or_default()
        })
        .unwrap_or_default();

    let service_account = spec.and_then(|s| s.service_account_name.clone());

    let node_selector = spec
        .and_then(|s| s.node_selector.clone())
        .unwrap_or_default();

    let tolerations = spec
        .and_then(|s| s.tolerations.as_ref())
        .map(|tols| {
            tols.iter()
                .map(|t| TolerationSummary {
                    key: t.key.clone(),
                    operator: t.operator.clone(),
                    value: t.value.clone(),
                    effect: t.effect.clone(),
                    toleration_seconds: t.toleration_seconds,
                })
                .collect()
        })
        .unwrap_or_default();

    let annotations = meta.annotations.clone().unwrap_or_default();

    PodDetail {
        summary,
        namespace: meta.namespace.clone().unwrap_or_default(),
        volumes,
        service_account,
        node_selector,
        tolerations,
        annotations,
    }
}

pub fn tool_definitions() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "name": "list_pods",
            "description": "List pods in a namespace. Returns name, phase, readiness, restart count, node, and container statuses. Optionally filter by label selector.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "label_selector": { "type": "string", "description": "Label selector to filter pods (e.g. app=nginx)" },
                    "field_selector": { "type": "string", "description": "Field selector to filter results (e.g. metadata.name=foo, status.phase=Running, spec.nodeName=node1)" }
                },
                "required": ["namespace"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "get_pod",
            "description": "Get a pod by name. Returns detailed information including phase, containers, volumes, service account, node selector, tolerations, and annotations.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "Pod name" }
                },
                "required": ["namespace", "name"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "create_pod",
            "description": "Create a standalone pod. Useful for debugging, one-shot tasks, or running a quick container.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "Pod name" },
                    "image": { "type": "string", "description": "Container image" },
                    "command": { "type": "array", "items": { "type": "string" }, "description": "Command to run (optional)" },
                    "restart_policy": { "type": "string", "description": "Restart policy (default: Never)", "enum": ["Never", "Always", "OnFailure"] }
                },
                "required": ["namespace", "name", "image"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "delete_pod",
            "description": "Delete a pod by name from a namespace. Optionally specify a grace period in seconds.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "Pod name" },
                    "grace_period_seconds": { "type": "integer", "description": "Grace period in seconds before force-killing the pod" }
                },
                "required": ["namespace", "name"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "evict_pod",
            "description": "Evict a pod respecting PodDisruptionBudgets. Safer than delete for production pods.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "Pod name" },
                    "grace_period_seconds": { "type": "integer", "description": "Grace period in seconds (optional)" }
                },
                "required": ["namespace", "name"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "exec_pod",
            "description": "Execute a command in a pod container and return the output. Requires the 'ws' feature.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "Pod name" },
                    "command": { "type": "array", "items": { "type": "string" }, "description": "Command to execute (e.g. [\"ls\", \"-la\"])" },
                    "container": { "type": "string", "description": "Container name (optional, defaults to first)" }
                },
                "required": ["namespace", "name", "command"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "port_forward_check",
            "description": "Check if a port on a pod is reachable by establishing and immediately closing a port-forward tunnel.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "Pod name" },
                    "port": { "type": "integer", "description": "Container port to check" }
                },
                "required": ["namespace", "name", "port"],
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
        "list_pods" => list_pods(client, args).await,
        "get_pod" => get_pod(client, args).await,
        "create_pod" => create_pod(client, args).await,
        "delete_pod" => delete_pod(client, args).await,
        "evict_pod" => evict_pod(client, args).await,
        "exec_pod" => exec_pod(client, args).await,
        "port_forward_check" => port_forward_check(client, args).await,
        _ => return None,
    };
    Some(result)
}

async fn list_pods(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let pod_api = api(client, ns)?;

    let mut lp = ListParams::default();
    if let Some(sel) = args["label_selector"].as_str() {
        lp = lp.labels(sel);
    }
    if let Some(sel) = args["field_selector"].as_str() {
        lp = lp.fields(sel);
    }

    let list = pod_api.list(&lp).await.map_err(|e| e.to_string())?;

    let summaries: Vec<serde_json::Value> = list
        .iter()
        .map(|pod| serde_json::to_value(pod_summary(pod)).unwrap_or_default())
        .collect();

    serde_json::to_string_pretty(&summaries).map_err(|e| e.to_string())
}

async fn get_pod(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;
    let pod_api = api(client, ns)?;
    let pod = pod_api.get(name).await.map_err(|e| e.to_string())?;

    let detail = extract_detail(&pod);
    serde_json::to_string_pretty(&detail).map_err(|e| e.to_string())
}

async fn create_pod(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;
    let image = args["image"].as_str().ok_or("image is required")?;
    let command: Option<Vec<String>> = args
        .get("command")
        .and_then(|v| serde_json::from_value(v.clone()).ok());
    let restart_policy = args["restart_policy"]
        .as_str()
        .unwrap_or("Never")
        .to_string();

    let mut labels = BTreeMap::new();
    labels.insert("app".to_string(), name.to_string());
    labels.insert(
        "app.kubernetes.io/managed-by".to_string(),
        "mcp-k8s".to_string(),
    );

    let pod = Pod {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            namespace: Some(ns.to_string()),
            labels: Some(labels),
            ..Default::default()
        },
        spec: Some(PodSpec {
            containers: vec![Container {
                name: name.to_string(),
                image: Some(image.to_string()),
                command,
                ..Default::default()
            }],
            restart_policy: Some(restart_policy),
            ..Default::default()
        }),
        ..Default::default()
    };

    let pod_api = api(client, ns)?;
    let created = pod_api
        .create(&PostParams::default(), &pod)
        .await
        .map_err(|e| e.to_string())?;

    let result = serde_json::json!({
        "name": created.metadata.name,
        "namespace": ns,
        "phase": created.status.as_ref().and_then(|s| s.phase.clone()).unwrap_or_else(|| "Pending".to_string()),
    });
    serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
}

async fn delete_pod(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;

    let mut dp = DeleteParams::default();
    if let Some(grace) = args["grace_period_seconds"].as_i64() {
        dp = dp.grace_period(grace as u32);
    }

    let pod_api = api(client, ns)?;
    pod_api
        .delete(name, &dp)
        .await
        .map_err(|e| e.to_string())?;

    let result = serde_json::json!({
        "deleted": name,
        "namespace": ns,
    });
    serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
}

async fn evict_pod(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;

    let mut ep = EvictParams::default();
    if let Some(grace) = args["grace_period_seconds"].as_i64() {
        let dp = DeleteParams::default().grace_period(grace as u32);
        ep.delete_options = Some(dp);
    }

    let pod_api = api(client, ns)?;
    pod_api
        .evict(name, &ep)
        .await
        .map_err(|e| e.to_string())?;

    let result = serde_json::json!({
        "evicted": name,
        "namespace": ns,
    });
    serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
}

async fn exec_pod(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;
    let command: Vec<String> = serde_json::from_value(args["command"].clone())
        .map_err(|e| format!("invalid command: {e}"))?;
    let container = args["container"].as_str().map(|s| s.to_string());

    let pods_api = api(client, ns)?;

    let ap = AttachParams {
        stdout: true,
        stderr: true,
        stdin: false,
        tty: false,
        container,
        ..AttachParams::default()
    };

    let mut process = pods_api
        .exec(name, command, &ap)
        .await
        .map_err(|e| e.to_string())?;

    let mut stdout_str = String::new();
    if let Some(mut stdout) = process.stdout() {
        stdout
            .read_to_string(&mut stdout_str)
            .await
            .map_err(|e| e.to_string())?;
    }

    let mut stderr_str = String::new();
    if let Some(mut stderr) = process.stderr() {
        stderr
            .read_to_string(&mut stderr_str)
            .await
            .map_err(|e| e.to_string())?;
    }

    let status = process.take_status().unwrap().await;
    let exit_code = status.as_ref().and_then(|s| {
        s.status
            .as_ref()
            .and_then(|st| if st == "Success" { Some(0i32) } else { None })
    });

    let result = serde_json::json!({
        "stdout": stdout_str,
        "stderr": stderr_str,
        "exit_code": exit_code,
    });
    serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
}

async fn port_forward_check(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;
    let port: u16 = args["port"]
        .as_i64()
        .ok_or("port is required")?
        .try_into()
        .map_err(|_| "port must be a valid u16 (0-65535)".to_string())?;

    let pods_api = api(client, ns)?;

    // Attempt to establish a port-forward tunnel
    let mut pf = pods_api
        .portforward(name, &[port])
        .await
        .map_err(|e| e.to_string())?;

    // Take the stream to verify the connection was established
    let stream = pf.take_stream(port);
    let reachable = stream.is_some();

    // Drop the stream and port-forwarder to close the tunnel immediately
    drop(stream);
    drop(pf);

    let result = serde_json::json!({
        "pod": name,
        "namespace": ns,
        "port": port,
        "reachable": reachable,
    });
    serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::api::core::v1::{
        Container, ContainerState, ContainerStateRunning, ContainerStatus, EmptyDirVolumeSource,
        PodCondition, PodSpec, PodStatus, Toleration, Volume,
    };
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

    #[test]
    fn tool_definitions_returns_seven_tools() {
        let defs = tool_definitions();
        assert_eq!(defs.len(), 7);

        let names: Vec<&str> = defs
            .iter()
            .map(|d| d["name"].as_str().unwrap())
            .collect();

        let mut unique = names.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(names.len(), unique.len(), "tool names must be unique");

        assert!(names.contains(&"list_pods"));
        assert!(names.contains(&"get_pod"));
        assert!(names.contains(&"create_pod"));
        assert!(names.contains(&"delete_pod"));
        assert!(names.contains(&"evict_pod"));
        assert!(names.contains(&"exec_pod"));
        assert!(names.contains(&"port_forward_check"));
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
    fn pod_detail_serialization() {
        let detail = PodDetail {
            summary: PodSummary {
                name: "my-pod".to_string(),
                phase: "Running".to_string(),
                ready: true,
                restart_count: 0,
                node: Some("node-1".to_string()),
                started_at: Some("2024-01-01T00:00:00Z".to_string()),
                conditions: vec![],
                container_statuses: vec![],
                oom_killed: false,
            },
            namespace: "default".to_string(),
            volumes: vec!["data-vol".to_string(), "config-vol".to_string()],
            service_account: Some("my-sa".to_string()),
            node_selector: BTreeMap::from([("disktype".to_string(), "ssd".to_string())]),
            tolerations: vec![TolerationSummary {
                key: Some("key1".to_string()),
                operator: Some("Equal".to_string()),
                value: Some("value1".to_string()),
                effect: Some("NoSchedule".to_string()),
                toleration_seconds: None,
            }],
            annotations: BTreeMap::from([("note".to_string(), "test".to_string())]),
        };

        let json = serde_json::to_value(&detail).unwrap();
        // Flattened summary fields
        assert_eq!(json["name"], "my-pod");
        assert_eq!(json["phase"], "Running");
        assert_eq!(json["ready"], true);
        assert_eq!(json["node"], "node-1");
        // Detail-specific fields
        assert_eq!(json["namespace"], "default");
        assert_eq!(json["volumes"].as_array().unwrap().len(), 2);
        assert_eq!(json["service_account"], "my-sa");
        assert_eq!(json["node_selector"]["disktype"], "ssd");
        assert_eq!(json["tolerations"].as_array().unwrap().len(), 1);
        assert_eq!(json["tolerations"][0]["key"], "key1");
        assert_eq!(json["annotations"]["note"], "test");
    }

    #[test]
    fn pod_detail_serialization_empty_fields() {
        let detail = PodDetail {
            summary: PodSummary {
                name: "bare-pod".to_string(),
                phase: "Pending".to_string(),
                ready: false,
                restart_count: 0,
                node: None,
                started_at: None,
                conditions: vec![],
                container_statuses: vec![],
                oom_killed: false,
            },
            namespace: "ns".to_string(),
            volumes: vec![],
            service_account: None,
            node_selector: BTreeMap::new(),
            tolerations: vec![],
            annotations: BTreeMap::new(),
        };

        let json = serde_json::to_value(&detail).unwrap();
        assert_eq!(json["name"], "bare-pod");
        assert!(json["node"].is_null());
        assert!(json["service_account"].is_null());
        assert!(json["volumes"].as_array().unwrap().is_empty());
        assert!(json["node_selector"].as_object().unwrap().is_empty());
        assert!(json["tolerations"].as_array().unwrap().is_empty());
        assert!(json["annotations"].as_object().unwrap().is_empty());
    }

    #[test]
    fn extract_detail_from_constructed_pod() {
        let pod = Pod {
            metadata: ObjectMeta {
                name: Some("test-pod".to_string()),
                namespace: Some("prod".to_string()),
                annotations: Some(BTreeMap::from([(
                    "kubectl.kubernetes.io/last-applied".to_string(),
                    "{}".to_string(),
                )])),
                ..Default::default()
            },
            spec: Some(PodSpec {
                containers: vec![Container {
                    name: "app".to_string(),
                    image: Some("nginx:1.25".to_string()),
                    ..Default::default()
                }],
                service_account_name: Some("my-service-account".to_string()),
                node_name: Some("worker-1".to_string()),
                node_selector: Some(BTreeMap::from([(
                    "zone".to_string(),
                    "us-east-1a".to_string(),
                )])),
                volumes: Some(vec![Volume {
                    name: "cache-volume".to_string(),
                    empty_dir: Some(EmptyDirVolumeSource::default()),
                    ..Default::default()
                }]),
                tolerations: Some(vec![
                    Toleration {
                        key: Some("dedicated".to_string()),
                        operator: Some("Equal".to_string()),
                        value: Some("gpu".to_string()),
                        effect: Some("NoSchedule".to_string()),
                        toleration_seconds: None,
                    },
                    Toleration {
                        key: Some("node.kubernetes.io/not-ready".to_string()),
                        operator: Some("Exists".to_string()),
                        effect: Some("NoExecute".to_string()),
                        toleration_seconds: Some(300),
                        ..Default::default()
                    },
                ]),
                ..Default::default()
            }),
            status: Some(PodStatus {
                phase: Some("Running".to_string()),
                start_time: None,
                container_statuses: Some(vec![ContainerStatus {
                    name: "app".to_string(),
                    ready: true,
                    restart_count: 2,
                    image: "nginx:1.25".to_string(),
                    image_id: "sha256:abc".to_string(),
                    state: Some(ContainerState {
                        running: Some(ContainerStateRunning { started_at: None }),
                        ..Default::default()
                    }),
                    ..Default::default()
                }]),
                conditions: Some(vec![PodCondition {
                    type_: "Ready".to_string(),
                    status: "True".to_string(),
                    ..Default::default()
                }]),
                ..Default::default()
            }),
        };

        let detail = extract_detail(&pod);

        // Summary fields
        assert_eq!(detail.summary.name, "test-pod");
        assert_eq!(detail.summary.phase, "Running");
        assert!(detail.summary.ready);
        assert_eq!(detail.summary.restart_count, 2);
        assert_eq!(detail.summary.node, Some("worker-1".to_string()));
        assert!(!detail.summary.oom_killed);
        assert_eq!(detail.summary.container_statuses.len(), 1);
        assert_eq!(detail.summary.container_statuses[0].name, "app");
        assert_eq!(detail.summary.container_statuses[0].image, "nginx:1.25");

        // Detail fields
        assert_eq!(detail.namespace, "prod");
        assert_eq!(detail.volumes, vec!["cache-volume".to_string()]);
        assert_eq!(
            detail.service_account,
            Some("my-service-account".to_string())
        );
        assert_eq!(detail.node_selector.get("zone").unwrap(), "us-east-1a");
        assert_eq!(detail.tolerations.len(), 2);
        assert_eq!(detail.tolerations[0].key, Some("dedicated".to_string()));
        assert_eq!(detail.tolerations[0].effect, Some("NoSchedule".to_string()));
        assert_eq!(detail.tolerations[1].toleration_seconds, Some(300));
        assert!(detail
            .annotations
            .contains_key("kubectl.kubernetes.io/last-applied"));
    }

    #[test]
    fn extract_detail_from_minimal_pod() {
        let pod = Pod {
            metadata: ObjectMeta {
                name: Some("minimal".to_string()),
                namespace: Some("default".to_string()),
                ..Default::default()
            },
            spec: Some(PodSpec {
                containers: vec![Container {
                    name: "main".to_string(),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        };

        let detail = extract_detail(&pod);
        assert_eq!(detail.summary.name, "minimal");
        assert_eq!(detail.summary.phase, "Unknown");
        assert!(!detail.summary.ready);
        assert_eq!(detail.namespace, "default");
        assert!(detail.volumes.is_empty());
        assert!(detail.service_account.is_none());
        assert!(detail.node_selector.is_empty());
        assert!(detail.tolerations.is_empty());
        assert!(detail.annotations.is_empty());
    }
}
