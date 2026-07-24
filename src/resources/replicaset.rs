use std::collections::BTreeMap;

use k8s_openapi::api::apps::v1::ReplicaSet;
use kube::api::ListParams;
use serde::Serialize;

use crate::client::K8sClient;

fn api(
    client: &K8sClient,
    ns: &str,
) -> Result<kube::Api<ReplicaSet>, String> {
    if !client.is_namespace_allowed(ns) {
        return Err(format!("Namespace '{ns}' is not in the allowed list"));
    }
    Ok(kube::Api::namespaced(client.inner().clone(), ns))
}

// ---------------------------------------------------------------------------
// Summary / detail types
// ---------------------------------------------------------------------------

#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct ReplicaSetReplicaCounts {
    pub desired: i32,
    pub ready: i32,
    pub available: i32,
}

#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct ReplicaSetSummary {
    pub name: String,
    pub namespace: String,
    pub replicas: ReplicaSetReplicaCounts,
    pub image: String,
    pub owner: Option<String>,
    pub created_at: Option<String>,
}

#[derive(Serialize, Debug)]
pub struct ReplicaSetConditionSummary {
    pub condition_type: String,
    pub status: String,
    pub reason: Option<String>,
    pub message: Option<String>,
    pub last_transition: Option<String>,
}

#[derive(Serialize, Debug)]
pub struct ReplicaSetDetail {
    pub name: String,
    pub namespace: String,
    pub replicas: ReplicaSetReplicaCounts,
    pub image: String,
    pub owner: Option<String>,
    pub created_at: Option<String>,
    pub revision: Option<String>,
    pub conditions: Vec<ReplicaSetConditionSummary>,
    pub labels: BTreeMap<String, String>,
    pub annotations: BTreeMap<String, String>,
}

// ---------------------------------------------------------------------------
// Extraction helpers
// ---------------------------------------------------------------------------

fn primary_image(rs: &ReplicaSet) -> String {
    rs.spec
        .as_ref()
        .and_then(|s| s.template.as_ref())
        .and_then(|t| t.spec.as_ref())
        .and_then(|s| s.containers.first())
        .and_then(|c| c.image.clone())
        .unwrap_or_default()
}

fn owner_deployment(rs: &ReplicaSet) -> Option<String> {
    rs.metadata
        .owner_references
        .as_ref()
        .and_then(|refs| {
            refs.iter()
                .find(|r| r.kind == "Deployment")
                .map(|r| r.name.clone())
        })
}

fn replica_counts(rs: &ReplicaSet) -> ReplicaSetReplicaCounts {
    let desired = rs.spec.as_ref().and_then(|s| s.replicas).unwrap_or(1);
    let status = rs.status.as_ref();
    ReplicaSetReplicaCounts {
        desired,
        ready: status.and_then(|s| s.ready_replicas).unwrap_or(0),
        available: status.and_then(|s| s.available_replicas).unwrap_or(0),
    }
}

fn extract_summary(rs: &ReplicaSet) -> ReplicaSetSummary {
    let meta = &rs.metadata;
    ReplicaSetSummary {
        name: meta.name.clone().unwrap_or_default(),
        namespace: meta.namespace.clone().unwrap_or_default(),
        replicas: replica_counts(rs),
        image: primary_image(rs),
        owner: owner_deployment(rs),
        created_at: meta.creation_timestamp.as_ref().map(|t| t.0.to_string()),
    }
}

fn extract_detail(rs: &ReplicaSet) -> ReplicaSetDetail {
    let meta = &rs.metadata;
    let annotations = meta.annotations.clone().unwrap_or_default();

    let revision = annotations
        .get("deployment.kubernetes.io/revision")
        .cloned();

    let conditions = rs
        .status
        .as_ref()
        .and_then(|s| s.conditions.as_ref())
        .map(|conds| {
            conds
                .iter()
                .map(|c| ReplicaSetConditionSummary {
                    condition_type: c.type_.clone(),
                    status: c.status.clone(),
                    reason: c.reason.clone(),
                    message: c.message.clone(),
                    last_transition: c
                        .last_transition_time
                        .as_ref()
                        .map(|t| t.0.to_string()),
                })
                .collect()
        })
        .unwrap_or_default();

    ReplicaSetDetail {
        name: meta.name.clone().unwrap_or_default(),
        namespace: meta.namespace.clone().unwrap_or_default(),
        replicas: replica_counts(rs),
        image: primary_image(rs),
        owner: owner_deployment(rs),
        created_at: meta.creation_timestamp.as_ref().map(|t| t.0.to_string()),
        revision,
        conditions,
        labels: meta.labels.clone().unwrap_or_default(),
        annotations,
    }
}

// ---------------------------------------------------------------------------
// Tool definitions
// ---------------------------------------------------------------------------

pub fn tool_definitions() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "name": "list_replicasets",
            "description": "List ReplicaSets in a namespace. Returns name, namespace, replica counts (desired/ready/available), image, owner deployment, and creation time. Optionally filter by label selector.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "label_selector": { "type": "string", "description": "Label selector to filter ReplicaSets (e.g. app=nginx)" }
                },
                "required": ["namespace"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "get_replicaset",
            "description": "Get detailed info for a single ReplicaSet including replica counts, image, owner deployment, revision annotation, conditions, labels, and annotations.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "ReplicaSet name" }
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
        "list_replicasets" => list_replicasets(client, args).await,
        "get_replicaset" => get_replicaset(client, args).await,
        _ => return None,
    };
    Some(result)
}

// ---------------------------------------------------------------------------
// Tool implementations
// ---------------------------------------------------------------------------

async fn list_replicasets(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let ns = args["namespace"]
        .as_str()
        .ok_or("namespace is required")?;
    let rs_api = api(client, ns)?;

    let mut lp = ListParams::default();
    if let Some(sel) = args["label_selector"].as_str() {
        lp = lp.labels(sel);
    }

    let list = rs_api
        .list(&lp)
        .await
        .map_err(|e| e.to_string())?;

    let summaries: Vec<serde_json::Value> = list
        .iter()
        .map(|rs| serde_json::to_value(extract_summary(rs)).unwrap_or_default())
        .collect();

    serde_json::to_string_pretty(&summaries).map_err(|e| e.to_string())
}

async fn get_replicaset(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let ns = args["namespace"]
        .as_str()
        .ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;

    let rs_api = api(client, ns)?;
    let rs = rs_api.get(name).await.map_err(|e| e.to_string())?;
    let detail = extract_detail(&rs);

    serde_json::to_string_pretty(
        &serde_json::to_value(detail).unwrap_or_default(),
    )
    .map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::api::apps::v1::{ReplicaSet, ReplicaSetSpec, ReplicaSetStatus};
    use k8s_openapi::api::core::v1::{Container, PodSpec, PodTemplateSpec};
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::{LabelSelector, ObjectMeta, OwnerReference};
    use std::collections::BTreeMap;

    /// Build a minimal ReplicaSet for testing extraction.
    fn make_test_replicaset() -> ReplicaSet {
        let mut labels = BTreeMap::new();
        labels.insert("app".to_string(), "test-rs".to_string());

        let mut annotations = BTreeMap::new();
        annotations.insert(
            "deployment.kubernetes.io/revision".to_string(),
            "3".to_string(),
        );

        ReplicaSet {
            metadata: ObjectMeta {
                name: Some("my-deploy-abc123".to_string()),
                namespace: Some("default".to_string()),
                labels: Some(labels),
                annotations: Some(annotations),
                owner_references: Some(vec![OwnerReference {
                    api_version: "apps/v1".to_string(),
                    kind: "Deployment".to_string(),
                    name: "my-deploy".to_string(),
                    uid: "uid-123".to_string(),
                    ..Default::default()
                }]),
                ..Default::default()
            },
            spec: Some(ReplicaSetSpec {
                replicas: Some(3),
                selector: LabelSelector {
                    match_labels: Some({
                        let mut sel = BTreeMap::new();
                        sel.insert("app".to_string(), "test-rs".to_string());
                        sel
                    }),
                    ..Default::default()
                },
                template: Some(PodTemplateSpec {
                    metadata: None,
                    spec: Some(PodSpec {
                        containers: vec![Container {
                            name: "app".to_string(),
                            image: Some("nginx:1.25".to_string()),
                            ..Default::default()
                        }],
                        ..Default::default()
                    }),
                }),
                ..Default::default()
            }),
            status: Some(ReplicaSetStatus {
                replicas: 3,
                ready_replicas: Some(2),
                available_replicas: Some(2),
                ..Default::default()
            }),
        }
    }

    #[test]
    fn tool_definitions_returns_two_unique_tools() {
        let defs = tool_definitions();
        assert_eq!(defs.len(), 2);

        let names: Vec<&str> = defs
            .iter()
            .filter_map(|d| d["name"].as_str())
            .collect();
        assert_eq!(names.len(), 2);

        let mut unique = names.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), 2, "tool names must be unique");

        assert!(names.contains(&"list_replicasets"));
        assert!(names.contains(&"get_replicaset"));
    }

    #[test]
    fn summary_serialization() {
        let summary = ReplicaSetSummary {
            name: "my-deploy-abc123".to_string(),
            namespace: "prod".to_string(),
            replicas: ReplicaSetReplicaCounts {
                desired: 5,
                ready: 4,
                available: 4,
            },
            image: "nginx:1.25".to_string(),
            owner: Some("my-deploy".to_string()),
            created_at: Some("2025-01-01T00:00:00Z".to_string()),
        };

        let json = serde_json::to_string_pretty(&summary).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["name"], "my-deploy-abc123");
        assert_eq!(parsed["namespace"], "prod");
        assert_eq!(parsed["replicas"]["desired"], 5);
        assert_eq!(parsed["replicas"]["ready"], 4);
        assert_eq!(parsed["replicas"]["available"], 4);
        assert_eq!(parsed["image"], "nginx:1.25");
        assert_eq!(parsed["owner"], "my-deploy");
        assert_eq!(parsed["created_at"], "2025-01-01T00:00:00Z");
    }

    #[test]
    fn extract_summary_from_replicaset() {
        let rs = make_test_replicaset();
        let summary = extract_summary(&rs);

        assert_eq!(summary.name, "my-deploy-abc123");
        assert_eq!(summary.namespace, "default");
        assert_eq!(summary.image, "nginx:1.25");
        assert_eq!(summary.replicas.desired, 3);
        assert_eq!(summary.replicas.ready, 2);
        assert_eq!(summary.replicas.available, 2);
        assert_eq!(summary.owner, Some("my-deploy".to_string()));
    }

    #[test]
    fn extract_detail_from_replicaset() {
        let rs = make_test_replicaset();
        let detail = extract_detail(&rs);

        assert_eq!(detail.name, "my-deploy-abc123");
        assert_eq!(detail.namespace, "default");
        assert_eq!(detail.image, "nginx:1.25");
        assert_eq!(detail.replicas.desired, 3);
        assert_eq!(detail.replicas.ready, 2);
        assert_eq!(detail.replicas.available, 2);
        assert_eq!(detail.owner, Some("my-deploy".to_string()));
        assert_eq!(detail.revision, Some("3".to_string()));
        assert!(detail.conditions.is_empty());
        assert_eq!(
            detail.labels.get("app").map(|s| s.as_str()),
            Some("test-rs")
        );
        assert!(detail.annotations.contains_key("deployment.kubernetes.io/revision"));
    }

    #[test]
    fn extract_summary_no_status_no_owner() {
        let rs = ReplicaSet {
            metadata: ObjectMeta {
                name: Some("orphan-rs".to_string()),
                namespace: Some("test".to_string()),
                ..Default::default()
            },
            spec: None,
            status: None,
        };
        let summary = extract_summary(&rs);

        assert_eq!(summary.name, "orphan-rs");
        assert_eq!(summary.namespace, "test");
        assert_eq!(summary.replicas.desired, 1);
        assert_eq!(summary.replicas.ready, 0);
        assert_eq!(summary.replicas.available, 0);
        assert_eq!(summary.image, "");
        assert_eq!(summary.owner, None);
    }
}
