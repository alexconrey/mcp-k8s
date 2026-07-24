use k8s_openapi::api::storagemigration::v1alpha1::StorageVersionMigration;
use kube::api::ListParams;
use serde::Serialize;

use crate::client::K8sClient;

fn api(client: &K8sClient) -> kube::Api<StorageVersionMigration> {
    kube::Api::all(client.inner().clone())
}

#[derive(Serialize, Debug)]
pub struct StorageVersionMigrationSummary {
    pub name: String,
    pub resource_group: Option<String>,
    pub resource_version_field: Option<String>,
    pub resource_name: Option<String>,
    pub created_at: Option<String>,
}

fn extract_summary(svm: &StorageVersionMigration) -> StorageVersionMigrationSummary {
    let meta = &svm.metadata;

    let (resource_group, resource_version_field, resource_name) = if let Some(spec) = &svm.spec {
        (
            spec.resource.group.clone(),
            spec.resource.version.clone(),
            spec.resource.resource.clone(),
        )
    } else {
        (None, None, None)
    };

    StorageVersionMigrationSummary {
        name: meta.name.clone().unwrap_or_default(),
        resource_group,
        resource_version_field,
        resource_name,
        created_at: meta.creation_timestamp.as_ref().map(|t| t.0.to_string()),
    }
}

pub fn tool_definitions() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "name": "list_storageversionmigrations",
            "description": "List all StorageVersionMigrations (storagemigration.k8s.io/v1alpha1). Returns name, resource (group/version/resource from spec), and created_at.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "required": [],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "get_storageversionmigration",
            "description": "Get a StorageVersionMigration by name. Returns name, resource (group/version/resource), created_at, status conditions, labels, and annotations.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "StorageVersionMigration name" }
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
        "list_storageversionmigrations" => list_storageversionmigrations(client).await,
        "get_storageversionmigration" => get_storageversionmigration(client, args).await,
        _ => return None,
    };
    Some(result)
}

async fn list_storageversionmigrations(client: &K8sClient) -> Result<String, String> {
    let svm_api = api(client);
    let list = svm_api
        .list(&ListParams::default())
        .await
        .map_err(|e| e.to_string())?;

    let summaries: Vec<StorageVersionMigrationSummary> =
        list.iter().map(|svm| extract_summary(svm)).collect();

    serde_json::to_string_pretty(&summaries).map_err(|e| e.to_string())
}

async fn get_storageversionmigration(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let name = args["name"].as_str().ok_or("name is required")?;
    let svm_api = api(client);
    let svm = svm_api.get(name).await.map_err(|e| e.to_string())?;

    let summary = extract_summary(&svm);
    let meta = &svm.metadata;

    let conditions: Vec<serde_json::Value> = svm
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
                        "last_update_time": c.last_update_time.as_ref().map(|t| t.0.to_string()),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    let result = serde_json::json!({
        "name": summary.name,
        "resource_group": summary.resource_group,
        "resource_version": summary.resource_version_field,
        "resource_name": summary.resource_name,
        "created_at": summary.created_at,
        "conditions": conditions,
        "labels": meta.labels.clone().unwrap_or_default(),
        "annotations": meta.annotations.clone().unwrap_or_default(),
    });

    serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::api::storagemigration::v1alpha1::{
        GroupVersionResource, StorageVersionMigrationSpec,
    };
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

        assert!(names.contains(&"list_storageversionmigrations"));
        assert!(names.contains(&"get_storageversionmigration"));
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
    fn storage_version_migration_summary_serialization() {
        let summary = StorageVersionMigrationSummary {
            name: "migrate-pods".to_string(),
            resource_group: Some("".to_string()),
            resource_version_field: Some("v1".to_string()),
            resource_name: Some("pods".to_string()),
            created_at: Some("2024-01-01T00:00:00Z".to_string()),
        };

        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["name"], "migrate-pods");
        assert_eq!(json["resource_group"], "");
        assert_eq!(json["resource_version_field"], "v1");
        assert_eq!(json["resource_name"], "pods");
        assert_eq!(json["created_at"], "2024-01-01T00:00:00Z");
    }

    #[test]
    fn storage_version_migration_summary_serialization_empty_fields() {
        let summary = StorageVersionMigrationSummary {
            name: "minimal".to_string(),
            resource_group: None,
            resource_version_field: None,
            resource_name: None,
            created_at: None,
        };

        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["name"], "minimal");
        assert!(json["resource_group"].is_null());
        assert!(json["resource_version_field"].is_null());
        assert!(json["resource_name"].is_null());
        assert!(json["created_at"].is_null());
    }

    #[test]
    fn extract_summary_from_storage_version_migration() {
        let svm = StorageVersionMigration {
            metadata: ObjectMeta {
                name: Some("migrate-configmaps".to_string()),
                labels: Some(BTreeMap::from([(
                    "app".to_string(),
                    "migration".to_string(),
                )])),
                ..Default::default()
            },
            spec: Some(StorageVersionMigrationSpec {
                resource: GroupVersionResource {
                    group: Some("".to_string()),
                    version: Some("v1".to_string()),
                    resource: Some("configmaps".to_string()),
                },
                continue_token: None,
            }),
            status: None,
        };

        let summary = extract_summary(&svm);
        assert_eq!(summary.name, "migrate-configmaps");
        assert_eq!(summary.resource_group.as_deref(), Some(""));
        assert_eq!(summary.resource_version_field.as_deref(), Some("v1"));
        assert_eq!(summary.resource_name.as_deref(), Some("configmaps"));
        assert!(summary.created_at.is_none());
    }

    #[test]
    fn extract_summary_from_minimal_storage_version_migration() {
        let svm = StorageVersionMigration {
            metadata: ObjectMeta {
                name: Some("empty-migration".to_string()),
                ..Default::default()
            },
            spec: None,
            status: None,
        };

        let summary = extract_summary(&svm);
        assert_eq!(summary.name, "empty-migration");
        assert!(summary.resource_group.is_none());
        assert!(summary.resource_version_field.is_none());
        assert!(summary.resource_name.is_none());
        assert!(summary.created_at.is_none());
    }

    #[test]
    fn extract_summary_with_custom_group() {
        let svm = StorageVersionMigration {
            metadata: ObjectMeta {
                name: Some("migrate-crds".to_string()),
                ..Default::default()
            },
            spec: Some(StorageVersionMigrationSpec {
                resource: GroupVersionResource {
                    group: Some("apps".to_string()),
                    version: Some("v1".to_string()),
                    resource: Some("deployments".to_string()),
                },
                continue_token: Some("abc123".to_string()),
            }),
            status: None,
        };

        let summary = extract_summary(&svm);
        assert_eq!(summary.name, "migrate-crds");
        assert_eq!(summary.resource_group.as_deref(), Some("apps"));
        assert_eq!(summary.resource_version_field.as_deref(), Some("v1"));
        assert_eq!(summary.resource_name.as_deref(), Some("deployments"));
    }
}
