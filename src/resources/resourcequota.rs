use std::collections::BTreeMap;

use k8s_openapi::api::core::v1::{ResourceQuota, ResourceQuotaSpec};
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::api::{DeleteParams, ListParams, Patch, PatchParams, PostParams};
use serde::Serialize;

use crate::client::K8sClient;

fn api(client: &K8sClient, ns: &str) -> Result<kube::Api<ResourceQuota>, String> {
    if !client.is_namespace_allowed(ns) {
        return Err(format!("Namespace '{ns}' is not in the allowed list"));
    }
    Ok(kube::Api::namespaced(client.inner().clone(), ns))
}

// ---------------------------------------------------------------------------
// Summary types
// ---------------------------------------------------------------------------

#[derive(Serialize, Debug)]
pub struct ResourceQuotaSummary {
    pub name: String,
    pub namespace: String,
    pub hard: BTreeMap<String, String>,
    pub used: BTreeMap<String, String>,
    pub created_at: Option<String>,
}

// ---------------------------------------------------------------------------
// Extraction helpers
// ---------------------------------------------------------------------------

fn quantity_map_to_strings(m: &Option<BTreeMap<String, Quantity>>) -> BTreeMap<String, String> {
    m.as_ref()
        .map(|map| map.iter().map(|(k, v)| (k.clone(), v.0.clone())).collect())
        .unwrap_or_default()
}

fn extract_summary(rq: &ResourceQuota) -> ResourceQuotaSummary {
    let meta = &rq.metadata;

    let hard = rq
        .status
        .as_ref()
        .and_then(|s| s.hard.as_ref())
        .or_else(|| rq.spec.as_ref().and_then(|s| s.hard.as_ref()))
        .cloned();

    let used = rq.status.as_ref().and_then(|s| s.used.clone());

    ResourceQuotaSummary {
        name: meta.name.clone().unwrap_or_default(),
        namespace: meta.namespace.clone().unwrap_or_default(),
        hard: quantity_map_to_strings(&hard),
        used: quantity_map_to_strings(&used),
        created_at: meta.creation_timestamp.as_ref().map(|t| t.0.to_string()),
    }
}

// ---------------------------------------------------------------------------
// Tool definitions
// ---------------------------------------------------------------------------

pub fn tool_definitions() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "name": "list_resourcequotas",
            "description": "List ResourceQuotas in a namespace. Returns name, namespace, hard limits, used amounts, and created_at.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "label_selector": { "type": "string", "description": "Label selector to filter results (e.g. app=nginx)" }
                },
                "required": ["namespace"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "get_resourcequota",
            "description": "Get a ResourceQuota by name. Returns name, namespace, hard limits, used amounts, created_at, labels, annotations, scopes, and scope_selector.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "ResourceQuota name" }
                },
                "required": ["namespace", "name"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "create_resourcequota",
            "description": "Create a ResourceQuota in a namespace with the given hard resource limits.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "ResourceQuota name" },
                    "hard": {
                        "type": "object",
                        "description": "Hard resource limits as key-value pairs (e.g. {\"cpu\": \"10\", \"memory\": \"20Gi\", \"pods\": \"50\"})",
                        "additionalProperties": { "type": "string" }
                    }
                },
                "required": ["namespace", "name", "hard"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "update_resourcequota",
            "description": "Update (merge patch) a ResourceQuota's hard limits. Provided keys are added or overwritten; existing keys not in the patch are preserved.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "ResourceQuota name" },
                    "hard": {
                        "type": "object",
                        "description": "Hard resource limits to merge into the ResourceQuota spec",
                        "additionalProperties": { "type": "string" }
                    }
                },
                "required": ["namespace", "name", "hard"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "delete_resourcequota",
            "description": "Delete a ResourceQuota by name from a namespace.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "ResourceQuota name" }
                },
                "required": ["namespace", "name"],
                "additionalProperties": false
            }
        }),
    ]
}

// ---------------------------------------------------------------------------
// Tool handler
// ---------------------------------------------------------------------------

pub async fn handle_tool(
    client: &K8sClient,
    name: &str,
    args: &serde_json::Value,
) -> Option<Result<String, String>> {
    let result = match name {
        "list_resourcequotas" => list_resourcequotas(client, args).await,
        "get_resourcequota" => get_resourcequota(client, args).await,
        "create_resourcequota" => create_resourcequota(client, args).await,
        "update_resourcequota" => update_resourcequota(client, args).await,
        "delete_resourcequota" => delete_resourcequota(client, args).await,
        _ => return None,
    };
    Some(result)
}

// ---------------------------------------------------------------------------
// Tool implementations
// ---------------------------------------------------------------------------

async fn list_resourcequotas(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let rq_api = api(client, ns)?;
    let label_selector = args["label_selector"].as_str();
    let mut lp = ListParams::default();
    if let Some(sel) = label_selector {
        lp = lp.labels(sel);
    }
    let list = rq_api.list(&lp).await.map_err(|e| e.to_string())?;

    let summaries: Vec<serde_json::Value> = list
        .iter()
        .map(|rq| {
            let s = extract_summary(rq);
            serde_json::to_value(s).unwrap_or_default()
        })
        .collect();

    serde_json::to_string_pretty(&summaries).map_err(|e| e.to_string())
}

async fn get_resourcequota(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;

    let rq_api = api(client, ns)?;
    let rq = rq_api.get(name).await.map_err(|e| e.to_string())?;

    let summary = extract_summary(&rq);
    let meta = &rq.metadata;
    let spec = rq.spec.as_ref();

    let scopes: Vec<String> = spec
        .and_then(|s| s.scopes.as_ref())
        .cloned()
        .unwrap_or_default();

    let scope_selector = spec
        .and_then(|s| s.scope_selector.as_ref())
        .map(|ss| serde_json::to_value(ss).unwrap_or_default())
        .unwrap_or(serde_json::Value::Null);

    let result = serde_json::json!({
        "name": summary.name,
        "namespace": summary.namespace,
        "hard": summary.hard,
        "used": summary.used,
        "created_at": summary.created_at,
        "labels": meta.labels.clone().unwrap_or_default(),
        "annotations": meta.annotations.clone().unwrap_or_default(),
        "scopes": scopes,
        "scope_selector": scope_selector,
    });

    serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
}

async fn create_resourcequota(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;
    let hard_raw: BTreeMap<String, String> =
        serde_json::from_value(args.get("hard").ok_or("hard is required")?.clone())
            .map_err(|e| e.to_string())?;

    let hard: BTreeMap<String, Quantity> = hard_raw
        .into_iter()
        .map(|(k, v)| (k, Quantity(v)))
        .collect();

    let mut labels = BTreeMap::new();
    labels.insert(
        "app.kubernetes.io/managed-by".to_string(),
        "mcp-k8s".to_string(),
    );

    let rq = ResourceQuota {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            namespace: Some(ns.to_string()),
            labels: Some(labels),
            ..Default::default()
        },
        spec: Some(ResourceQuotaSpec {
            hard: Some(hard),
            ..Default::default()
        }),
        ..Default::default()
    };

    let rq_api = api(client, ns)?;
    let created = rq_api
        .create(&PostParams::default(), &rq)
        .await
        .map_err(|e| e.to_string())?;

    let summary = extract_summary(&created);
    serde_json::to_string_pretty(&serde_json::to_value(summary).unwrap_or_default())
        .map_err(|e| e.to_string())
}

async fn update_resourcequota(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;
    let hard_raw: BTreeMap<String, String> =
        serde_json::from_value(args.get("hard").ok_or("hard is required")?.clone())
            .map_err(|e| e.to_string())?;

    let patch = serde_json::json!({
        "spec": {
            "hard": hard_raw,
        },
    });

    let rq_api = api(client, ns)?;
    let patched = rq_api
        .patch(name, &PatchParams::apply("mcp-k8s"), &Patch::Merge(&patch))
        .await
        .map_err(|e| e.to_string())?;

    let summary = extract_summary(&patched);
    serde_json::to_string_pretty(&serde_json::to_value(summary).unwrap_or_default())
        .map_err(|e| e.to_string())
}

async fn delete_resourcequota(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;

    let rq_api = api(client, ns)?;
    rq_api
        .delete(name, &DeleteParams::default())
        .await
        .map_err(|e| e.to_string())?;

    let result = serde_json::json!({
        "deleted": name,
        "namespace": ns,
    });
    serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::api::core::v1::ResourceQuotaStatus;

    #[test]
    fn tool_definitions_returns_five_tools() {
        let defs = tool_definitions();
        assert_eq!(defs.len(), 5);

        let names: Vec<&str> = defs.iter().filter_map(|d| d["name"].as_str()).collect();

        let mut unique = names.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(names.len(), unique.len(), "tool names must be unique");

        assert!(names.contains(&"list_resourcequotas"));
        assert!(names.contains(&"get_resourcequota"));
        assert!(names.contains(&"create_resourcequota"));
        assert!(names.contains(&"update_resourcequota"));
        assert!(names.contains(&"delete_resourcequota"));
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
    fn resourcequota_summary_serialization() {
        let summary = ResourceQuotaSummary {
            name: "my-quota".to_string(),
            namespace: "default".to_string(),
            hard: BTreeMap::from([
                ("cpu".to_string(), "10".to_string()),
                ("memory".to_string(), "20Gi".to_string()),
                ("pods".to_string(), "50".to_string()),
            ]),
            used: BTreeMap::from([
                ("cpu".to_string(), "3".to_string()),
                ("memory".to_string(), "8Gi".to_string()),
                ("pods".to_string(), "12".to_string()),
            ]),
            created_at: Some("2024-01-01T00:00:00Z".to_string()),
        };

        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["name"], "my-quota");
        assert_eq!(json["namespace"], "default");
        assert_eq!(json["hard"]["cpu"], "10");
        assert_eq!(json["hard"]["memory"], "20Gi");
        assert_eq!(json["hard"]["pods"], "50");
        assert_eq!(json["used"]["cpu"], "3");
        assert_eq!(json["used"]["memory"], "8Gi");
        assert_eq!(json["used"]["pods"], "12");
        assert_eq!(json["created_at"], "2024-01-01T00:00:00Z");
    }

    #[test]
    fn resourcequota_summary_serialization_empty_fields() {
        let summary = ResourceQuotaSummary {
            name: "empty-quota".to_string(),
            namespace: "ns".to_string(),
            hard: BTreeMap::new(),
            used: BTreeMap::new(),
            created_at: None,
        };

        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["name"], "empty-quota");
        assert_eq!(json["namespace"], "ns");
        assert!(json["hard"].as_object().unwrap().is_empty());
        assert!(json["used"].as_object().unwrap().is_empty());
        assert!(json["created_at"].is_null());
    }

    #[test]
    fn extract_summary_from_resourcequota_with_status() {
        let mut hard = BTreeMap::new();
        hard.insert("cpu".to_string(), Quantity("10".to_string()));
        hard.insert("memory".to_string(), Quantity("20Gi".to_string()));
        hard.insert("pods".to_string(), Quantity("50".to_string()));

        let mut used = BTreeMap::new();
        used.insert("cpu".to_string(), Quantity("5".to_string()));
        used.insert("memory".to_string(), Quantity("10Gi".to_string()));
        used.insert("pods".to_string(), Quantity("25".to_string()));

        let rq = ResourceQuota {
            metadata: ObjectMeta {
                name: Some("prod-quota".to_string()),
                namespace: Some("production".to_string()),
                labels: Some(BTreeMap::from([("env".to_string(), "prod".to_string())])),
                ..Default::default()
            },
            spec: Some(ResourceQuotaSpec {
                hard: Some(hard.clone()),
                ..Default::default()
            }),
            status: Some(ResourceQuotaStatus {
                hard: Some(hard),
                used: Some(used),
            }),
        };

        let summary = extract_summary(&rq);
        assert_eq!(summary.name, "prod-quota");
        assert_eq!(summary.namespace, "production");
        assert_eq!(summary.hard.get("cpu").unwrap(), "10");
        assert_eq!(summary.hard.get("memory").unwrap(), "20Gi");
        assert_eq!(summary.hard.get("pods").unwrap(), "50");
        assert_eq!(summary.used.get("cpu").unwrap(), "5");
        assert_eq!(summary.used.get("memory").unwrap(), "10Gi");
        assert_eq!(summary.used.get("pods").unwrap(), "25");
        assert!(summary.created_at.is_none());
    }

    #[test]
    fn extract_summary_from_resourcequota_spec_only() {
        let mut hard = BTreeMap::new();
        hard.insert("pods".to_string(), Quantity("100".to_string()));

        let rq = ResourceQuota {
            metadata: ObjectMeta {
                name: Some("spec-only".to_string()),
                namespace: Some("default".to_string()),
                ..Default::default()
            },
            spec: Some(ResourceQuotaSpec {
                hard: Some(hard),
                ..Default::default()
            }),
            status: None,
        };

        let summary = extract_summary(&rq);
        assert_eq!(summary.name, "spec-only");
        assert_eq!(summary.namespace, "default");
        // Falls back to spec.hard when status.hard is absent
        assert_eq!(summary.hard.get("pods").unwrap(), "100");
        assert!(summary.used.is_empty());
    }

    #[test]
    fn extract_summary_from_empty_resourcequota() {
        let rq = ResourceQuota {
            metadata: ObjectMeta {
                name: Some("empty-rq".to_string()),
                namespace: Some("default".to_string()),
                ..Default::default()
            },
            ..Default::default()
        };

        let summary = extract_summary(&rq);
        assert_eq!(summary.name, "empty-rq");
        assert_eq!(summary.namespace, "default");
        assert!(summary.hard.is_empty());
        assert!(summary.used.is_empty());
        assert!(summary.created_at.is_none());
    }
}
