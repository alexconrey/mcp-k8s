use std::collections::BTreeMap;

use k8s_openapi::api::core::v1::Namespace;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::api::{DeleteParams, Patch, PatchParams, PostParams};
use serde::Serialize;

use crate::client::K8sClient;

fn api(client: &K8sClient) -> kube::Api<Namespace> {
    kube::Api::all(client.inner().clone())
}

#[derive(Serialize, Debug)]
pub struct NamespaceDetail {
    pub name: String,
    pub status: String,
    pub labels: BTreeMap<String, String>,
    pub annotations: BTreeMap<String, String>,
    pub created_at: Option<String>,
    pub finalizers: Vec<String>,
}

fn extract_detail(ns: &Namespace) -> NamespaceDetail {
    let meta = &ns.metadata;
    let status = ns
        .status
        .as_ref()
        .and_then(|s| s.phase.clone())
        .unwrap_or_else(|| "Unknown".to_string());
    let finalizers = meta.finalizers.clone().unwrap_or_default();

    NamespaceDetail {
        name: meta.name.clone().unwrap_or_default(),
        status,
        labels: meta.labels.clone().unwrap_or_default(),
        annotations: meta.annotations.clone().unwrap_or_default(),
        created_at: meta.creation_timestamp.as_ref().map(|t| t.0.to_string()),
        finalizers,
    }
}

pub fn tool_definitions() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "name": "get_namespace",
            "description": "Get a namespace by name. Returns name, status (Active/Terminating), labels, annotations, created_at, and finalizers.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Namespace name" }
                },
                "required": ["name"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "create_namespace",
            "description": "Create a new Kubernetes namespace.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Namespace name" },
                    "labels": {
                        "type": "object",
                        "description": "Optional labels to apply to the namespace",
                        "additionalProperties": { "type": "string" }
                    }
                },
                "required": ["name"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "update_namespace",
            "description": "Update a namespace's labels and/or annotations.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Namespace name" },
                    "labels": {
                        "type": "object",
                        "description": "Labels to set/update",
                        "additionalProperties": { "type": "string" }
                    },
                    "annotations": {
                        "type": "object",
                        "description": "Annotations to set/update",
                        "additionalProperties": { "type": "string" }
                    }
                },
                "required": ["name"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "delete_namespace",
            "description": "Delete a namespace by name.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Namespace name" }
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
        "get_namespace" => get_namespace(client, args).await,
        "create_namespace" => create_namespace(client, args).await,
        "update_namespace" => update_namespace(client, args).await,
        "delete_namespace" => delete_namespace(client, args).await,
        _ => return None,
    };
    Some(result)
}

async fn get_namespace(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let name = args["name"].as_str().ok_or("name is required")?;

    if !client.is_namespace_allowed(name) {
        return Err(format!("Namespace '{name}' is not in the allowed list"));
    }

    let ns_api = api(client);
    let ns = ns_api.get(name).await.map_err(|e| e.to_string())?;
    let detail = extract_detail(&ns);
    serde_json::to_string_pretty(&detail).map_err(|e| e.to_string())
}

async fn create_namespace(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let name = args["name"].as_str().ok_or("name is required")?;

    let mut labels: BTreeMap<String, String> = args
        .get("labels")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();
    labels.insert(
        "app.kubernetes.io/managed-by".to_string(),
        "mcp-k8s".to_string(),
    );

    let ns = Namespace {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            labels: Some(labels),
            ..Default::default()
        },
        ..Default::default()
    };

    let ns_api = api(client);
    let created = ns_api
        .create(&PostParams::default(), &ns)
        .await
        .map_err(|e| e.to_string())?;

    let detail = extract_detail(&created);
    serde_json::to_string_pretty(&detail).map_err(|e| e.to_string())
}

async fn update_namespace(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let name = args["name"].as_str().ok_or("name is required")?;

    let labels: Option<BTreeMap<String, String>> = args
        .get("labels")
        .and_then(|v| serde_json::from_value(v.clone()).ok());
    let annotations: Option<BTreeMap<String, String>> = args
        .get("annotations")
        .and_then(|v| serde_json::from_value(v.clone()).ok());

    let patch = serde_json::json!({
        "metadata": {
            "labels": labels,
            "annotations": annotations,
        }
    });

    let ns_api = api(client);
    let patched = ns_api
        .patch(name, &PatchParams::apply("mcp-k8s"), &Patch::Merge(&patch))
        .await
        .map_err(|e| e.to_string())?;

    let detail = extract_detail(&patched);
    serde_json::to_string_pretty(&detail).map_err(|e| e.to_string())
}

async fn delete_namespace(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let name = args["name"].as_str().ok_or("name is required")?;

    let ns_api = api(client);
    ns_api
        .delete(name, &DeleteParams::default())
        .await
        .map_err(|e| e.to_string())?;

    let result = serde_json::json!({
        "deleted": name,
    });
    serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_definitions_returns_four_tools() {
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

        assert!(names.contains(&"get_namespace"));
        assert!(names.contains(&"create_namespace"));
        assert!(names.contains(&"delete_namespace"));
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
    fn namespace_detail_serialization() {
        let detail = NamespaceDetail {
            name: "production".to_string(),
            status: "Active".to_string(),
            labels: BTreeMap::from([
                ("env".to_string(), "prod".to_string()),
            ]),
            annotations: BTreeMap::from([
                ("note".to_string(), "critical".to_string()),
            ]),
            created_at: Some("2024-01-01T00:00:00Z".to_string()),
            finalizers: vec!["kubernetes".to_string()],
        };

        let json = serde_json::to_value(&detail).unwrap();
        assert_eq!(json["name"], "production");
        assert_eq!(json["status"], "Active");
        assert_eq!(json["labels"]["env"], "prod");
        assert_eq!(json["annotations"]["note"], "critical");
        assert_eq!(json["created_at"], "2024-01-01T00:00:00Z");
        assert_eq!(json["finalizers"].as_array().unwrap().len(), 1);
        assert_eq!(json["finalizers"][0], "kubernetes");
    }

    #[test]
    fn namespace_detail_serialization_empty_fields() {
        let detail = NamespaceDetail {
            name: "empty".to_string(),
            status: "Active".to_string(),
            labels: BTreeMap::new(),
            annotations: BTreeMap::new(),
            created_at: None,
            finalizers: vec![],
        };

        let json = serde_json::to_value(&detail).unwrap();
        assert_eq!(json["name"], "empty");
        assert_eq!(json["status"], "Active");
        assert!(json["labels"].as_object().unwrap().is_empty());
        assert!(json["annotations"].as_object().unwrap().is_empty());
        assert!(json["created_at"].is_null());
        assert!(json["finalizers"].as_array().unwrap().is_empty());
    }

    #[test]
    fn extract_detail_from_namespace() {
        let ns = Namespace {
            metadata: ObjectMeta {
                name: Some("test-ns".to_string()),
                labels: Some(BTreeMap::from([
                    ("env".to_string(), "staging".to_string()),
                ])),
                annotations: Some(BTreeMap::from([
                    ("description".to_string(), "test namespace".to_string()),
                ])),
                finalizers: Some(vec!["kubernetes".to_string()]),
                ..Default::default()
            },
            status: Some(k8s_openapi::api::core::v1::NamespaceStatus {
                phase: Some("Active".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };

        let detail = extract_detail(&ns);
        assert_eq!(detail.name, "test-ns");
        assert_eq!(detail.status, "Active");
        assert_eq!(detail.labels.get("env").unwrap(), "staging");
        assert_eq!(detail.annotations.get("description").unwrap(), "test namespace");
        assert!(detail.created_at.is_none());
        assert_eq!(detail.finalizers, vec!["kubernetes".to_string()]);
    }

    #[test]
    fn extract_detail_from_empty_namespace() {
        let ns = Namespace {
            metadata: ObjectMeta {
                name: Some("minimal-ns".to_string()),
                ..Default::default()
            },
            ..Default::default()
        };

        let detail = extract_detail(&ns);
        assert_eq!(detail.name, "minimal-ns");
        assert_eq!(detail.status, "Unknown");
        assert!(detail.labels.is_empty());
        assert!(detail.annotations.is_empty());
        assert!(detail.created_at.is_none());
        assert!(detail.finalizers.is_empty());
    }

    #[test]
    fn extract_detail_terminating_namespace() {
        let ns = Namespace {
            metadata: ObjectMeta {
                name: Some("dying-ns".to_string()),
                finalizers: Some(vec![
                    "kubernetes".to_string(),
                    "custom-finalizer".to_string(),
                ]),
                ..Default::default()
            },
            status: Some(k8s_openapi::api::core::v1::NamespaceStatus {
                phase: Some("Terminating".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };

        let detail = extract_detail(&ns);
        assert_eq!(detail.name, "dying-ns");
        assert_eq!(detail.status, "Terminating");
        assert_eq!(detail.finalizers.len(), 2);
        assert!(detail.finalizers.contains(&"kubernetes".to_string()));
        assert!(detail.finalizers.contains(&"custom-finalizer".to_string()));
    }
}
