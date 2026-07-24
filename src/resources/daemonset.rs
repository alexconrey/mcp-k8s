use std::collections::BTreeMap;

use k8s_openapi::api::apps::v1::{DaemonSet, DaemonSetSpec};
use k8s_openapi::api::core::v1::{Container, PodSpec, PodTemplateSpec};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{LabelSelector, ObjectMeta};
use kube::api::{DeleteParams, ListParams, Patch, PatchParams, PostParams};
use serde::Serialize;

use crate::client::K8sClient;

fn api(
    client: &K8sClient,
    ns: &str,
) -> Result<kube::Api<DaemonSet>, String> {
    if !client.is_namespace_allowed(ns) {
        return Err(format!("Namespace '{ns}' is not in the allowed list"));
    }
    Ok(kube::Api::namespaced(client.inner().clone(), ns))
}

#[derive(Serialize, Debug, PartialEq)]
pub struct DaemonSetSummary {
    pub name: String,
    pub namespace: String,
    pub image: String,
    pub desired_number_scheduled: i32,
    pub current_number_scheduled: i32,
    pub number_ready: i32,
    pub number_available: i32,
    pub created_at: Option<String>,
    pub labels: BTreeMap<String, String>,
}

#[derive(Serialize, Debug)]
pub struct DaemonSetConditionSummary {
    pub condition_type: String,
    pub status: String,
    pub reason: Option<String>,
    pub message: Option<String>,
    pub last_transition: Option<String>,
}

#[derive(Serialize, Debug)]
pub struct TolerationSummary {
    pub key: Option<String>,
    pub operator: Option<String>,
    pub value: Option<String>,
    pub effect: Option<String>,
    pub toleration_seconds: Option<i64>,
}

#[derive(Serialize, Debug)]
pub struct DaemonSetDetail {
    pub name: String,
    pub namespace: String,
    pub image: String,
    pub desired_number_scheduled: i32,
    pub current_number_scheduled: i32,
    pub number_ready: i32,
    pub number_available: i32,
    pub created_at: Option<String>,
    pub labels: BTreeMap<String, String>,
    pub conditions: Vec<DaemonSetConditionSummary>,
    pub update_strategy: Option<String>,
    pub node_selector: BTreeMap<String, String>,
    pub tolerations: Vec<TolerationSummary>,
    pub annotations: BTreeMap<String, String>,
}

fn primary_image(ds: &DaemonSet) -> String {
    ds.spec
        .as_ref()
        .and_then(|s| s.template.spec.as_ref())
        .and_then(|s| s.containers.first())
        .and_then(|c| c.image.clone())
        .unwrap_or_default()
}

fn extract_summary(ds: &DaemonSet) -> DaemonSetSummary {
    let meta = &ds.metadata;
    let status = ds.status.as_ref();

    DaemonSetSummary {
        name: meta.name.clone().unwrap_or_default(),
        namespace: meta.namespace.clone().unwrap_or_default(),
        image: primary_image(ds),
        desired_number_scheduled: status
            .map(|s| s.desired_number_scheduled)
            .unwrap_or(0),
        current_number_scheduled: status
            .map(|s| s.current_number_scheduled)
            .unwrap_or(0),
        number_ready: status.map(|s| s.number_ready).unwrap_or(0),
        number_available: status.and_then(|s| s.number_available).unwrap_or(0),
        created_at: meta.creation_timestamp.as_ref().map(|t| t.0.to_string()),
        labels: meta.labels.clone().unwrap_or_default(),
    }
}

fn extract_detail(ds: &DaemonSet) -> DaemonSetDetail {
    let meta = &ds.metadata;
    let status = ds.status.as_ref();
    let spec = ds.spec.as_ref();

    let conditions = status
        .and_then(|s| s.conditions.as_ref())
        .map(|conds| {
            conds
                .iter()
                .map(|c| DaemonSetConditionSummary {
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

    let update_strategy = spec
        .and_then(|s| s.update_strategy.as_ref())
        .and_then(|us| us.type_.clone());

    let node_selector = spec
        .and_then(|s| s.template.spec.as_ref())
        .and_then(|ps| ps.node_selector.clone())
        .unwrap_or_default();

    let tolerations = spec
        .and_then(|s| s.template.spec.as_ref())
        .and_then(|ps| ps.tolerations.as_ref())
        .map(|tols| {
            tols.iter()
                .map(|t| TolerationSummary {
                    key: t.key.clone(),
                    operator: t.operator.clone(),
                    value: t.value.clone(),
                    effect: t.effect.clone(),
                    toleration_seconds: t.toleration_seconds,
                })
                .collect()
        })
        .unwrap_or_default();

    DaemonSetDetail {
        name: meta.name.clone().unwrap_or_default(),
        namespace: meta.namespace.clone().unwrap_or_default(),
        image: primary_image(ds),
        desired_number_scheduled: status
            .map(|s| s.desired_number_scheduled)
            .unwrap_or(0),
        current_number_scheduled: status
            .map(|s| s.current_number_scheduled)
            .unwrap_or(0),
        number_ready: status.map(|s| s.number_ready).unwrap_or(0),
        number_available: status.and_then(|s| s.number_available).unwrap_or(0),
        created_at: meta.creation_timestamp.as_ref().map(|t| t.0.to_string()),
        labels: meta.labels.clone().unwrap_or_default(),
        conditions,
        update_strategy,
        node_selector,
        tolerations,
        annotations: meta.annotations.clone().unwrap_or_default(),
    }
}

pub fn tool_definitions() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "name": "list_daemonsets",
            "description": "List daemonsets in a namespace with image, node counts, and labels.",
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
            "name": "get_daemonset",
            "description": "Get detailed info for a single daemonset including conditions, update strategy, node selector, tolerations, and annotations.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "DaemonSet name" }
                },
                "required": ["namespace", "name"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "create_daemonset",
            "description": "Create a daemonset that runs a pod on all (or selected) nodes.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "DaemonSet name" },
                    "image": { "type": "string", "description": "Container image" },
                    "port": { "type": "integer", "description": "Container port (optional)" }
                },
                "required": ["namespace", "name", "image"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "update_daemonset",
            "description": "Patch a daemonset. Supports updating the container image.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "DaemonSet name" },
                    "image": { "type": "string", "description": "New container image (optional)" }
                },
                "required": ["namespace", "name"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "delete_daemonset",
            "description": "Delete a daemonset by name.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "DaemonSet name" }
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
        "list_daemonsets" => list_daemonsets(client, args).await,
        "get_daemonset" => get_daemonset(client, args).await,
        "create_daemonset" => create_daemonset(client, args).await,
        "update_daemonset" => update_daemonset(client, args).await,
        "delete_daemonset" => delete_daemonset(client, args).await,
        _ => return None,
    };
    Some(result)
}

async fn list_daemonsets(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let ds_api = api(client, ns)?;
    let label_selector = args["label_selector"].as_str();
    let mut lp = ListParams::default();
    if let Some(sel) = label_selector {
        lp = lp.labels(sel);
    }
    let list = ds_api
        .list(&lp)
        .await
        .map_err(|e| e.to_string())?;

    let summaries: Vec<serde_json::Value> = list
        .iter()
        .map(|ds| serde_json::to_value(extract_summary(ds)).unwrap_or_default())
        .collect();

    serde_json::to_string_pretty(&summaries).map_err(|e| e.to_string())
}

async fn get_daemonset(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;
    let ds_api = api(client, ns)?;
    let ds = ds_api.get(name).await.map_err(|e| e.to_string())?;
    let detail = extract_detail(&ds);

    serde_json::to_string_pretty(&serde_json::to_value(detail).unwrap_or_default())
        .map_err(|e| e.to_string())
}

async fn create_daemonset(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;
    let image = args["image"].as_str().ok_or("image is required")?;
    let port = args["port"].as_i64();

    let mut labels = BTreeMap::new();
    labels.insert("app".to_string(), name.to_string());
    labels.insert("app.kubernetes.io/managed-by".to_string(), "mcp-k8s".to_string());

    let mut match_labels = BTreeMap::new();
    match_labels.insert("app".to_string(), name.to_string());

    let mut container = Container {
        name: name.to_string(),
        image: Some(image.to_string()),
        ..Default::default()
    };

    if let Some(p) = port {
        use k8s_openapi::api::core::v1::ContainerPort;
        container.ports = Some(vec![ContainerPort {
            container_port: p as i32,
            ..Default::default()
        }]);
    }

    let ds = DaemonSet {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            namespace: Some(ns.to_string()),
            labels: Some(labels.clone()),
            ..Default::default()
        },
        spec: Some(DaemonSetSpec {
            selector: LabelSelector {
                match_labels: Some(match_labels),
                ..Default::default()
            },
            template: PodTemplateSpec {
                metadata: Some(ObjectMeta {
                    labels: Some(labels),
                    ..Default::default()
                }),
                spec: Some(PodSpec {
                    containers: vec![container],
                    ..Default::default()
                }),
            },
            ..Default::default()
        }),
        ..Default::default()
    };

    let ds_api = api(client, ns)?;
    let created = ds_api
        .create(&PostParams::default(), &ds)
        .await
        .map_err(|e| e.to_string())?;

    let summary = extract_summary(&created);
    serde_json::to_string_pretty(&serde_json::to_value(summary).unwrap_or_default())
        .map_err(|e| e.to_string())
}

async fn update_daemonset(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;
    let image = args["image"].as_str();

    let mut patch = serde_json::json!({});

    if let Some(img) = image {
        patch = serde_json::json!({
            "spec": {
                "template": {
                    "spec": {
                        "containers": [{
                            "name": name,
                            "image": img
                        }]
                    }
                }
            }
        });
    }

    let ds_api = api(client, ns)?;
    let patched = ds_api
        .patch(
            name,
            &PatchParams::apply("mcp-k8s"),
            &Patch::Merge(&patch),
        )
        .await
        .map_err(|e| e.to_string())?;

    let detail = extract_detail(&patched);
    serde_json::to_string_pretty(&serde_json::to_value(detail).unwrap_or_default())
        .map_err(|e| e.to_string())
}

async fn delete_daemonset(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;
    let ds_api = api(client, ns)?;

    ds_api
        .delete(name, &DeleteParams::default())
        .await
        .map_err(|e| e.to_string())?;

    Ok(format!("DaemonSet '{name}' in namespace '{ns}' deleted"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::api::apps::v1::{DaemonSetSpec, DaemonSetStatus};
    use k8s_openapi::api::core::v1::{Container, PodSpec, PodTemplateSpec};
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
    use std::collections::BTreeMap;

    fn make_test_daemonset() -> DaemonSet {
        let mut labels = BTreeMap::new();
        labels.insert("app".to_string(), "test-ds".to_string());

        DaemonSet {
            metadata: ObjectMeta {
                name: Some("test-ds".to_string()),
                namespace: Some("default".to_string()),
                labels: Some(labels),
                ..Default::default()
            },
            spec: Some(DaemonSetSpec {
                template: PodTemplateSpec {
                    metadata: None,
                    spec: Some(PodSpec {
                        containers: vec![Container {
                            name: "test-container".to_string(),
                            image: Some("nginx:latest".to_string()),
                            ..Default::default()
                        }],
                        ..Default::default()
                    }),
                },
                selector: Default::default(),
                ..Default::default()
            }),
            status: Some(DaemonSetStatus {
                desired_number_scheduled: 3,
                current_number_scheduled: 3,
                number_ready: 2,
                number_available: Some(2),
                number_misscheduled: 0,
                ..Default::default()
            }),
        }
    }

    #[test]
    fn test_tool_definitions_returns_five_tools() {
        let defs = tool_definitions();
        assert_eq!(defs.len(), 5);
    }

    #[test]
    fn test_tool_definitions_unique_names() {
        let defs = tool_definitions();
        let names: Vec<&str> = defs
            .iter()
            .filter_map(|d| d["name"].as_str())
            .collect();
        assert_eq!(names.len(), 5);
        let mut unique = names.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), 5);
    }

    #[test]
    fn test_summary_serialization() {
        let summary = DaemonSetSummary {
            name: "my-ds".to_string(),
            namespace: "kube-system".to_string(),
            image: "fluentd:v1.0".to_string(),
            desired_number_scheduled: 5,
            current_number_scheduled: 5,
            number_ready: 4,
            number_available: 4,
            created_at: Some("2024-01-01T00:00:00Z".to_string()),
            labels: BTreeMap::new(),
        };

        let json = serde_json::to_string_pretty(&summary).expect("serialization should succeed");
        let parsed: serde_json::Value =
            serde_json::from_str(&json).expect("deserialization should succeed");

        assert_eq!(parsed["name"], "my-ds");
        assert_eq!(parsed["namespace"], "kube-system");
        assert_eq!(parsed["image"], "fluentd:v1.0");
        assert_eq!(parsed["desired_number_scheduled"], 5);
        assert_eq!(parsed["current_number_scheduled"], 5);
        assert_eq!(parsed["number_ready"], 4);
        assert_eq!(parsed["number_available"], 4);
        assert_eq!(parsed["created_at"], "2024-01-01T00:00:00Z");
    }

    #[test]
    fn test_extract_summary_from_daemonset() {
        let ds = make_test_daemonset();
        let summary = extract_summary(&ds);

        assert_eq!(summary.name, "test-ds");
        assert_eq!(summary.namespace, "default");
        assert_eq!(summary.image, "nginx:latest");
        assert_eq!(summary.desired_number_scheduled, 3);
        assert_eq!(summary.current_number_scheduled, 3);
        assert_eq!(summary.number_ready, 2);
        assert_eq!(summary.number_available, 2);
        assert_eq!(
            summary.labels.get("app").map(|s| s.as_str()),
            Some("test-ds")
        );
    }

    #[test]
    fn test_extract_detail_from_daemonset() {
        let ds = make_test_daemonset();
        let detail = extract_detail(&ds);

        assert_eq!(detail.name, "test-ds");
        assert_eq!(detail.namespace, "default");
        assert_eq!(detail.image, "nginx:latest");
        assert_eq!(detail.desired_number_scheduled, 3);
        assert_eq!(detail.number_ready, 2);
        assert!(detail.conditions.is_empty());
        assert!(detail.node_selector.is_empty());
        assert!(detail.tolerations.is_empty());
    }

    #[test]
    fn test_extract_summary_no_status() {
        let ds = DaemonSet {
            metadata: ObjectMeta {
                name: Some("no-status".to_string()),
                namespace: Some("test".to_string()),
                ..Default::default()
            },
            spec: None,
            status: None,
        };
        let summary = extract_summary(&ds);

        assert_eq!(summary.name, "no-status");
        assert_eq!(summary.desired_number_scheduled, 0);
        assert_eq!(summary.current_number_scheduled, 0);
        assert_eq!(summary.number_ready, 0);
        assert_eq!(summary.number_available, 0);
        assert_eq!(summary.image, "");
    }
}
