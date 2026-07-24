use std::collections::BTreeMap;

use k8s_openapi::api::core::v1::{
    PersistentVolumeClaim, PersistentVolumeClaimSpec, VolumeResourceRequirements,
};
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::api::{DeleteParams, ListParams, Patch, PatchParams, PostParams};
use serde::Serialize;

use crate::client::K8sClient;

fn api(client: &K8sClient, ns: &str) -> Result<kube::Api<PersistentVolumeClaim>, String> {
    if !client.is_namespace_allowed(ns) {
        return Err(format!("Namespace '{ns}' is not in the allowed list"));
    }
    Ok(kube::Api::namespaced(client.inner().clone(), ns))
}

#[derive(Serialize, Debug)]
pub struct PvcSummary {
    pub name: String,
    pub namespace: String,
    pub status: String,
    pub capacity: Option<String>,
    pub access_modes: Vec<String>,
    pub storage_class: Option<String>,
    pub volume_name: Option<String>,
    pub created_at: Option<String>,
}

fn extract_summary(pvc: &PersistentVolumeClaim) -> PvcSummary {
    let meta = &pvc.metadata;

    let status_phase = pvc
        .status
        .as_ref()
        .and_then(|s| s.phase.clone())
        .unwrap_or_else(|| "Unknown".to_string());

    let capacity = pvc
        .status
        .as_ref()
        .and_then(|s| s.capacity.as_ref())
        .and_then(|c| c.get("storage"))
        .map(|q| q.0.clone());

    let access_modes = pvc
        .status
        .as_ref()
        .and_then(|s| s.access_modes.clone())
        .or_else(|| pvc.spec.as_ref().and_then(|s| s.access_modes.clone()))
        .unwrap_or_default();

    let storage_class = pvc.spec.as_ref().and_then(|s| s.storage_class_name.clone());

    let volume_name = pvc.spec.as_ref().and_then(|s| s.volume_name.clone());

    PvcSummary {
        name: meta.name.clone().unwrap_or_default(),
        namespace: meta.namespace.clone().unwrap_or_default(),
        status: status_phase,
        capacity,
        access_modes,
        storage_class,
        volume_name,
        created_at: meta.creation_timestamp.as_ref().map(|t| t.0.to_string()),
    }
}

pub fn tool_definitions() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "name": "list_pvcs",
            "description": "List PersistentVolumeClaims in a namespace. Returns name, namespace, status, capacity, access_modes, storage_class, volume_name, and created_at.",
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
            "name": "get_pvc",
            "description": "Get a PersistentVolumeClaim by name. Returns name, namespace, status, capacity, access_modes, storage_class, volume_name, created_at, labels, annotations, and conditions.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "PVC name" }
                },
                "required": ["namespace", "name"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "create_pvc",
            "description": "Create a PersistentVolumeClaim in a namespace.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "PVC name" },
                    "storage_class": { "type": "string", "description": "Storage class name (optional)" },
                    "access_modes": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Access modes (default: [\"ReadWriteOnce\"])"
                    },
                    "storage": { "type": "string", "description": "Storage size (e.g. \"10Gi\")" }
                },
                "required": ["namespace", "name", "storage"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "update_pvc",
            "description": "Update a PersistentVolumeClaim. Supports resizing storage (requires storage class with allowVolumeExpansion).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "PVC name" },
                    "storage": { "type": "string", "description": "New storage size (e.g. \"20Gi\")" }
                },
                "required": ["namespace", "name", "storage"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "delete_pvc",
            "description": "Delete a PersistentVolumeClaim by name from a namespace.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "PVC name" }
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
        "list_pvcs" => list_pvcs(client, args).await,
        "get_pvc" => get_pvc(client, args).await,
        "create_pvc" => create_pvc(client, args).await,
        "update_pvc" => update_pvc(client, args).await,
        "delete_pvc" => delete_pvc(client, args).await,
        _ => return None,
    };
    Some(result)
}

async fn list_pvcs(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let pvc_api = api(client, ns)?;
    let label_selector = args["label_selector"].as_str();
    let mut lp = ListParams::default();
    if let Some(sel) = label_selector {
        lp = lp.labels(sel);
    }
    let list = pvc_api.list(&lp).await.map_err(|e| e.to_string())?;

    let summaries: Vec<PvcSummary> = list.iter().map(extract_summary).collect();

    serde_json::to_string_pretty(&summaries).map_err(|e| e.to_string())
}

async fn get_pvc(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;
    let pvc_api = api(client, ns)?;
    let pvc = pvc_api.get(name).await.map_err(|e| e.to_string())?;

    let summary = extract_summary(&pvc);
    let meta = &pvc.metadata;

    let conditions = pvc
        .status
        .as_ref()
        .and_then(|s| s.conditions.as_ref())
        .map(|conds| {
            conds
                .iter()
                .map(|c| {
                    serde_json::json!({
                        "type": c.type_,
                        "status": c.status,
                        "reason": c.reason,
                        "message": c.message,
                        "last_transition_time": c.last_transition_time.as_ref().map(|t| t.0.to_string()),
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let result = serde_json::json!({
        "name": summary.name,
        "namespace": summary.namespace,
        "status": summary.status,
        "capacity": summary.capacity,
        "access_modes": summary.access_modes,
        "storage_class": summary.storage_class,
        "volume_name": summary.volume_name,
        "created_at": summary.created_at,
        "labels": meta.labels.clone().unwrap_or_default(),
        "annotations": meta.annotations.clone().unwrap_or_default(),
        "conditions": conditions,
    });

    serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
}

async fn create_pvc(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;
    let storage = args["storage"].as_str().ok_or("storage is required")?;

    let storage_class = args
        .get("storage_class")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let access_modes: Vec<String> = args
        .get("access_modes")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_else(|| vec!["ReadWriteOnce".to_string()]);

    let mut labels = BTreeMap::new();
    labels.insert(
        "app.kubernetes.io/managed-by".to_string(),
        "mcp-k8s".to_string(),
    );

    let mut requests = BTreeMap::new();
    requests.insert("storage".to_string(), Quantity(storage.to_string()));

    let pvc = PersistentVolumeClaim {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            namespace: Some(ns.to_string()),
            labels: Some(labels),
            ..Default::default()
        },
        spec: Some(PersistentVolumeClaimSpec {
            access_modes: Some(access_modes),
            storage_class_name: storage_class,
            resources: Some(VolumeResourceRequirements {
                requests: Some(requests),
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    };

    let pvc_api = api(client, ns)?;
    let created = pvc_api
        .create(&PostParams::default(), &pvc)
        .await
        .map_err(|e| e.to_string())?;

    let summary = extract_summary(&created);
    serde_json::to_string_pretty(&summary).map_err(|e| e.to_string())
}

async fn update_pvc(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;
    let storage = args["storage"].as_str().ok_or("storage is required")?;

    let patch = serde_json::json!({
        "spec": {
            "resources": {
                "requests": {
                    "storage": storage
                }
            }
        }
    });

    let pvc_api = api(client, ns)?;
    let patched = pvc_api
        .patch(name, &PatchParams::apply("mcp-k8s"), &Patch::Merge(&patch))
        .await
        .map_err(|e| e.to_string())?;

    let summary = extract_summary(&patched);
    serde_json::to_string_pretty(&summary).map_err(|e| e.to_string())
}

async fn delete_pvc(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;

    let pvc_api = api(client, ns)?;
    pvc_api
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
    use k8s_openapi::api::core::v1::{PersistentVolumeClaimCondition, PersistentVolumeClaimStatus};

    #[test]
    fn tool_definitions_returns_five_tools() {
        let defs = tool_definitions();
        assert_eq!(defs.len(), 5);

        let names: Vec<&str> = defs.iter().map(|d| d["name"].as_str().unwrap()).collect();

        let mut unique = names.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(names.len(), unique.len(), "tool names must be unique");

        assert!(names.contains(&"list_pvcs"));
        assert!(names.contains(&"get_pvc"));
        assert!(names.contains(&"create_pvc"));
        assert!(names.contains(&"delete_pvc"));
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
    fn pvc_summary_serialization() {
        let summary = PvcSummary {
            name: "my-pvc".to_string(),
            namespace: "default".to_string(),
            status: "Bound".to_string(),
            capacity: Some("10Gi".to_string()),
            access_modes: vec!["ReadWriteOnce".to_string()],
            storage_class: Some("standard".to_string()),
            volume_name: Some("pv-abc123".to_string()),
            created_at: Some("2024-01-01T00:00:00Z".to_string()),
        };

        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["name"], "my-pvc");
        assert_eq!(json["namespace"], "default");
        assert_eq!(json["status"], "Bound");
        assert_eq!(json["capacity"], "10Gi");
        assert_eq!(json["access_modes"].as_array().unwrap().len(), 1);
        assert_eq!(json["access_modes"][0], "ReadWriteOnce");
        assert_eq!(json["storage_class"], "standard");
        assert_eq!(json["volume_name"], "pv-abc123");
        assert_eq!(json["created_at"], "2024-01-01T00:00:00Z");
    }

    #[test]
    fn pvc_summary_serialization_empty_fields() {
        let summary = PvcSummary {
            name: "empty-pvc".to_string(),
            namespace: "ns".to_string(),
            status: "Pending".to_string(),
            capacity: None,
            access_modes: vec![],
            storage_class: None,
            volume_name: None,
            created_at: None,
        };

        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["name"], "empty-pvc");
        assert_eq!(json["status"], "Pending");
        assert!(json["capacity"].is_null());
        assert!(json["access_modes"].as_array().unwrap().is_empty());
        assert!(json["storage_class"].is_null());
        assert!(json["volume_name"].is_null());
        assert!(json["created_at"].is_null());
    }

    #[test]
    fn extract_summary_from_bound_pvc() {
        let mut capacity = BTreeMap::new();
        capacity.insert("storage".to_string(), Quantity("20Gi".to_string()));

        let pvc = PersistentVolumeClaim {
            metadata: ObjectMeta {
                name: Some("data-pvc".to_string()),
                namespace: Some("prod".to_string()),
                labels: Some(BTreeMap::from([(
                    "app".to_string(),
                    "database".to_string(),
                )])),
                ..Default::default()
            },
            spec: Some(PersistentVolumeClaimSpec {
                access_modes: Some(vec![
                    "ReadWriteOnce".to_string(),
                    "ReadOnlyMany".to_string(),
                ]),
                storage_class_name: Some("fast-ssd".to_string()),
                volume_name: Some("pv-data-001".to_string()),
                ..Default::default()
            }),
            status: Some(PersistentVolumeClaimStatus {
                phase: Some("Bound".to_string()),
                capacity: Some(capacity),
                access_modes: Some(vec!["ReadWriteOnce".to_string()]),
                conditions: Some(vec![PersistentVolumeClaimCondition {
                    type_: "Resizing".to_string(),
                    status: "True".to_string(),
                    reason: Some("ExpandingVolume".to_string()),
                    message: Some("Expanding volume".to_string()),
                    last_transition_time: None,
                    ..Default::default()
                }]),
                ..Default::default()
            }),
        };

        let summary = extract_summary(&pvc);
        assert_eq!(summary.name, "data-pvc");
        assert_eq!(summary.namespace, "prod");
        assert_eq!(summary.status, "Bound");
        assert_eq!(summary.capacity, Some("20Gi".to_string()));
        // Status access_modes take precedence
        assert_eq!(summary.access_modes, vec!["ReadWriteOnce".to_string()]);
        assert_eq!(summary.storage_class, Some("fast-ssd".to_string()));
        assert_eq!(summary.volume_name, Some("pv-data-001".to_string()));
        assert!(summary.created_at.is_none());
    }

    #[test]
    fn extract_summary_from_pending_pvc() {
        let pvc = PersistentVolumeClaim {
            metadata: ObjectMeta {
                name: Some("pending-pvc".to_string()),
                namespace: Some("default".to_string()),
                ..Default::default()
            },
            spec: Some(PersistentVolumeClaimSpec {
                access_modes: Some(vec!["ReadWriteMany".to_string()]),
                storage_class_name: Some("nfs".to_string()),
                ..Default::default()
            }),
            status: Some(PersistentVolumeClaimStatus {
                phase: Some("Pending".to_string()),
                ..Default::default()
            }),
        };

        let summary = extract_summary(&pvc);
        assert_eq!(summary.name, "pending-pvc");
        assert_eq!(summary.namespace, "default");
        assert_eq!(summary.status, "Pending");
        assert!(summary.capacity.is_none());
        // Falls back to spec access_modes when status has none
        assert_eq!(summary.access_modes, vec!["ReadWriteMany".to_string()]);
        assert_eq!(summary.storage_class, Some("nfs".to_string()));
        assert!(summary.volume_name.is_none());
    }

    #[test]
    fn extract_summary_from_empty_pvc() {
        let pvc = PersistentVolumeClaim {
            metadata: ObjectMeta {
                name: Some("empty-pvc".to_string()),
                namespace: Some("default".to_string()),
                ..Default::default()
            },
            ..Default::default()
        };

        let summary = extract_summary(&pvc);
        assert_eq!(summary.name, "empty-pvc");
        assert_eq!(summary.namespace, "default");
        assert_eq!(summary.status, "Unknown");
        assert!(summary.capacity.is_none());
        assert!(summary.access_modes.is_empty());
        assert!(summary.storage_class.is_none());
        assert!(summary.volume_name.is_none());
        assert!(summary.created_at.is_none());
    }
}
