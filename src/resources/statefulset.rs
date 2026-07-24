use std::collections::BTreeMap;

use k8s_openapi::api::apps::v1::{StatefulSet, StatefulSetSpec};
use k8s_openapi::api::core::v1::{Container, ContainerPort, PodSpec, PodTemplateSpec};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{LabelSelector, ObjectMeta};
use kube::api::{DeleteParams, ListParams, Patch, PatchParams, PostParams};
use serde::Serialize;

use crate::client::K8sClient;

fn api(client: &K8sClient, ns: &str) -> Result<kube::Api<StatefulSet>, String> {
    if !client.is_namespace_allowed(ns) {
        return Err(format!("Namespace '{ns}' is not in the allowed list"));
    }
    Ok(kube::Api::namespaced(client.inner().clone(), ns))
}

// ---------------------------------------------------------------------------
// Summary / detail types
// ---------------------------------------------------------------------------

#[derive(Serialize, Debug, Clone)]
pub struct StatefulSetReplicaCounts {
    pub desired: i32,
    pub ready: i32,
    pub updated: i32,
}

#[derive(Serialize, Debug, Clone)]
pub struct StatefulSetSummary {
    pub name: String,
    pub namespace: String,
    pub image: String,
    pub replicas: StatefulSetReplicaCounts,
    pub created_at: Option<String>,
    pub labels: BTreeMap<String, String>,
}

#[derive(Serialize, Debug)]
pub struct StatefulSetConditionSummary {
    pub condition_type: String,
    pub status: String,
    pub reason: Option<String>,
    pub message: Option<String>,
    pub last_transition: Option<String>,
}

#[derive(Serialize, Debug)]
pub struct VolumeClaimTemplateSummary {
    pub name: String,
    pub storage_class: Option<String>,
    pub access_modes: Vec<String>,
    pub storage: Option<String>,
}

#[derive(Serialize, Debug)]
pub struct StatefulSetDetail {
    pub name: String,
    pub namespace: String,
    pub image: String,
    pub replicas: StatefulSetReplicaCounts,
    pub created_at: Option<String>,
    pub labels: BTreeMap<String, String>,
    pub annotations: BTreeMap<String, String>,
    pub conditions: Vec<StatefulSetConditionSummary>,
    pub service_name: String,
    pub pod_management_policy: String,
    pub update_strategy: String,
    pub volume_claim_templates: Vec<VolumeClaimTemplateSummary>,
}

// ---------------------------------------------------------------------------
// Extraction helpers
// ---------------------------------------------------------------------------

fn primary_image(sts: &StatefulSet) -> String {
    sts.spec
        .as_ref()
        .and_then(|s| s.template.spec.as_ref())
        .and_then(|s| s.containers.first())
        .and_then(|c| c.image.clone())
        .unwrap_or_default()
}

fn replica_counts(sts: &StatefulSet) -> StatefulSetReplicaCounts {
    let desired = sts.spec.as_ref().and_then(|s| s.replicas).unwrap_or(1);
    let status = sts.status.as_ref();
    StatefulSetReplicaCounts {
        desired,
        ready: status.and_then(|s| s.ready_replicas).unwrap_or(0),
        updated: status.and_then(|s| s.updated_replicas).unwrap_or(0),
    }
}

fn extract_summary(sts: &StatefulSet) -> StatefulSetSummary {
    let meta = &sts.metadata;
    StatefulSetSummary {
        name: meta.name.clone().unwrap_or_default(),
        namespace: meta.namespace.clone().unwrap_or_default(),
        image: primary_image(sts),
        replicas: replica_counts(sts),
        created_at: meta.creation_timestamp.as_ref().map(|t| t.0.to_string()),
        labels: meta.labels.clone().unwrap_or_default(),
    }
}

fn extract_detail(sts: &StatefulSet) -> StatefulSetDetail {
    let meta = &sts.metadata;
    let spec = sts.spec.as_ref();

    let conditions = sts
        .status
        .as_ref()
        .and_then(|s| s.conditions.as_ref())
        .map(|conds| {
            conds
                .iter()
                .map(|c| StatefulSetConditionSummary {
                    condition_type: c.type_.clone(),
                    status: c.status.clone(),
                    reason: c.reason.clone(),
                    message: c.message.clone(),
                    last_transition: c.last_transition_time.as_ref().map(|t| t.0.to_string()),
                })
                .collect()
        })
        .unwrap_or_default();

    let service_name = spec
        .and_then(|s| s.service_name.clone())
        .unwrap_or_default();

    let pod_management_policy = spec
        .and_then(|s| s.pod_management_policy.clone())
        .unwrap_or_else(|| "OrderedReady".to_string());

    let update_strategy = spec
        .and_then(|s| s.update_strategy.as_ref())
        .and_then(|u| u.type_.clone())
        .unwrap_or_else(|| "RollingUpdate".to_string());

    let volume_claim_templates = spec
        .and_then(|s| s.volume_claim_templates.as_ref())
        .map(|vcts| {
            vcts.iter()
                .map(|pvc| {
                    let pvc_spec = pvc.spec.as_ref();
                    VolumeClaimTemplateSummary {
                        name: pvc.metadata.name.clone().unwrap_or_default(),
                        storage_class: pvc_spec.and_then(|s| s.storage_class_name.clone()),
                        access_modes: pvc_spec
                            .and_then(|s| s.access_modes.clone())
                            .unwrap_or_default(),
                        storage: pvc_spec
                            .and_then(|s| s.resources.as_ref())
                            .and_then(|r| r.requests.as_ref())
                            .and_then(|req| req.get("storage"))
                            .map(|q| q.0.clone()),
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    StatefulSetDetail {
        name: meta.name.clone().unwrap_or_default(),
        namespace: meta.namespace.clone().unwrap_or_default(),
        image: primary_image(sts),
        replicas: replica_counts(sts),
        created_at: meta.creation_timestamp.as_ref().map(|t| t.0.to_string()),
        labels: meta.labels.clone().unwrap_or_default(),
        annotations: meta.annotations.clone().unwrap_or_default(),
        conditions,
        service_name,
        pod_management_policy,
        update_strategy,
        volume_claim_templates,
    }
}

// ---------------------------------------------------------------------------
// Tool definitions
// ---------------------------------------------------------------------------

pub fn tool_definitions() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "name": "list_statefulsets",
            "description": "List statefulsets in a namespace with replica counts and status.",
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
            "name": "get_statefulset",
            "description": "Get detailed info for a single statefulset including conditions, service name, update strategy, and volume claim templates.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "StatefulSet name" }
                },
                "required": ["namespace", "name"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "create_statefulset",
            "description": "Create a new statefulset in the specified namespace.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "StatefulSet name" },
                    "image": { "type": "string", "description": "Container image" },
                    "replicas": { "type": "integer", "description": "Number of replicas (default: 1)" },
                    "service_name": { "type": "string", "description": "Governing headless service name" },
                    "port": { "type": "integer", "description": "Container port to expose (optional)" }
                },
                "required": ["namespace", "name", "image", "service_name"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "update_statefulset",
            "description": "Update (patch) an existing statefulset. Supports changing image and replica count.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "StatefulSet name" },
                    "image": { "type": "string", "description": "New container image (optional)" },
                    "replicas": { "type": "integer", "description": "New replica count (optional)" }
                },
                "required": ["namespace", "name"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "delete_statefulset",
            "description": "Delete a statefulset by name.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "StatefulSet name" }
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
        "list_statefulsets" => list_statefulsets(client, args).await,
        "get_statefulset" => get_statefulset(client, args).await,
        "create_statefulset" => create_statefulset(client, args).await,
        "update_statefulset" => update_statefulset(client, args).await,
        "delete_statefulset" => delete_statefulset(client, args).await,
        _ => return None,
    };
    Some(result)
}

// ---------------------------------------------------------------------------
// Tool implementations
// ---------------------------------------------------------------------------

async fn list_statefulsets(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let sts_api = api(client, ns)?;
    let label_selector = args["label_selector"].as_str();
    let mut lp = ListParams::default();
    if let Some(sel) = label_selector {
        lp = lp.labels(sel);
    }
    let list = sts_api.list(&lp).await.map_err(|e| e.to_string())?;

    let summaries: Vec<serde_json::Value> = list
        .iter()
        .map(|sts| {
            let s = extract_summary(sts);
            serde_json::to_value(s).unwrap_or_default()
        })
        .collect();

    serde_json::to_string_pretty(&summaries).map_err(|e| e.to_string())
}

async fn get_statefulset(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;

    let sts_api = api(client, ns)?;
    let sts = sts_api.get(name).await.map_err(|e| e.to_string())?;
    let detail = extract_detail(&sts);

    serde_json::to_string_pretty(&serde_json::to_value(detail).unwrap_or_default())
        .map_err(|e| e.to_string())
}

async fn create_statefulset(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;
    let image = args["image"].as_str().ok_or("image is required")?;
    let replicas = args["replicas"].as_i64().unwrap_or(1) as i32;
    let service_name = args["service_name"]
        .as_str()
        .ok_or("service_name is required")?;
    let port = args["port"].as_i64().map(|p| p as i32);

    let mut labels = BTreeMap::new();
    labels.insert("app".to_string(), name.to_string());
    labels.insert(
        "app.kubernetes.io/managed-by".to_string(),
        "mcp-k8s".to_string(),
    );

    let ports = port.map(|p| {
        vec![ContainerPort {
            container_port: p,
            ..Default::default()
        }]
    });

    let sts = StatefulSet {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            namespace: Some(ns.to_string()),
            labels: Some(labels.clone()),
            ..Default::default()
        },
        spec: Some(StatefulSetSpec {
            replicas: Some(replicas),
            service_name: Some(service_name.to_string()),
            selector: LabelSelector {
                match_labels: Some({
                    let mut sel = BTreeMap::new();
                    sel.insert("app".to_string(), name.to_string());
                    sel
                }),
                ..Default::default()
            },
            template: PodTemplateSpec {
                metadata: Some(ObjectMeta {
                    labels: Some({
                        let mut pod_labels = BTreeMap::new();
                        pod_labels.insert("app".to_string(), name.to_string());
                        pod_labels
                    }),
                    ..Default::default()
                }),
                spec: Some(PodSpec {
                    containers: vec![Container {
                        name: name.to_string(),
                        image: Some(image.to_string()),
                        ports,
                        ..Default::default()
                    }],
                    ..Default::default()
                }),
            },
            ..Default::default()
        }),
        ..Default::default()
    };

    let sts_api = api(client, ns)?;
    let created = sts_api
        .create(&PostParams::default(), &sts)
        .await
        .map_err(|e| e.to_string())?;

    let summary = extract_summary(&created);
    serde_json::to_string_pretty(&serde_json::to_value(summary).unwrap_or_default())
        .map_err(|e| e.to_string())
}

async fn update_statefulset(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;
    let image = args["image"].as_str();
    let replicas = args["replicas"].as_i64().map(|r| r as i32);

    if image.is_none() && replicas.is_none() {
        return Err("At least one of 'image' or 'replicas' must be provided".to_string());
    }

    let mut patch = serde_json::json!({ "spec": {} });

    if let Some(r) = replicas {
        patch["spec"]["replicas"] = serde_json::json!(r);
    }

    if let Some(img) = image {
        patch["spec"]["template"] = serde_json::json!({
            "spec": {
                "containers": [{
                    "name": name,
                    "image": img
                }]
            }
        });
    }

    let sts_api = api(client, ns)?;
    let patched = sts_api
        .patch(name, &PatchParams::apply("mcp-k8s"), &Patch::Merge(&patch))
        .await
        .map_err(|e| e.to_string())?;

    let detail = extract_detail(&patched);
    serde_json::to_string_pretty(&serde_json::to_value(detail).unwrap_or_default())
        .map_err(|e| e.to_string())
}

async fn delete_statefulset(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;

    let sts_api = api(client, ns)?;
    sts_api
        .delete(name, &DeleteParams::default())
        .await
        .map_err(|e| e.to_string())?;

    Ok(format!("StatefulSet '{name}' in namespace '{ns}' deleted"))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::api::apps::v1::{StatefulSet, StatefulSetSpec, StatefulSetStatus};
    use k8s_openapi::api::core::v1::{Container, PodSpec, PodTemplateSpec};
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::{LabelSelector, ObjectMeta};
    use std::collections::BTreeMap;

    /// Build a minimal StatefulSet for testing extraction.
    fn make_test_statefulset() -> StatefulSet {
        let mut labels = BTreeMap::new();
        labels.insert("app".to_string(), "test-sts".to_string());
        labels.insert(
            "app.kubernetes.io/managed-by".to_string(),
            "mcp-k8s".to_string(),
        );

        StatefulSet {
            metadata: ObjectMeta {
                name: Some("test-sts".to_string()),
                namespace: Some("default".to_string()),
                labels: Some(labels.clone()),
                ..Default::default()
            },
            spec: Some(StatefulSetSpec {
                replicas: Some(3),
                service_name: Some("test-svc".to_string()),
                selector: LabelSelector {
                    match_labels: Some({
                        let mut sel = BTreeMap::new();
                        sel.insert("app".to_string(), "test-sts".to_string());
                        sel
                    }),
                    ..Default::default()
                },
                template: PodTemplateSpec {
                    metadata: Some(ObjectMeta {
                        labels: Some({
                            let mut pod_labels = BTreeMap::new();
                            pod_labels.insert("app".to_string(), "test-sts".to_string());
                            pod_labels
                        }),
                        ..Default::default()
                    }),
                    spec: Some(PodSpec {
                        containers: vec![Container {
                            name: "test-sts".to_string(),
                            image: Some("nginx:latest".to_string()),
                            ..Default::default()
                        }],
                        ..Default::default()
                    }),
                },
                ..Default::default()
            }),
            status: Some(StatefulSetStatus {
                ready_replicas: Some(2),
                updated_replicas: Some(3),
                replicas: 3,
                ..Default::default()
            }),
        }
    }

    #[test]
    fn tool_definitions_returns_five_unique_tools() {
        let defs = tool_definitions();
        assert_eq!(defs.len(), 5);

        let names: Vec<&str> = defs.iter().filter_map(|d| d["name"].as_str()).collect();
        assert_eq!(names.len(), 5);

        let mut unique = names.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), 5, "tool names must be unique");
    }

    #[test]
    fn summary_serialization() {
        let summary = StatefulSetSummary {
            name: "my-sts".to_string(),
            namespace: "prod".to_string(),
            image: "redis:7".to_string(),
            replicas: StatefulSetReplicaCounts {
                desired: 3,
                ready: 3,
                updated: 3,
            },
            created_at: Some("2025-01-01T00:00:00Z".to_string()),
            labels: BTreeMap::new(),
        };

        let json = serde_json::to_string_pretty(&summary).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["name"], "my-sts");
        assert_eq!(parsed["namespace"], "prod");
        assert_eq!(parsed["image"], "redis:7");
        assert_eq!(parsed["replicas"]["desired"], 3);
        assert_eq!(parsed["replicas"]["ready"], 3);
        assert_eq!(parsed["replicas"]["updated"], 3);
        assert_eq!(parsed["created_at"], "2025-01-01T00:00:00Z");
    }

    #[test]
    fn extract_summary_from_statefulset() {
        let sts = make_test_statefulset();
        let summary = extract_summary(&sts);

        assert_eq!(summary.name, "test-sts");
        assert_eq!(summary.namespace, "default");
        assert_eq!(summary.image, "nginx:latest");
        assert_eq!(summary.replicas.desired, 3);
        assert_eq!(summary.replicas.ready, 2);
        assert_eq!(summary.replicas.updated, 3);
        assert_eq!(
            summary.labels.get("app").map(|s| s.as_str()),
            Some("test-sts")
        );
    }

    #[test]
    fn extract_detail_from_statefulset() {
        let sts = make_test_statefulset();
        let detail = extract_detail(&sts);

        assert_eq!(detail.name, "test-sts");
        assert_eq!(detail.namespace, "default");
        assert_eq!(detail.image, "nginx:latest");
        assert_eq!(detail.service_name, "test-svc");
        assert_eq!(detail.pod_management_policy, "OrderedReady");
        assert_eq!(detail.update_strategy, "RollingUpdate");
        assert!(detail.volume_claim_templates.is_empty());
        assert!(detail.conditions.is_empty());
    }
}
