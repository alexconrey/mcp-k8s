use std::collections::BTreeMap;

use k8s_openapi::api::apps::v1::{
    RollingUpdateStatefulSetStrategy, StatefulSet, StatefulSetPersistentVolumeClaimRetentionPolicy,
    StatefulSetSpec, StatefulSetUpdateStrategy,
};
use k8s_openapi::api::core::v1::{
    PersistentVolumeClaim, PersistentVolumeClaimSpec, VolumeResourceRequirements,
};
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{LabelSelector, ObjectMeta};
use kube::api::{DeleteParams, ListParams, PostParams};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::client::K8sClient;
use crate::policy::{PatchKind, Source};
use crate::resources::podspec::{self, WorkloadKind};

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
// Arguments
// ---------------------------------------------------------------------------

fn update_strategy_schema() -> Value {
    json!({
        "type": "object",
        "description": "Update strategy",
        "properties": {
            "type": { "type": "string", "enum": ["RollingUpdate", "OnDelete"] },
            "partition": { "type": "integer", "description": "Only pods with an ordinal >= partition are updated (staged rollouts)" },
            "max_unavailable": { "type": ["integer", "string"], "description": "Pods that may be unavailable during an update" }
        },
        "additionalProperties": false
    })
}

fn retention_schema() -> Value {
    json!({
        "type": "object",
        "description": "What happens to the PVCs from volume_claim_templates when the StatefulSet is deleted or scaled down",
        "properties": {
            "when_deleted": { "type": "string", "enum": ["Retain", "Delete"] },
            "when_scaled": { "type": "string", "enum": ["Retain", "Delete"] }
        },
        "additionalProperties": false
    })
}

fn statefulset_properties(create: bool) -> Value {
    let mut props = json!({
        "replicas": { "type": "integer", "description": "Number of replicas (default: 1)" },
        "update_strategy": update_strategy_schema(),
        "min_ready_seconds": { "type": "integer" },
        "revision_history_limit": { "type": "integer" },
        "persistent_volume_claim_retention_policy": retention_schema()
    });
    if create {
        props["service_name"] =
            json!({ "type": "string", "description": "Governing headless Service name" });
        props["pod_management_policy"] = json!({ "type": "string", "enum": ["OrderedReady", "Parallel"], "description": "OrderedReady (default) starts pods one at a time; Parallel starts them together" });
        props["volume_claim_templates"] = json!({
            "type": "array",
            "description": "Per-pod PersistentVolumeClaims. Mount them by name in volume_mounts.",
            "items": {
                "type": "object",
                "properties": {
                    "name": { "type": "string" },
                    "storage": { "type": "string", "description": "Requested size, e.g. \"10Gi\"" },
                    "storage_class_name": { "type": "string" },
                    "access_modes": { "type": "array", "items": { "type": "string", "enum": ["ReadWriteOnce", "ReadOnlyMany", "ReadWriteMany", "ReadWriteOncePod"] }, "description": "Default: [ReadWriteOnce]" },
                    "volume_mode": { "type": "string", "enum": ["Filesystem", "Block"] },
                    "labels": { "type": "object", "additionalProperties": { "type": "string" } },
                    "annotations": { "type": "object", "additionalProperties": { "type": "string" } }
                },
                "required": ["name", "storage"],
                "additionalProperties": false
            }
        });
    }
    props
}

fn create_statefulset_schema() -> Value {
    podspec::workload_schema(
        WorkloadKind::StatefulSet,
        statefulset_properties(true),
        &["namespace", "name", "service_name"],
    )
}

fn update_statefulset_schema() -> Value {
    podspec::workload_update_schema(WorkloadKind::StatefulSet, statefulset_properties(false))
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
    partition: Option<i32>,
    #[serde(alias = "maxUnavailable")]
    max_unavailable: Option<Value>,
}

fn update_strategy(args: &Value) -> Result<Option<StatefulSetUpdateStrategy>, String> {
    let Some(s) = podspec::opt::<UpdateStrategyArgs>(args, "update_strategy")? else {
        return Ok(None);
    };
    let type_ = s.type_.unwrap_or_else(|| "RollingUpdate".to_string());
    podspec::check_enum(
        &type_,
        &["RollingUpdate", "OnDelete"],
        "update_strategy.type",
    )?;
    let max_unavailable = s
        .max_unavailable
        .map(|v| podspec::int_or_string(&v, "update_strategy.max_unavailable"))
        .transpose()?;
    if type_ == "OnDelete" && (s.partition.is_some() || max_unavailable.is_some()) {
        return Err(
            "update_strategy: partition and max_unavailable only apply to RollingUpdate"
                .to_string(),
        );
    }
    Ok(Some(StatefulSetUpdateStrategy {
        type_: Some(type_),
        rolling_update: (s.partition.is_some() || max_unavailable.is_some()).then_some(
            RollingUpdateStatefulSetStrategy {
                partition: s.partition,
                max_unavailable,
            },
        ),
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RetentionArgs {
    #[serde(alias = "whenDeleted")]
    when_deleted: Option<String>,
    #[serde(alias = "whenScaled")]
    when_scaled: Option<String>,
}

fn retention(
    args: &Value,
) -> Result<Option<StatefulSetPersistentVolumeClaimRetentionPolicy>, String> {
    let Some(r) = podspec::opt::<RetentionArgs>(args, "persistent_volume_claim_retention_policy")?
    else {
        return Ok(None);
    };
    for (v, what) in [
        (&r.when_deleted, "when_deleted"),
        (&r.when_scaled, "when_scaled"),
    ] {
        if let Some(v) = v {
            podspec::check_enum(v, &["Retain", "Delete"], what)?;
        }
    }
    Ok(Some(StatefulSetPersistentVolumeClaimRetentionPolicy {
        when_deleted: r.when_deleted,
        when_scaled: r.when_scaled,
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClaimTemplateArgs {
    name: String,
    storage: Value,
    #[serde(alias = "storageClassName")]
    storage_class_name: Option<String>,
    #[serde(alias = "accessModes")]
    access_modes: Option<Vec<String>>,
    #[serde(alias = "volumeMode")]
    volume_mode: Option<String>,
    labels: Option<BTreeMap<String, String>>,
    annotations: Option<BTreeMap<String, String>>,
}

fn claim_templates(args: &Value) -> Result<Option<Vec<PersistentVolumeClaim>>, String> {
    let Some(items) = podspec::opt::<Vec<ClaimTemplateArgs>>(args, "volume_claim_templates")?
    else {
        return Ok(None);
    };
    items
        .into_iter()
        .enumerate()
        .map(|(i, t)| {
            let what = format!("volume_claim_templates[{i}]");
            let access_modes = t
                .access_modes
                .unwrap_or_else(|| vec!["ReadWriteOnce".to_string()]);
            for m in &access_modes {
                podspec::check_enum(
                    m,
                    &[
                        "ReadWriteOnce",
                        "ReadOnlyMany",
                        "ReadWriteMany",
                        "ReadWriteOncePod",
                    ],
                    &format!("{what}.access_modes"),
                )?;
            }
            if let Some(m) = &t.volume_mode {
                podspec::check_enum(m, &["Filesystem", "Block"], &format!("{what}.volume_mode"))?;
            }
            let storage = podspec::scalar_string(&t.storage, &format!("{what}.storage"))?;
            Ok(PersistentVolumeClaim {
                metadata: ObjectMeta {
                    name: Some(t.name),
                    labels: t.labels,
                    annotations: t.annotations,
                    ..Default::default()
                },
                spec: Some(PersistentVolumeClaimSpec {
                    access_modes: Some(access_modes),
                    storage_class_name: t.storage_class_name,
                    volume_mode: t.volume_mode,
                    resources: Some(VolumeResourceRequirements {
                        requests: Some(BTreeMap::from([(
                            "storage".to_string(),
                            Quantity(storage),
                        )])),
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            })
        })
        .collect::<Result<Vec<_>, String>>()
        .map(Some)
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
            "description": format!(
                "Create a StatefulSet (pods selected by app=<name>). StatefulSet settings: service_name, replicas, volume_claim_templates, pod_management_policy, update_strategy (RollingUpdate with partition/max_unavailable, or OnDelete), persistent_volume_claim_retention_policy, min_ready_seconds, revision_history_limit, pod_annotations. {}",
                podspec::CREATE_DESCRIPTION
            ),
            "inputSchema": create_statefulset_schema()
        }),
        serde_json::json!({
            "name": "update_statefulset",
            "description": format!(
                "Update (patch) an existing StatefulSet: replicas, update_strategy, retention policy, and any container or pod field. {}",
                podspec::UPDATE_DESCRIPTION
            ),
            "inputSchema": update_statefulset_schema()
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
    let args = podspec::normalize_args(args, &create_statefulset_schema())?;
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;
    let service_name = args["service_name"]
        .as_str()
        .ok_or("service_name is required")?;
    if let Some(p) = args["pod_management_policy"].as_str() {
        podspec::check_enum(p, &["OrderedReady", "Parallel"], "pod_management_policy")?;
    }
    let sts_api = api(client, ns)?;

    let (meta, template) =
        podspec::build_template(&args, name, WorkloadKind::StatefulSet, client.policy())?;
    let sts = StatefulSet {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            namespace: Some(ns.to_string()),
            labels: Some(meta.labels),
            annotations: meta.annotations,
            ..Default::default()
        },
        spec: Some(StatefulSetSpec {
            replicas: Some(opt_i32(&args, "replicas")?.unwrap_or(1)),
            service_name: Some(service_name.to_string()),
            selector: LabelSelector {
                match_labels: Some(meta.selector),
                ..Default::default()
            },
            template,
            volume_claim_templates: claim_templates(&args)?,
            pod_management_policy: args["pod_management_policy"].as_str().map(str::to_string),
            update_strategy: update_strategy(&args)?,
            persistent_volume_claim_retention_policy: retention(&args)?,
            min_ready_seconds: opt_i32(&args, "min_ready_seconds")?,
            revision_history_limit: opt_i32(&args, "revision_history_limit")?,
            ..Default::default()
        }),
        ..Default::default()
    };
    crate::policy::check_typed(client, &sts)?;

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
    let args = podspec::normalize_args(args, &update_statefulset_schema())?;
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;
    let sts_api = api(client, ns)?;

    let target = if podspec::touches_container(&args) {
        let current = sts_api.get(name).await.map_err(|e| e.to_string())?;
        let pod_spec = current.spec.as_ref().and_then(|s| s.template.spec.as_ref());
        Some(podspec::resolve_container(
            pod_spec,
            args["container"].as_str(),
            name,
        )?)
    } else {
        None
    };

    let mut spec = Map::new();
    if let Some(r) = opt_i32(&args, "replicas")? {
        spec.insert("replicas".into(), json!(r));
    }
    if let Some(strategy) = update_strategy(&args)? {
        let mut value = serde_json::to_value(&strategy).map_err(|e| e.to_string())?;
        if strategy.type_.as_deref() == Some("OnDelete") {
            value["rollingUpdate"] = Value::Null;
        }
        spec.insert("updateStrategy".into(), value);
    }
    if let Some(policy) = retention(&args)? {
        spec.insert(
            "persistentVolumeClaimRetentionPolicy".into(),
            serde_json::to_value(policy).map_err(|e| e.to_string())?,
        );
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
        return Err(
            "Nothing to update: give at least one field, e.g. image or replicas".to_string(),
        );
    }

    let patched = crate::policy::patch_checked(
        client,
        &sts_api,
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
