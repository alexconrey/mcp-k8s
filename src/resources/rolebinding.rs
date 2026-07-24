use std::collections::BTreeMap;

use k8s_openapi::api::rbac::v1::RoleBinding;
use k8s_openapi::api::rbac::v1::{RoleRef, Subject};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::api::{DeleteParams, ListParams, PostParams};
use serde::Serialize;

use crate::client::K8sClient;

fn api(client: &K8sClient, ns: &str) -> Result<kube::Api<RoleBinding>, String> {
    if !client.is_namespace_allowed(ns) {
        return Err(format!("Namespace '{ns}' is not in the allowed list"));
    }
    Ok(kube::Api::namespaced(client.inner().clone(), ns))
}

#[derive(Serialize, Debug)]
pub struct SubjectSummary {
    pub kind: String,
    pub name: String,
    pub namespace: Option<String>,
}

#[derive(Serialize, Debug)]
pub struct RoleBindingSummary {
    pub name: String,
    pub namespace: String,
    pub role_ref_kind: String,
    pub role_ref_name: String,
    pub subjects_count: usize,
    pub created_at: Option<String>,
}

fn extract_summary(rb: &RoleBinding) -> RoleBindingSummary {
    let meta = &rb.metadata;
    let subjects_count = rb.subjects.as_ref().map(|s| s.len()).unwrap_or(0);

    RoleBindingSummary {
        name: meta.name.clone().unwrap_or_default(),
        namespace: meta.namespace.clone().unwrap_or_default(),
        role_ref_kind: rb.role_ref.kind.clone(),
        role_ref_name: rb.role_ref.name.clone(),
        subjects_count,
        created_at: meta.creation_timestamp.as_ref().map(|t| t.0.to_string()),
    }
}

fn extract_subjects(rb: &RoleBinding) -> Vec<SubjectSummary> {
    rb.subjects
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
            "name": "list_rolebindings",
            "description": "List role bindings in a namespace. Returns name, namespace, role_ref (kind and name), subjects count, and created_at.",
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
            "name": "get_rolebinding",
            "description": "Get a role binding by name. Returns name, namespace, role_ref (kind and name), subjects count, created_at, subjects (kind, name, namespace), labels, and annotations.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "RoleBinding name" }
                },
                "required": ["namespace", "name"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "create_rolebinding",
            "description": "Create a role binding in a namespace. Binds a Role or ClusterRole to a set of subjects.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "RoleBinding name" },
                    "role_ref": {
                        "type": "object",
                        "description": "The role to bind (Role or ClusterRole)",
                        "properties": {
                            "kind": { "type": "string", "description": "Kind of the role (Role or ClusterRole)" },
                            "name": { "type": "string", "description": "Name of the role" },
                            "api_group": { "type": "string", "description": "API group (default: rbac.authorization.k8s.io)" }
                        },
                        "required": ["kind", "name"],
                        "additionalProperties": false
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
                            "required": ["kind", "name"],
                            "additionalProperties": false
                        }
                    }
                },
                "required": ["namespace", "name", "role_ref", "subjects"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "delete_rolebinding",
            "description": "Delete a role binding by name from a namespace.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "RoleBinding name" }
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
        "list_rolebindings" => list_rolebindings(client, args).await,
        "get_rolebinding" => get_rolebinding(client, args).await,
        "create_rolebinding" => create_rolebinding(client, args).await,
        "delete_rolebinding" => delete_rolebinding(client, args).await,
        _ => return None,
    };
    Some(result)
}

async fn list_rolebindings(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let rb_api = api(client, ns)?;
    let label_selector = args["label_selector"].as_str();
    let mut lp = ListParams::default();
    if let Some(sel) = label_selector {
        lp = lp.labels(sel);
    }
    let list = rb_api.list(&lp).await.map_err(|e| e.to_string())?;

    let summaries: Vec<serde_json::Value> = list
        .iter()
        .map(|rb| {
            let s = extract_summary(rb);
            serde_json::json!({
                "name": s.name,
                "namespace": s.namespace,
                "role_ref_kind": s.role_ref_kind,
                "role_ref_name": s.role_ref_name,
                "subjects_count": s.subjects_count,
                "created_at": s.created_at,
            })
        })
        .collect();

    serde_json::to_string_pretty(&summaries).map_err(|e| e.to_string())
}

async fn get_rolebinding(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;
    let rb_api = api(client, ns)?;
    let rb = rb_api.get(name).await.map_err(|e| e.to_string())?;

    let summary = extract_summary(&rb);
    let subjects = extract_subjects(&rb);
    let meta = &rb.metadata;
    let result = serde_json::json!({
        "name": summary.name,
        "namespace": summary.namespace,
        "role_ref_kind": summary.role_ref_kind,
        "role_ref_name": summary.role_ref_name,
        "subjects_count": summary.subjects_count,
        "created_at": summary.created_at,
        "subjects": subjects,
        "labels": meta.labels.clone().unwrap_or_default(),
        "annotations": meta.annotations.clone().unwrap_or_default(),
    });

    serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
}

async fn create_rolebinding(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
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
            let kind = s["kind"]
                .as_str()
                .ok_or("subject kind is required")
                .map(|v| v.to_string())?;
            let subj_name = s["name"]
                .as_str()
                .ok_or("subject name is required")
                .map(|v| v.to_string())?;
            let subj_ns = s["namespace"].as_str().map(|v| v.to_string());

            Ok(Subject {
                kind,
                name: subj_name,
                namespace: subj_ns,
                api_group: Some(match s["kind"].as_str().unwrap_or("") {
                    "ServiceAccount" => String::new(),
                    _ => "rbac.authorization.k8s.io".to_string(),
                }),
            })
        })
        .collect::<Result<Vec<Subject>, String>>()?;

    let mut labels = BTreeMap::new();
    labels.insert(
        "app.kubernetes.io/managed-by".to_string(),
        "mcp-k8s".to_string(),
    );

    let rb = RoleBinding {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            namespace: Some(ns.to_string()),
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

    let rb_api = api(client, ns)?;
    let created = rb_api
        .create(&PostParams::default(), &rb)
        .await
        .map_err(|e| e.to_string())?;

    let summary = extract_summary(&created);
    serde_json::to_string_pretty(&summary).map_err(|e| e.to_string())
}

async fn delete_rolebinding(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;

    let rb_api = api(client, ns)?;
    rb_api
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
    fn tool_definitions_returns_four_unique_tools() {
        let defs = tool_definitions();
        assert_eq!(defs.len(), 4);

        let names: Vec<&str> = defs.iter().map(|d| d["name"].as_str().unwrap()).collect();

        let mut unique = names.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(names.len(), unique.len(), "tool names must be unique");

        assert!(names.contains(&"list_rolebindings"));
        assert!(names.contains(&"get_rolebinding"));
        assert!(names.contains(&"create_rolebinding"));
        assert!(names.contains(&"delete_rolebinding"));
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
    fn rolebinding_summary_serialization() {
        let summary = RoleBindingSummary {
            name: "my-rb".to_string(),
            namespace: "default".to_string(),
            role_ref_kind: "ClusterRole".to_string(),
            role_ref_name: "admin".to_string(),
            subjects_count: 3,
            created_at: Some("2024-01-01T00:00:00Z".to_string()),
        };

        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["name"], "my-rb");
        assert_eq!(json["namespace"], "default");
        assert_eq!(json["role_ref_kind"], "ClusterRole");
        assert_eq!(json["role_ref_name"], "admin");
        assert_eq!(json["subjects_count"], 3);
        assert_eq!(json["created_at"], "2024-01-01T00:00:00Z");
    }

    #[test]
    fn rolebinding_summary_serialization_empty_fields() {
        let summary = RoleBindingSummary {
            name: "empty-rb".to_string(),
            namespace: "ns".to_string(),
            role_ref_kind: "Role".to_string(),
            role_ref_name: "viewer".to_string(),
            subjects_count: 0,
            created_at: None,
        };

        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["name"], "empty-rb");
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

        let json = serde_json::to_value(&subject).unwrap();
        assert_eq!(json["kind"], "ServiceAccount");
        assert_eq!(json["name"], "my-sa");
        assert_eq!(json["namespace"], "default");
    }

    #[test]
    fn subject_summary_serialization_no_namespace() {
        let subject = SubjectSummary {
            kind: "User".to_string(),
            name: "jane".to_string(),
            namespace: None,
        };

        let json = serde_json::to_value(&subject).unwrap();
        assert_eq!(json["kind"], "User");
        assert_eq!(json["name"], "jane");
        assert!(json["namespace"].is_null());
    }

    #[test]
    fn extract_summary_from_rolebinding() {
        let rb = RoleBinding {
            metadata: ObjectMeta {
                name: Some("test-rb".to_string()),
                namespace: Some("prod".to_string()),
                labels: Some(BTreeMap::from([("app".to_string(), "myapp".to_string())])),
                ..Default::default()
            },
            role_ref: RoleRef {
                api_group: "rbac.authorization.k8s.io".to_string(),
                kind: "ClusterRole".to_string(),
                name: "edit".to_string(),
            },
            subjects: Some(vec![
                Subject {
                    kind: "ServiceAccount".to_string(),
                    name: "deployer".to_string(),
                    namespace: Some("prod".to_string()),
                    api_group: Some(String::new()),
                },
                Subject {
                    kind: "User".to_string(),
                    name: "jane".to_string(),
                    namespace: None,
                    api_group: Some("rbac.authorization.k8s.io".to_string()),
                },
            ]),
        };

        let summary = extract_summary(&rb);
        assert_eq!(summary.name, "test-rb");
        assert_eq!(summary.namespace, "prod");
        assert_eq!(summary.role_ref_kind, "ClusterRole");
        assert_eq!(summary.role_ref_name, "edit");
        assert_eq!(summary.subjects_count, 2);
        assert!(summary.created_at.is_none());
    }

    #[test]
    fn extract_summary_from_empty_rolebinding() {
        let rb = RoleBinding {
            metadata: ObjectMeta {
                name: Some("empty-rb".to_string()),
                namespace: Some("default".to_string()),
                ..Default::default()
            },
            role_ref: RoleRef {
                api_group: "rbac.authorization.k8s.io".to_string(),
                kind: "Role".to_string(),
                name: "viewer".to_string(),
            },
            subjects: None,
        };

        let summary = extract_summary(&rb);
        assert_eq!(summary.name, "empty-rb");
        assert_eq!(summary.namespace, "default");
        assert_eq!(summary.role_ref_kind, "Role");
        assert_eq!(summary.role_ref_name, "viewer");
        assert_eq!(summary.subjects_count, 0);
        assert!(summary.created_at.is_none());
    }

    #[test]
    fn extract_subjects_from_rolebinding() {
        let rb = RoleBinding {
            metadata: ObjectMeta::default(),
            role_ref: RoleRef {
                api_group: "rbac.authorization.k8s.io".to_string(),
                kind: "Role".to_string(),
                name: "reader".to_string(),
            },
            subjects: Some(vec![
                Subject {
                    kind: "ServiceAccount".to_string(),
                    name: "my-sa".to_string(),
                    namespace: Some("kube-system".to_string()),
                    api_group: Some(String::new()),
                },
                Subject {
                    kind: "Group".to_string(),
                    name: "developers".to_string(),
                    namespace: None,
                    api_group: Some("rbac.authorization.k8s.io".to_string()),
                },
            ]),
        };

        let subjects = extract_subjects(&rb);
        assert_eq!(subjects.len(), 2);
        assert_eq!(subjects[0].kind, "ServiceAccount");
        assert_eq!(subjects[0].name, "my-sa");
        assert_eq!(subjects[0].namespace, Some("kube-system".to_string()));
        assert_eq!(subjects[1].kind, "Group");
        assert_eq!(subjects[1].name, "developers");
        assert!(subjects[1].namespace.is_none());
    }

    #[test]
    fn extract_subjects_from_empty_rolebinding() {
        let rb = RoleBinding {
            metadata: ObjectMeta::default(),
            role_ref: RoleRef {
                api_group: "rbac.authorization.k8s.io".to_string(),
                kind: "Role".to_string(),
                name: "reader".to_string(),
            },
            subjects: None,
        };

        let subjects = extract_subjects(&rb);
        assert!(subjects.is_empty());
    }
}
