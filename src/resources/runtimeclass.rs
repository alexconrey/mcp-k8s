use std::collections::BTreeMap;

use k8s_openapi::api::node::v1::RuntimeClass;
use kube::api::ListParams;
use serde::Serialize;

use crate::client::K8sClient;

fn api(client: &K8sClient) -> kube::Api<RuntimeClass> {
    kube::Api::all(client.inner().clone())
}

#[derive(Serialize, Debug)]
pub struct RuntimeClassSummary {
    pub name: String,
    pub handler: String,
    pub created_at: Option<String>,
}

fn extract_summary(rc: &RuntimeClass) -> RuntimeClassSummary {
    let meta = &rc.metadata;

    let created_at = meta.creation_timestamp.as_ref().map(|t| t.0.to_string());

    RuntimeClassSummary {
        name: meta.name.clone().unwrap_or_default(),
        handler: rc.handler.clone(),
        created_at,
    }
}

pub fn tool_definitions() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "name": "list_runtimeclasses",
            "description": "List all RuntimeClasses in the cluster. Returns name, handler, and created_at.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "required": [],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "get_runtimeclass",
            "description": "Get a RuntimeClass by name. Returns name, handler, created_at, scheduling (node_selector, tolerations), overhead, labels, and annotations.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "RuntimeClass name" }
                },
                "required": ["name"],
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
        "list_runtimeclasses" => list_runtimeclasses(client).await,
        "get_runtimeclass" => get_runtimeclass(client, args).await,
        _ => return None,
    };
    Some(result)
}

async fn list_runtimeclasses(client: &K8sClient) -> Result<String, String> {
    let rc_api = api(client);
    let list = rc_api
        .list(&ListParams::default())
        .await
        .map_err(|e| e.to_string())?;

    let summaries: Vec<RuntimeClassSummary> = list.iter().map(|rc| extract_summary(rc)).collect();

    serde_json::to_string_pretty(&summaries).map_err(|e| e.to_string())
}

async fn get_runtimeclass(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let name = args["name"].as_str().ok_or("name is required")?;

    let rc_api = api(client);
    let rc = rc_api.get(name).await.map_err(|e| e.to_string())?;

    let summary = extract_summary(&rc);
    let meta = &rc.metadata;

    let scheduling = rc.scheduling.as_ref().map(|s| {
        serde_json::json!({
            "node_selector": s.node_selector.clone().unwrap_or_default(),
            "tolerations": s.tolerations.as_ref().map(|tols| {
                tols.iter().map(|t| serde_json::json!({
                    "key": t.key,
                    "operator": t.operator,
                    "value": t.value,
                    "effect": t.effect,
                    "toleration_seconds": t.toleration_seconds,
                })).collect::<Vec<_>>()
            }).unwrap_or_default(),
        })
    });

    let overhead = rc.overhead.as_ref().map(|o| {
        let pod_fixed: BTreeMap<String, String> = o
            .pod_fixed
            .as_ref()
            .map(|pf| pf.iter().map(|(k, v)| (k.clone(), v.0.clone())).collect())
            .unwrap_or_default();
        serde_json::json!({ "pod_fixed": pod_fixed })
    });

    let result = serde_json::json!({
        "name": summary.name,
        "handler": summary.handler,
        "created_at": summary.created_at,
        "scheduling": scheduling,
        "overhead": overhead,
        "labels": meta.labels.clone().unwrap_or_default(),
        "annotations": meta.annotations.clone().unwrap_or_default(),
    });

    serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::api::core::v1::Toleration;
    use k8s_openapi::api::node::v1::{Overhead, Scheduling};
    use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
    use std::collections::BTreeMap;

    #[test]
    fn tool_definitions_returns_two_tools() {
        let defs = tool_definitions();
        assert_eq!(defs.len(), 2);

        let names: Vec<&str> = defs.iter().map(|d| d["name"].as_str().unwrap()).collect();

        let mut unique = names.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(names.len(), unique.len(), "tool names must be unique");

        assert!(names.contains(&"list_runtimeclasses"));
        assert!(names.contains(&"get_runtimeclass"));
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
    fn runtimeclass_summary_serialization() {
        let summary = RuntimeClassSummary {
            name: "gvisor".to_string(),
            handler: "runsc".to_string(),
            created_at: Some("2024-01-01T00:00:00Z".to_string()),
        };

        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["name"], "gvisor");
        assert_eq!(json["handler"], "runsc");
        assert_eq!(json["created_at"], "2024-01-01T00:00:00Z");
    }

    #[test]
    fn runtimeclass_summary_serialization_minimal() {
        let summary = RuntimeClassSummary {
            name: "runc".to_string(),
            handler: "runc".to_string(),
            created_at: None,
        };

        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["name"], "runc");
        assert_eq!(json["handler"], "runc");
        assert!(json["created_at"].is_null());
    }

    fn make_test_runtimeclass() -> RuntimeClass {
        let mut labels = BTreeMap::new();
        labels.insert("app".to_string(), "gvisor".to_string());

        let mut annotations = BTreeMap::new();
        annotations.insert(
            "example.com/description".to_string(),
            "gVisor sandbox runtime".to_string(),
        );

        let mut node_selector = BTreeMap::new();
        node_selector.insert(
            "runtime.kubernetes.io/gvisor".to_string(),
            "true".to_string(),
        );

        let mut pod_fixed = BTreeMap::new();
        pod_fixed.insert("cpu".to_string(), Quantity("100m".to_string()));
        pod_fixed.insert("memory".to_string(), Quantity("64Mi".to_string()));

        RuntimeClass {
            metadata: ObjectMeta {
                name: Some("gvisor".to_string()),
                labels: Some(labels),
                annotations: Some(annotations),
                ..Default::default()
            },
            handler: "runsc".to_string(),
            overhead: Some(Overhead {
                pod_fixed: Some(pod_fixed),
            }),
            scheduling: Some(Scheduling {
                node_selector: Some(node_selector),
                tolerations: Some(vec![Toleration {
                    key: Some("runtime".to_string()),
                    operator: Some("Equal".to_string()),
                    value: Some("gvisor".to_string()),
                    effect: Some("NoSchedule".to_string()),
                    toleration_seconds: None,
                }]),
            }),
        }
    }

    #[test]
    fn extract_summary_from_runtimeclass() {
        let rc = make_test_runtimeclass();
        let summary = extract_summary(&rc);

        assert_eq!(summary.name, "gvisor");
        assert_eq!(summary.handler, "runsc");
    }

    #[test]
    fn extract_summary_from_minimal_runtimeclass() {
        let rc = RuntimeClass {
            metadata: ObjectMeta {
                name: Some("runc".to_string()),
                ..Default::default()
            },
            handler: "runc".to_string(),
            overhead: None,
            scheduling: None,
        };

        let summary = extract_summary(&rc);
        assert_eq!(summary.name, "runc");
        assert_eq!(summary.handler, "runc");
        assert!(summary.created_at.is_none());
    }

    #[test]
    fn extract_summary_preserves_handler() {
        let rc = RuntimeClass {
            metadata: ObjectMeta {
                name: Some("kata".to_string()),
                ..Default::default()
            },
            handler: "kata-runtime".to_string(),
            overhead: None,
            scheduling: None,
        };

        let summary = extract_summary(&rc);
        assert_eq!(summary.name, "kata");
        assert_eq!(summary.handler, "kata-runtime");
    }
}
