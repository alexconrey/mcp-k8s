use k8s_openapi::api::apiserverinternal::v1alpha1::StorageVersion;
use kube::api::ListParams;
use serde::Serialize;

use crate::client::K8sClient;

fn api(client: &K8sClient) -> kube::Api<StorageVersion> {
    kube::Api::all(client.inner().clone())
}

#[derive(Serialize, Debug)]
pub struct StorageVersionSummary {
    pub name: String,
    pub created_at: Option<String>,
}

fn extract_summary(sv: &StorageVersion) -> StorageVersionSummary {
    let meta = &sv.metadata;

    StorageVersionSummary {
        name: meta.name.clone().unwrap_or_default(),
        created_at: meta.creation_timestamp.as_ref().map(|t| t.0.to_string()),
    }
}

pub fn tool_definitions() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "name": "list_storageversions",
            "description": "List all StorageVersions in the cluster. Returns name and created_at.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "required": [],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "get_storageversion",
            "description": "Get a StorageVersion by name. Returns name, spec, status (storage_versions with api_server_id and encoding_version, common_encoding_version, conditions), labels, and annotations.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "StorageVersion name" }
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
        "list_storageversions" => list_storageversions(client).await,
        "get_storageversion" => get_storageversion(client, args).await,
        _ => return None,
    };
    Some(result)
}

async fn list_storageversions(client: &K8sClient) -> Result<String, String> {
    let sv_api = api(client);
    let list = sv_api
        .list(&ListParams::default())
        .await
        .map_err(|e| e.to_string())?;

    let summaries: Vec<StorageVersionSummary> = list.iter().map(extract_summary).collect();

    serde_json::to_string_pretty(&summaries).map_err(|e| e.to_string())
}

async fn get_storageversion(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let name = args["name"].as_str().ok_or("name is required")?;

    let sv_api = api(client);
    let sv = sv_api.get(name).await.map_err(|e| e.to_string())?;

    let summary = extract_summary(&sv);
    let meta = &sv.metadata;
    let status = &sv.status;

    let storage_versions = status
        .storage_versions
        .as_ref()
        .map(|svs| {
            svs.iter()
                .map(|s| {
                    serde_json::json!({
                        "api_server_id": s.api_server_id,
                        "encoding_version": s.encoding_version,
                        "decodable_versions": s.decodable_versions.clone().unwrap_or_default(),
                        "served_versions": s.served_versions.clone().unwrap_or_default(),
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let conditions = status
        .conditions
        .as_ref()
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
                        "observed_generation": c.observed_generation,
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let result = serde_json::json!({
        "name": summary.name,
        "created_at": summary.created_at,
        "spec": sv.spec.0,
        "status": {
            "storage_versions": storage_versions,
            "common_encoding_version": status.common_encoding_version,
            "conditions": conditions,
        },
        "labels": meta.labels.clone().unwrap_or_default(),
        "annotations": meta.annotations.clone().unwrap_or_default(),
    });

    serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_definitions_returns_two_tools() {
        let defs = tool_definitions();
        assert_eq!(defs.len(), 2);

        let names: Vec<&str> = defs.iter().map(|d| d["name"].as_str().unwrap()).collect();

        let mut unique = names.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(names.len(), unique.len(), "tool names must be unique");

        assert!(names.contains(&"list_storageversions"));
        assert!(names.contains(&"get_storageversion"));
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
    fn storageversion_summary_serialization() {
        let summary = StorageVersionSummary {
            name: "apps.deployments".to_string(),
            created_at: Some("2024-01-01T00:00:00Z".to_string()),
        };

        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["name"], "apps.deployments");
        assert_eq!(json["created_at"], "2024-01-01T00:00:00Z");
    }

    #[test]
    fn storageversion_summary_serialization_minimal() {
        let summary = StorageVersionSummary {
            name: "core.pods".to_string(),
            created_at: None,
        };

        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["name"], "core.pods");
        assert!(json["created_at"].is_null());
    }

    #[test]
    fn extract_summary_from_storageversion() {
        use k8s_openapi::api::apiserverinternal::v1alpha1::{
            StorageVersionSpec, StorageVersionStatus,
        };
        use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

        let sv = StorageVersion {
            metadata: ObjectMeta {
                name: Some("apps.deployments".to_string()),
                ..Default::default()
            },
            spec: StorageVersionSpec(serde_json::json!({})),
            status: StorageVersionStatus::default(),
        };

        let summary = extract_summary(&sv);
        assert_eq!(summary.name, "apps.deployments");
        assert!(summary.created_at.is_none());
    }

    #[test]
    fn extract_summary_from_minimal_storageversion() {
        use k8s_openapi::api::apiserverinternal::v1alpha1::{
            StorageVersionSpec, StorageVersionStatus,
        };
        use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

        let sv = StorageVersion {
            metadata: ObjectMeta {
                name: Some("batch.jobs".to_string()),
                ..Default::default()
            },
            spec: StorageVersionSpec(serde_json::json!({})),
            status: StorageVersionStatus {
                common_encoding_version: None,
                conditions: None,
                storage_versions: None,
            },
        };

        let summary = extract_summary(&sv);
        assert_eq!(summary.name, "batch.jobs");
        assert!(summary.created_at.is_none());
    }
}
