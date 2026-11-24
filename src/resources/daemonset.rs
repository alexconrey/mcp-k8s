use std::collections::BTreeMap;

use k8s_openapi::api::apps::v1::{
    DaemonSet, DaemonSetSpec, DaemonSetUpdateStrategy, RollingUpdateDaemonSet,
};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{LabelSelector, ObjectMeta};
use kube::api::{DeleteParams, ListParams, PostParams};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::client::K8sClient;
use crate::policy::{PatchKind, Source};
use crate::resources::podspec::{self, WorkloadKind};

fn api(client: &K8sClient, ns: &str) -> Result<kube::Api<DaemonSet>, String> {
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
        desired_number_scheduled: status.map(|s| s.desired_number_scheduled).unwrap_or(0),
        current_number_scheduled: status.map(|s| s.current_number_scheduled).unwrap_or(0),
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
                    last_transition: c.last_transition_time.as_ref().map(|t| t.0.to_string()),
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
        desired_number_scheduled: status.map(|s| s.desired_number_scheduled).unwrap_or(0),
        current_number_scheduled: status.map(|s| s.current_number_scheduled).unwrap_or(0),
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

fn daemonset_properties() -> Value {
    json!({
        "update_strategy": {
            "type": "object",
            "description": "Update strategy",
            "properties": {
                "type": { "type": "string", "enum": ["RollingUpdate", "OnDelete"] },
                "max_unavailable": { "type": ["integer", "string"], "description": "Nodes whose pod may be unavailable during an update (default: 1)" },
                "max_surge": { "type": ["integer", "string"], "description": "Nodes that may run an extra (new) pod during an update" }
            },
            "additionalProperties": false
        },
        "min_ready_seconds": { "type": "integer" },
        "revision_history_limit": { "type": "integer" }
    })
}

fn create_daemonset_schema() -> Value {
    podspec::workload_schema(WorkloadKind::DaemonSet, daemonset_properties(), &["namespace", "name"])
}

fn update_daemonset_schema() -> Value {
    podspec::workload_update_schema(WorkloadKind::DaemonSet, daemonset_properties())
}

fn opt_i32(args: &Value, key: &str) -> Result<Option<i32>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_i64()
            .and_then(|n| i32::try_from(n).ok())
            .map(Some)
            .ok_or_else(|| format!("{key} must be a 32-bit integer")),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateStrategyArgs {
    #[serde(rename = "type")]
    type_: Option<String>,
    #[serde(alias = "maxUnavailable")]
    max_unavailable: Option<Value>,
    #[serde(alias = "maxSurge")]
    max_surge: Option<Value>,
}

fn update_strategy(args: &Value) -> Result<Option<DaemonSetUpdateStrategy>, String> {
    let Some(s) = podspec::opt::<UpdateStrategyArgs>(args, "update_strategy")? else {
        return Ok(None);
    };
    let type_ = s.type_.unwrap_or_else(|| "RollingUpdate".to_string());
    podspec::check_enum(&type_, &["RollingUpdate", "OnDelete"], "update_strategy.type")?;
    let max_unavailable = s
        .max_unavailable
        .map(|v| podspec::int_or_string(&v, "update_strategy.max_unavailable"))
        .transpose()?;
    let max_surge = s
        .max_surge
        .map(|v| podspec::int_or_string(&v, "update_strategy.max_surge"))
        .transpose()?;
    if type_ == "OnDelete" && (max_unavailable.is_some() || max_surge.is_some()) {
        return Err("update_strategy: max_unavailable and max_surge only apply to RollingUpdate".to_string());
    }
    Ok(Some(DaemonSetUpdateStrategy {
        type_: Some(type_),
        rolling_update: (max_unavailable.is_some() || max_surge.is_some()).then_some(
            RollingUpdateDaemonSet {
                max_unavailable,
                max_surge,
            },
        ),
    }))
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
            "description": format!(
                "Create a DaemonSet that runs a pod on every (or every selected) node; pods selected by app=<name>. DaemonSet settings: update_strategy (RollingUpdate with max_unavailable/max_surge, or OnDelete), min_ready_seconds, revision_history_limit, pod_annotations; use node_selector, affinity and tolerations to pick nodes. {}",
                podspec::CREATE_DESCRIPTION
            ),
            "inputSchema": create_daemonset_schema()
        }),
        serde_json::json!({
            "name": "update_daemonset",
            "description": format!(
                "Update (patch) an existing DaemonSet: update_strategy and any container or pod field. {}",
                podspec::UPDATE_DESCRIPTION
            ),
            "inputSchema": update_daemonset_schema()
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

async fn list_daemonsets(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let ds_api = api(client, ns)?;
    let label_selector = args["label_selector"].as_str();
    let mut lp = ListParams::default();
    if let Some(sel) = label_selector {
        lp = lp.labels(sel);
    }
    let list = ds_api.list(&lp).await.map_err(|e| e.to_string())?;

    let summaries: Vec<serde_json::Value> = list
        .iter()
        .map(|ds| serde_json::to_value(extract_summary(ds)).unwrap_or_default())
        .collect();

    serde_json::to_string_pretty(&summaries).map_err(|e| e.to_string())
}

async fn get_daemonset(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;
    let ds_api = api(client, ns)?;
    let ds = ds_api.get(name).await.map_err(|e| e.to_string())?;
    let detail = extract_detail(&ds);

    serde_json::to_string_pretty(&serde_json::to_value(detail).unwrap_or_default())
        .map_err(|e| e.to_string())
}

async fn create_daemonset(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let args = podspec::normalize_args(args, &create_daemonset_schema())?;
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;
    let ds_api = api(client, ns)?;

    let (meta, template) =
        podspec::build_template(&args, name, WorkloadKind::DaemonSet, client.policy())?;
    let ds = DaemonSet {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            namespace: Some(ns.to_string()),
            labels: Some(meta.labels),
            annotations: meta.annotations,
            ..Default::default()
        },
        spec: Some(DaemonSetSpec {
            selector: LabelSelector {
                match_labels: Some(meta.selector),
                ..Default::default()
            },
            template,
            update_strategy: update_strategy(&args)?,
            min_ready_seconds: opt_i32(&args, "min_ready_seconds")?,
            revision_history_limit: opt_i32(&args, "revision_history_limit")?,
        }),
        ..Default::default()
    };
    crate::policy::check_typed(client, &ds)?;

    let created = ds_api
        .create(&PostParams::default(), &ds)
        .await
        .map_err(|e| e.to_string())?;

    let summary = extract_summary(&created);
    serde_json::to_string_pretty(&serde_json::to_value(summary).unwrap_or_default())
        .map_err(|e| e.to_string())
}

async fn update_daemonset(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let args = podspec::normalize_args(args, &update_daemonset_schema())?;
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;
    let ds_api = api(client, ns)?;

    let target = if podspec::touches_container(&args) {
        let current = ds_api.get(name).await.map_err(|e| e.to_string())?;
        let pod_spec = current.spec.as_ref().and_then(|s| s.template.spec.as_ref());
        Some(podspec::resolve_container(pod_spec, args["container"].as_str(), name)?)
    } else {
        None
    };

    let mut spec = Map::new();
    if let Some(strategy) = update_strategy(&args)? {
        let mut value = serde_json::to_value(&strategy).map_err(|e| e.to_string())?;
        if strategy.type_.as_deref() == Some("OnDelete") {
            value["rollingUpdate"] = Value::Null;
        }
        spec.insert("updateStrategy".into(), value);
    }
    for (arg, field) in [
        ("min_ready_seconds", "minReadySeconds"),
        ("revision_history_limit", "revisionHistoryLimit"),
    ] {
        if let Some(v) = opt_i32(&args, arg)? {
            spec.insert(field.into(), json!(v));
        }
    }
    if let Some(template) = podspec::build_template_patch(&args, target.as_deref())? {
        spec.insert("template".into(), template);
    }
    if spec.is_empty() {
        return Err("Nothing to update: give at least one field, e.g. image".to_string());
    }

    let patched = crate::policy::patch_checked(
        client,
        &ds_api,
        name,
        json!({ "spec": spec }),
        PatchKind::Strategic,
        Source::Typed,
    )
    .await?;

    let detail = extract_detail(&patched);
    serde_json::to_string_pretty(&serde_json::to_value(detail).unwrap_or_default())
        .map_err(|e| e.to_string())
}

async fn delete_daemonset(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
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
        let names: Vec<&str> = defs.iter().filter_map(|d| d["name"].as_str()).collect();
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
