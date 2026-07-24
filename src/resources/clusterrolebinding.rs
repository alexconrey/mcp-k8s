use std::collections::BTreeMap;

use k8s_openapi::api::rbac::v1::{ClusterRoleBinding, RoleRef, Subject};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::api::{DeleteParams, ListParams, PostParams};
use serde::Serialize;

use crate::client::K8sClient;

fn api(client: &K8sClient) -> kube::Api<ClusterRoleBinding> {
    kube::Api::all(client.inner().clone())
}

#[derive(Serialize, Debug, PartialEq)]
pub struct ClusterRoleBindingSummary {
    pub name: String,
    pub role_ref_kind: String,
    pub role_ref_name: String,
    pub subjects_count: usize,
    pub created_at: Option<String>,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct SubjectSummary {
    pub kind: String,
    pub name: String,
    pub namespace: Option<String>,
}

fn extract_summary(crb: &ClusterRoleBinding) -> ClusterRoleBindingSummary {
    let meta = &crb.metadata;
    let subjects_count = crb.subjects.as_ref().map(|s| s.len()).unwrap_or(0);

    ClusterRoleBindingSummary {
        name: meta.name.clone().unwrap_or_default(),
        role_ref_kind: crb.role_ref.kind.clone(),
        role_ref_name: crb.role_ref.name.clone(),
        subjects_count,
        created_at: meta.creation_timestamp.as_ref().map(|t| t.0.to_string()),
    }
}

fn extract_subjects(crb: &ClusterRoleBinding) -> Vec<SubjectSummary> {
    crb.subjects
        .as_ref()
        .map(|subjects| {
            subjects
                .iter()
                .map(|s| SubjectSummary {
                    kind: s.kind.clone(),
                    name: s.name.clone(),
                    namespace: s.namespace.clone(),
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn tool_definitions() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "name": "list_clusterrolebindings",
            "description": "List all ClusterRoleBindings in the cluster. Returns name, role_ref (kind and name), subjects count, and created_at.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "get_clusterrolebinding",
            "description": "Get a ClusterRoleBinding by name. Returns subjects (kind, name, namespace), labels, and annotations.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "ClusterRoleBinding name" }
                },
                "required": ["name"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "create_clusterrolebinding",
            "description": "Create a ClusterRoleBinding. Binds a ClusterRole or Role to subjects (users, groups, or service accounts).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "ClusterRoleBinding name" },
                    "role_ref": {
                        "type": "object",
                        "description": "The role to bind. api_group defaults to rbac.authorization.k8s.io.",
                        "properties": {
                            "kind": { "type": "string", "description": "Kind of the role (ClusterRole or Role)" },
                            "name": { "type": "string", "description": "Name of the role" },
                            "api_group": { "type": "string", "description": "API group (default: rbac.authorization.k8s.io)" }
                        },
                        "required": ["kind", "name"]
                    },
                    "subjects": {
                        "type": "array",
                        "description": "Subjects to bind the role to",
                        "items": {
                            "type": "object",
                            "properties": {
                                "kind": { "type": "string", "description": "Kind of subject (User, Group, or ServiceAccount)" },
                                "name": { "type": "string", "description": "Name of the subject" },
                                "namespace": { "type": "string", "description": "Namespace of the subject (required for ServiceAccount)" }
                            },
                            "required": ["kind", "name"]
                        }
                    }
                },
                "required": ["name", "role_ref", "subjects"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "delete_clusterrolebinding",
            "description": "Delete a ClusterRoleBinding by name.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "ClusterRoleBinding name" }
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
        "list_clusterrolebindings" => list_clusterrolebindings(client).await,
        "get_clusterrolebinding" => get_clusterrolebinding(client, args).await,
        "create_clusterrolebinding" => create_clusterrolebinding(client, args).await,
        "delete_clusterrolebinding" => delete_clusterrolebinding(client, args).await,
        _ => return None,
    };
    Some(result)
}

async fn list_clusterrolebindings(client: &K8sClient) -> Result<String, String> {
    let crb_api = api(client);
    let list = crb_api
        .list(&ListParams::default())
        .await
        .map_err(|e| e.to_string())?;

    let summaries: Vec<serde_json::Value> = list
        .iter()
        .map(|crb| {
            let s = extract_summary(crb);
            serde_json::json!({
                "name": s.name,
                "role_ref": {
                    "kind": s.role_ref_kind,
                    "name": s.role_ref_name,
                },
                "subjects_count": s.subjects_count,
                "created_at": s.created_at,
            })
        })
        .collect();

    serde_json::to_string_pretty(&summaries).map_err(|e| e.to_string())
}

async fn get_clusterrolebinding(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let name = args["name"].as_str().ok_or("name is required")?;

    let crb_api = api(client);
    let crb = crb_api.get(name).await.map_err(|e| e.to_string())?;

    let meta = &crb.metadata;
    let subjects = extract_subjects(&crb);

    let result = serde_json::json!({
        "name": meta.name.clone().unwrap_or_default(),
        "role_ref": {
            "kind": crb.role_ref.kind,
            "name": crb.role_ref.name,
            "api_group": crb.role_ref.api_group,
        },
        "subjects": subjects,
        "labels": meta.labels.clone().unwrap_or_default(),
        "annotations": meta.annotations.clone().unwrap_or_default(),
        "created_at": meta.creation_timestamp.as_ref().map(|t| t.0.to_string()),
    });

    serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
}

async fn create_clusterrolebinding(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let name = args["name"].as_str().ok_or("name is required")?;

    let role_ref_val = args.get("role_ref").ok_or("role_ref is required")?;
    let role_ref_kind = role_ref_val["kind"]
        .as_str()
        .ok_or("role_ref.kind is required")?;
    let role_ref_name = role_ref_val["name"]
        .as_str()
        .ok_or("role_ref.name is required")?;
    let role_ref_api_group = role_ref_val["api_group"]
        .as_str()
        .unwrap_or("rbac.authorization.k8s.io");

    let subjects_val = args
        .get("subjects")
        .and_then(|v| v.as_array())
        .ok_or("subjects is required and must be an array")?;

    let subjects: Vec<Subject> = subjects_val
        .iter()
        .map(|s| {
            let kind = s["kind"].as_str().unwrap_or("User").to_string();
            let subj_name = s["name"].as_str().unwrap_or_default().to_string();
            let namespace = s["namespace"].as_str().map(|n| n.to_string());

            let api_group = match kind.as_str() {
                "ServiceAccount" => "".to_string(),
                _ => "rbac.authorization.k8s.io".to_string(),
            };

            Subject {
                kind,
                name: subj_name,
                namespace,
                api_group: Some(api_group),
            }
        })
        .collect();

    let mut labels = BTreeMap::new();
    labels.insert(
        "app.kubernetes.io/managed-by".to_string(),
        "mcp-k8s".to_string(),
    );

    let crb = ClusterRoleBinding {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            labels: Some(labels),
            ..Default::default()
        },
        role_ref: RoleRef {
            api_group: role_ref_api_group.to_string(),
            kind: role_ref_kind.to_string(),
            name: role_ref_name.to_string(),
        },
        subjects: Some(subjects),
    };

    let crb_api = api(client);
    let created = crb_api
        .create(&PostParams::default(), &crb)
        .await
        .map_err(|e| e.to_string())?;

    let summary = extract_summary(&created);
    serde_json::to_string_pretty(&summary).map_err(|e| e.to_string())
}

async fn delete_clusterrolebinding(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let name = args["name"].as_str().ok_or("name is required")?;

    let crb_api = api(client);
    crb_api
        .delete(name, &DeleteParams::default())
        .await
        .map_err(|e| e.to_string())?;

    let result = serde_json::json!({
        "deleted": true,
        "name": name,
    });
    serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

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

        assert!(names.contains(&"list_clusterrolebindings"));
        assert!(names.contains(&"get_clusterrolebinding"));
        assert!(names.contains(&"create_clusterrolebinding"));
        assert!(names.contains(&"delete_clusterrolebinding"));
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
    fn cluster_role_binding_summary_serialization() {
        let summary = ClusterRoleBindingSummary {
            name: "admin-binding".to_string(),
            role_ref_kind: "ClusterRole".to_string(),
            role_ref_name: "admin".to_string(),
            subjects_count: 3,
            created_at: Some("2024-01-01T00:00:00Z".to_string()),
        };

        let json = serde_json::to_value(&summary).expect("serialization should succeed");
        assert_eq!(json["name"], "admin-binding");
        assert_eq!(json["role_ref_kind"], "ClusterRole");
        assert_eq!(json["role_ref_name"], "admin");
        assert_eq!(json["subjects_count"], 3);
        assert_eq!(json["created_at"], "2024-01-01T00:00:00Z");
    }

    #[test]
    fn cluster_role_binding_summary_serialization_no_timestamp() {
        let summary = ClusterRoleBindingSummary {
            name: "viewer-binding".to_string(),
            role_ref_kind: "ClusterRole".to_string(),
            role_ref_name: "view".to_string(),
            subjects_count: 0,
            created_at: None,
        };

        let json = serde_json::to_value(&summary).expect("serialization should succeed");
        assert_eq!(json["name"], "viewer-binding");
        assert_eq!(json["subjects_count"], 0);
        assert!(json["created_at"].is_null());
    }

    #[test]
    fn subject_summary_serialization() {
        let subject = SubjectSummary {
            kind: "ServiceAccount".to_string(),
            name: "my-sa".to_string(),
            namespace: Some("default".to_string()),
        };

        let json = serde_json::to_value(&subject).expect("serialization should succeed");
        assert_eq!(json["kind"], "ServiceAccount");
        assert_eq!(json["name"], "my-sa");
        assert_eq!(json["namespace"], "default");
    }

    #[test]
    fn subject_summary_serialization_no_namespace() {
        let subject = SubjectSummary {
            kind: "User".to_string(),
            name: "admin@example.com".to_string(),
            namespace: None,
        };

        let json = serde_json::to_value(&subject).expect("serialization should succeed");
        assert_eq!(json["kind"], "User");
        assert_eq!(json["name"], "admin@example.com");
        assert!(json["namespace"].is_null());
    }

    fn make_test_clusterrolebinding() -> ClusterRoleBinding {
        let mut labels = BTreeMap::new();
        labels.insert(
            "app.kubernetes.io/managed-by".to_string(),
            "mcp-k8s".to_string(),
        );

        let mut annotations = BTreeMap::new();
        annotations.insert(
            "description".to_string(),
            "Test binding".to_string(),
        );

        ClusterRoleBinding {
            metadata: ObjectMeta {
                name: Some("test-binding".to_string()),
                labels: Some(labels),
                annotations: Some(annotations),
                ..Default::default()
            },
            role_ref: RoleRef {
                api_group: "rbac.authorization.k8s.io".to_string(),
                kind: "ClusterRole".to_string(),
                name: "cluster-admin".to_string(),
            },
            subjects: Some(vec![
                Subject {
                    kind: "User".to_string(),
                    name: "admin@example.com".to_string(),
                    namespace: None,
                    api_group: Some("rbac.authorization.k8s.io".to_string()),
                },
                Subject {
                    kind: "ServiceAccount".to_string(),
                    name: "default".to_string(),
                    namespace: Some("kube-system".to_string()),
                    api_group: Some(String::new()),
                },
                Subject {
                    kind: "Group".to_string(),
                    name: "system:masters".to_string(),
                    namespace: None,
                    api_group: Some("rbac.authorization.k8s.io".to_string()),
                },
            ]),
        }
    }

    #[test]
    fn extract_summary_from_clusterrolebinding() {
        let crb = make_test_clusterrolebinding();
        let summary = extract_summary(&crb);

        assert_eq!(summary.name, "test-binding");
        assert_eq!(summary.role_ref_kind, "ClusterRole");
        assert_eq!(summary.role_ref_name, "cluster-admin");
        assert_eq!(summary.subjects_count, 3);
        assert!(summary.created_at.is_none());
    }

    #[test]
    fn extract_subjects_from_clusterrolebinding() {
        let crb = make_test_clusterrolebinding();
        let subjects = extract_subjects(&crb);

        assert_eq!(subjects.len(), 3);

        assert_eq!(subjects[0].kind, "User");
        assert_eq!(subjects[0].name, "admin@example.com");
        assert!(subjects[0].namespace.is_none());

        assert_eq!(subjects[1].kind, "ServiceAccount");
        assert_eq!(subjects[1].name, "default");
        assert_eq!(subjects[1].namespace.as_deref(), Some("kube-system"));

        assert_eq!(subjects[2].kind, "Group");
        assert_eq!(subjects[2].name, "system:masters");
        assert!(subjects[2].namespace.is_none());
    }

    #[test]
    fn extract_subjects_from_empty_clusterrolebinding() {
        let crb = ClusterRoleBinding {
            metadata: ObjectMeta {
                name: Some("empty-binding".to_string()),
                ..Default::default()
            },
            role_ref: RoleRef {
                api_group: "rbac.authorization.k8s.io".to_string(),
                kind: "ClusterRole".to_string(),
                name: "view".to_string(),
            },
            subjects: None,
        };

        let summary = extract_summary(&crb);
        assert_eq!(summary.name, "empty-binding");
        assert_eq!(summary.subjects_count, 0);

        let subjects = extract_subjects(&crb);
        assert!(subjects.is_empty());
    }

    #[test]
    fn extract_summary_empty_metadata() {
        let crb = ClusterRoleBinding {
            metadata: ObjectMeta::default(),
            role_ref: RoleRef {
                api_group: "rbac.authorization.k8s.io".to_string(),
                kind: "ClusterRole".to_string(),
                name: "edit".to_string(),
            },
            subjects: None,
        };

        let summary = extract_summary(&crb);
        assert_eq!(summary.name, "");
        assert_eq!(summary.role_ref_kind, "ClusterRole");
        assert_eq!(summary.role_ref_name, "edit");
        assert_eq!(summary.subjects_count, 0);
        assert!(summary.created_at.is_none());
    }
}
