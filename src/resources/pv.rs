use k8s_openapi::api::core::v1::PersistentVolume;
use kube::api::{DeleteParams, ListParams};
use serde::Serialize;

use crate::client::K8sClient;

fn api(client: &K8sClient) -> kube::Api<PersistentVolume> {
    kube::Api::all(client.inner().clone())
}

#[derive(Serialize, Debug)]
pub struct PvSummary {
    pub name: String,
    pub status: String,
    pub capacity: Option<String>,
    pub access_modes: Vec<String>,
    pub reclaim_policy: Option<String>,
    pub storage_class: Option<String>,
    pub claim_ref: Option<String>,
    pub created_at: Option<String>,
}

fn extract_summary(pv: &PersistentVolume) -> PvSummary {
    let meta = &pv.metadata;
    let spec = pv.spec.as_ref();

    let status = pv
        .status
        .as_ref()
        .and_then(|s| s.phase.clone())
        .unwrap_or_default();

    let capacity = spec
        .and_then(|s| s.capacity.as_ref())
        .and_then(|c| c.get("storage"))
        .map(|q| q.0.clone());

    let access_modes = spec
        .and_then(|s| s.access_modes.as_ref())
        .cloned()
        .unwrap_or_default();

    let reclaim_policy = spec.and_then(|s| s.persistent_volume_reclaim_policy.clone());

    let storage_class = spec.and_then(|s| s.storage_class_name.clone());

    let claim_ref = spec.and_then(|s| {
        s.claim_ref.as_ref().map(|cr| {
            format!(
                "{}/{}",
                cr.namespace.as_deref().unwrap_or(""),
                cr.name.as_deref().unwrap_or("")
            )
        })
    });

    let created_at = meta.creation_timestamp.as_ref().map(|t| t.0.to_string());

    PvSummary {
        name: meta.name.clone().unwrap_or_default(),
        status,
        capacity,
        access_modes,
        reclaim_policy,
        storage_class,
        claim_ref,
        created_at,
    }
}

pub fn tool_definitions() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "name": "list_pvs",
            "description": "List all PersistentVolumes. Returns name, status, capacity, access_modes, reclaim_policy, storage_class, claim_ref, and created_at.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "required": [],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "get_pv",
            "description": "Get a PersistentVolume by name. Returns detailed info including labels, annotations, mount_options, and volume_mode.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "PersistentVolume name" }
                },
                "required": ["name"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "delete_pv",
            "description": "Delete a PersistentVolume by name.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "PersistentVolume name" }
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
        "list_pvs" => list_pvs(client).await,
        "get_pv" => get_pv(client, args).await,
        "delete_pv" => delete_pv(client, args).await,
        _ => return None,
    };
    Some(result)
}

async fn list_pvs(client: &K8sClient) -> Result<String, String> {
    let pv_api = api(client);
    let list = pv_api
        .list(&ListParams::default())
        .await
        .map_err(|e| e.to_string())?;

    let summaries: Vec<PvSummary> = list.iter().map(|pv| extract_summary(pv)).collect();

    serde_json::to_string_pretty(&summaries).map_err(|e| e.to_string())
}

async fn get_pv(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let name = args["name"].as_str().ok_or("name is required")?;
    let pv_api = api(client);
    let pv = pv_api.get(name).await.map_err(|e| e.to_string())?;

    let summary = extract_summary(&pv);
    let meta = &pv.metadata;
    let spec = pv.spec.as_ref();

    let mount_options = spec
        .and_then(|s| s.mount_options.as_ref())
        .cloned()
        .unwrap_or_default();

    let volume_mode = spec.and_then(|s| s.volume_mode.clone());

    let result = serde_json::json!({
        "name": summary.name,
        "status": summary.status,
        "capacity": summary.capacity,
        "access_modes": summary.access_modes,
        "reclaim_policy": summary.reclaim_policy,
        "storage_class": summary.storage_class,
        "claim_ref": summary.claim_ref,
        "created_at": summary.created_at,
        "labels": meta.labels.clone().unwrap_or_default(),
        "annotations": meta.annotations.clone().unwrap_or_default(),
        "mount_options": mount_options,
        "volume_mode": volume_mode,
    });

    serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
}

async fn delete_pv(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let name = args["name"].as_str().ok_or("name is required")?;

    let pv_api = api(client);
    pv_api
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
    use k8s_openapi::api::core::v1::ObjectReference;
    use k8s_openapi::api::core::v1::{PersistentVolumeSpec, PersistentVolumeStatus};
    use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
    use std::collections::BTreeMap;

    #[test]
    fn tool_definitions_returns_three_tools() {
        let defs = tool_definitions();
        assert_eq!(defs.len(), 3);

        let names: Vec<&str> = defs.iter().map(|d| d["name"].as_str().unwrap()).collect();

        let mut unique = names.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(names.len(), unique.len(), "tool names must be unique");

        assert!(names.contains(&"list_pvs"));
        assert!(names.contains(&"get_pv"));
        assert!(names.contains(&"delete_pv"));
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
    fn pv_summary_serialization() {
        let summary = PvSummary {
            name: "pv-data-01".to_string(),
            status: "Bound".to_string(),
            capacity: Some("100Gi".to_string()),
            access_modes: vec!["ReadWriteOnce".to_string()],
            reclaim_policy: Some("Retain".to_string()),
            storage_class: Some("gp3".to_string()),
            claim_ref: Some("default/my-claim".to_string()),
            created_at: Some("2024-01-01T00:00:00Z".to_string()),
        };

        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["name"], "pv-data-01");
        assert_eq!(json["status"], "Bound");
        assert_eq!(json["capacity"], "100Gi");
        assert_eq!(json["access_modes"].as_array().unwrap().len(), 1);
        assert_eq!(json["access_modes"][0], "ReadWriteOnce");
        assert_eq!(json["reclaim_policy"], "Retain");
        assert_eq!(json["storage_class"], "gp3");
        assert_eq!(json["claim_ref"], "default/my-claim");
        assert_eq!(json["created_at"], "2024-01-01T00:00:00Z");
    }

    #[test]
    fn pv_summary_serialization_empty_fields() {
        let summary = PvSummary {
            name: "pv-empty".to_string(),
            status: "Available".to_string(),
            capacity: None,
            access_modes: vec![],
            reclaim_policy: None,
            storage_class: None,
            claim_ref: None,
            created_at: None,
        };

        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["name"], "pv-empty");
        assert_eq!(json["status"], "Available");
        assert!(json["capacity"].is_null());
        assert!(json["access_modes"].as_array().unwrap().is_empty());
        assert!(json["reclaim_policy"].is_null());
        assert!(json["storage_class"].is_null());
        assert!(json["claim_ref"].is_null());
        assert!(json["created_at"].is_null());
    }

    #[test]
    fn extract_summary_from_pv() {
        let pv = PersistentVolume {
            metadata: ObjectMeta {
                name: Some("pv-test-01".to_string()),
                labels: Some(BTreeMap::from([(
                    "tier".to_string(),
                    "storage".to_string(),
                )])),
                ..Default::default()
            },
            spec: Some(PersistentVolumeSpec {
                capacity: Some(BTreeMap::from([(
                    "storage".to_string(),
                    Quantity("50Gi".to_string()),
                )])),
                access_modes: Some(vec![
                    "ReadWriteOnce".to_string(),
                    "ReadOnlyMany".to_string(),
                ]),
                persistent_volume_reclaim_policy: Some("Delete".to_string()),
                storage_class_name: Some("standard".to_string()),
                claim_ref: Some(ObjectReference {
                    namespace: Some("prod".to_string()),
                    name: Some("data-claim".to_string()),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            status: Some(PersistentVolumeStatus {
                phase: Some("Bound".to_string()),
                ..Default::default()
            }),
        };

        let summary = extract_summary(&pv);
        assert_eq!(summary.name, "pv-test-01");
        assert_eq!(summary.status, "Bound");
        assert_eq!(summary.capacity.as_deref(), Some("50Gi"));
        assert_eq!(summary.access_modes.len(), 2);
        assert!(summary.access_modes.contains(&"ReadWriteOnce".to_string()));
        assert!(summary.access_modes.contains(&"ReadOnlyMany".to_string()));
        assert_eq!(summary.reclaim_policy.as_deref(), Some("Delete"));
        assert_eq!(summary.storage_class.as_deref(), Some("standard"));
        assert_eq!(summary.claim_ref.as_deref(), Some("prod/data-claim"));
        assert!(summary.created_at.is_none());
    }

    #[test]
    fn extract_summary_from_empty_pv() {
        let pv = PersistentVolume {
            metadata: ObjectMeta {
                name: Some("pv-minimal".to_string()),
                ..Default::default()
            },
            spec: None,
            status: None,
        };

        let summary = extract_summary(&pv);
        assert_eq!(summary.name, "pv-minimal");
        assert_eq!(summary.status, "");
        assert!(summary.capacity.is_none());
        assert!(summary.access_modes.is_empty());
        assert!(summary.reclaim_policy.is_none());
        assert!(summary.storage_class.is_none());
        assert!(summary.claim_ref.is_none());
        assert!(summary.created_at.is_none());
    }
}
