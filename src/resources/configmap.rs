use std::collections::BTreeMap;

use k8s_openapi::api::core::v1::ConfigMap;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::api::{DeleteParams, ListParams, Patch, PatchParams, PostParams};
use serde::Serialize;

use crate::client::K8sClient;

fn api(client: &K8sClient, ns: &str) -> Result<kube::Api<ConfigMap>, String> {
    if !client.is_namespace_allowed(ns) {
        return Err(format!("Namespace '{ns}' is not in the allowed list"));
    }
    Ok(kube::Api::namespaced(client.inner().clone(), ns))
}

#[derive(Serialize, Debug)]
pub struct ConfigMapSummary {
    pub name: String,
    pub namespace: String,
    pub data_keys: Vec<String>,
    pub created_at: Option<String>,
    pub labels: BTreeMap<String, String>,
}

fn extract_summary(cm: &ConfigMap) -> ConfigMapSummary {
    let meta = &cm.metadata;
    let data_keys = cm
        .data
        .as_ref()
        .map(|d| d.keys().cloned().collect())
        .unwrap_or_default();
    ConfigMapSummary {
        name: meta.name.clone().unwrap_or_default(),
        namespace: meta.namespace.clone().unwrap_or_default(),
        data_keys,
        created_at: meta.creation_timestamp.as_ref().map(|t| t.0.to_string()),
        labels: meta.labels.clone().unwrap_or_default(),
    }
}

pub fn tool_definitions() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "name": "list_configmaps",
            "description": "List configmaps in a namespace. Returns name, namespace, data key count, and created_at.",
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
            "name": "get_configmap",
            "description": "Get a configmap by name. Returns name, namespace, labels, annotations, and data keys with values.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "ConfigMap name" }
                },
                "required": ["namespace", "name"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "create_configmap",
            "description": "Create a configmap in a namespace with the given data key-value pairs.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "ConfigMap name" },
                    "data": {
                        "type": "object",
                        "description": "Key-value string pairs for the configmap data",
                        "additionalProperties": { "type": "string" }
                    }
                },
                "required": ["namespace", "name", "data"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "update_configmap",
            "description": "Update (merge patch) a configmap's data. Provided keys are added or overwritten; existing keys not in the patch are preserved.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "ConfigMap name" },
                    "data": {
                        "type": "object",
                        "description": "Key-value string pairs to merge into the configmap data",
                        "additionalProperties": { "type": "string" }
                    }
                },
                "required": ["namespace", "name", "data"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "delete_configmap",
            "description": "Delete a configmap by name from a namespace.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "ConfigMap name" }
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
        "list_configmaps" => list_configmaps(client, args).await,
        "get_configmap" => get_configmap(client, args).await,
        "create_configmap" => create_configmap(client, args).await,
        "update_configmap" => update_configmap(client, args).await,
        "delete_configmap" => delete_configmap(client, args).await,
        _ => return None,
    };
    Some(result)
}

async fn list_configmaps(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let cm_api = api(client, ns)?;
    let label_selector = args["label_selector"].as_str();
    let field_selector = args["field_selector"].as_str();
    let mut lp = ListParams::default();
    if let Some(sel) = label_selector {
        lp = lp.labels(sel);
    }
    if let Some(sel) = field_selector {
        lp = lp.fields(sel);
    }
    let list = cm_api.list(&lp).await.map_err(|e| e.to_string())?;

    let summaries: Vec<serde_json::Value> = list
        .iter()
        .map(|cm| {
            let s = extract_summary(cm);
            serde_json::json!({
                "name": s.name,
                "namespace": s.namespace,
                "data_key_count": s.data_keys.len(),
                "created_at": s.created_at,
            })
        })
        .collect();

    serde_json::to_string_pretty(&summaries).map_err(|e| e.to_string())
}

async fn get_configmap(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;
    let cm_api = api(client, ns)?;
    let cm = cm_api.get(name).await.map_err(|e| e.to_string())?;

    let meta = &cm.metadata;
    let result = serde_json::json!({
        "name": meta.name.clone().unwrap_or_default(),
        "namespace": meta.namespace.clone().unwrap_or_default(),
        "labels": meta.labels.clone().unwrap_or_default(),
        "annotations": meta.annotations.clone().unwrap_or_default(),
        "data": cm.data.clone().unwrap_or_default(),
    });

    serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
}

async fn create_configmap(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;
    let data: BTreeMap<String, String> =
        serde_json::from_value(args.get("data").ok_or("data is required")?.clone())
            .map_err(|e| e.to_string())?;

    let mut labels = BTreeMap::new();
    labels.insert(
        "app.kubernetes.io/managed-by".to_string(),
        "mcp-k8s".to_string(),
    );

    let cm = ConfigMap {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            namespace: Some(ns.to_string()),
            labels: Some(labels),
            ..Default::default()
        },
        data: Some(data),
        ..Default::default()
    };

    let cm_api = api(client, ns)?;
    let created = cm_api
        .create(&PostParams::default(), &cm)
        .await
        .map_err(|e| e.to_string())?;

    let summary = extract_summary(&created);
    serde_json::to_string_pretty(&summary).map_err(|e| e.to_string())
}

async fn update_configmap(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;
    let data: BTreeMap<String, String> =
        serde_json::from_value(args.get("data").ok_or("data is required")?.clone())
            .map_err(|e| e.to_string())?;

    let patch = serde_json::json!({
        "data": data,
    });

    let cm_api = api(client, ns)?;
    let patched = cm_api
        .patch(name, &PatchParams::apply("mcp-k8s"), &Patch::Merge(&patch))
        .await
        .map_err(|e| e.to_string())?;

    let summary = extract_summary(&patched);
    serde_json::to_string_pretty(&summary).map_err(|e| e.to_string())
}

async fn delete_configmap(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;

    let cm_api = api(client, ns)?;
    cm_api
        .delete(name, &DeleteParams::default())
        .await
        .map_err(|e| e.to_string())?;

    let result = serde_json::json!({
        "deleted": name,
        "namespace": ns,
    });
    serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_definitions_returns_five_unique_tools() {
        let defs = tool_definitions();
        assert_eq!(defs.len(), 5);

        let names: Vec<&str> = defs.iter().map(|d| d["name"].as_str().unwrap()).collect();

        let mut unique = names.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(names.len(), unique.len(), "tool names must be unique");

        // Verify expected names are present
        assert!(names.contains(&"list_configmaps"));
        assert!(names.contains(&"get_configmap"));
        assert!(names.contains(&"create_configmap"));
        assert!(names.contains(&"update_configmap"));
        assert!(names.contains(&"delete_configmap"));
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
    fn configmap_summary_serialization() {
        let summary = ConfigMapSummary {
            name: "my-config".to_string(),
            namespace: "default".to_string(),
            data_keys: vec!["key1".to_string(), "key2".to_string()],
            created_at: Some("2024-01-01T00:00:00Z".to_string()),
            labels: BTreeMap::from([("app".to_string(), "test".to_string())]),
        };

        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["name"], "my-config");
        assert_eq!(json["namespace"], "default");
        assert_eq!(json["data_keys"].as_array().unwrap().len(), 2);
        assert_eq!(json["created_at"], "2024-01-01T00:00:00Z");
        assert_eq!(json["labels"]["app"], "test");
    }

    #[test]
    fn configmap_summary_serialization_empty_fields() {
        let summary = ConfigMapSummary {
            name: "empty".to_string(),
            namespace: "ns".to_string(),
            data_keys: vec![],
            created_at: None,
            labels: BTreeMap::new(),
        };

        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["name"], "empty");
        assert!(json["data_keys"].as_array().unwrap().is_empty());
        assert!(json["created_at"].is_null());
        assert!(json["labels"].as_object().unwrap().is_empty());
    }

    #[test]
    fn extract_summary_from_configmap() {
        let cm = ConfigMap {
            metadata: ObjectMeta {
                name: Some("test-cm".to_string()),
                namespace: Some("prod".to_string()),
                labels: Some(BTreeMap::from([(
                    "env".to_string(),
                    "production".to_string(),
                )])),
                ..Default::default()
            },
            data: Some(BTreeMap::from([
                ("config.yaml".to_string(), "key: value".to_string()),
                ("settings.json".to_string(), "{}".to_string()),
                ("README".to_string(), "hello".to_string()),
            ])),
            ..Default::default()
        };

        let summary = extract_summary(&cm);
        assert_eq!(summary.name, "test-cm");
        assert_eq!(summary.namespace, "prod");
        assert_eq!(summary.data_keys.len(), 3);
        assert!(summary.data_keys.contains(&"config.yaml".to_string()));
        assert!(summary.data_keys.contains(&"settings.json".to_string()));
        assert!(summary.data_keys.contains(&"README".to_string()));
        assert!(summary.created_at.is_none()); // no timestamp set
        assert_eq!(summary.labels.get("env").unwrap(), "production");
    }

    #[test]
    fn extract_summary_from_empty_configmap() {
        let cm = ConfigMap {
            metadata: ObjectMeta {
                name: Some("empty-cm".to_string()),
                namespace: Some("default".to_string()),
                ..Default::default()
            },
            ..Default::default()
        };

        let summary = extract_summary(&cm);
        assert_eq!(summary.name, "empty-cm");
        assert_eq!(summary.namespace, "default");
        assert!(summary.data_keys.is_empty());
        assert!(summary.created_at.is_none());
        assert!(summary.labels.is_empty());
    }
}
