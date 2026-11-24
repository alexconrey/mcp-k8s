use std::collections::BTreeMap;

use k8s_openapi::api::apps::v1::{
    Deployment, DeploymentSpec, DeploymentStrategy, ReplicaSet, RollingUpdateDeployment,
};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{LabelSelector, ObjectMeta};
use kube::api::{DeleteParams, ListParams, Patch, PatchParams, PostParams};
use serde::Serialize;
use serde_json::{json, Map, Value};

use crate::client::K8sClient;
use crate::policy::{PatchKind, Source};
use crate::resources::podspec::{self, WorkloadKind};

fn api(client: &K8sClient, ns: &str) -> Result<kube::Api<Deployment>, String> {
    if !client.is_namespace_allowed(ns) {
        return Err(format!("Namespace '{ns}' is not in the allowed list"));
    }
    Ok(kube::Api::namespaced(client.inner().clone(), ns))
}

fn rs_api(client: &K8sClient, ns: &str) -> Result<kube::Api<ReplicaSet>, String> {
    if !client.is_namespace_allowed(ns) {
        return Err(format!("Namespace '{ns}' is not in the allowed list"));
    }
    Ok(kube::Api::namespaced(client.inner().clone(), ns))
}

// ---------------------------------------------------------------------------
// Summary type
// ---------------------------------------------------------------------------

#[derive(Serialize, Debug, Clone)]
pub struct DeploymentReplicaCounts {
    pub desired: i32,
    pub ready: i32,
    pub available: i32,
    pub updated: i32,
}

#[derive(Serialize, Debug, Clone)]
pub struct DeploymentSummary {
    pub name: String,
    pub namespace: String,
    pub image: String,
    pub replicas: DeploymentReplicaCounts,
    pub created_at: Option<String>,
    pub labels: BTreeMap<String, String>,
}

// ---------------------------------------------------------------------------
// Extraction helpers
// ---------------------------------------------------------------------------

fn primary_image(dep: &Deployment) -> String {
    dep.spec
        .as_ref()
        .and_then(|s| s.template.spec.as_ref())
        .and_then(|s| s.containers.first())
        .and_then(|c| c.image.clone())
        .unwrap_or_default()
}

fn replica_counts(dep: &Deployment) -> DeploymentReplicaCounts {
    let desired = dep.spec.as_ref().and_then(|s| s.replicas).unwrap_or(1);
    let status = dep.status.as_ref();
    DeploymentReplicaCounts {
        desired,
        ready: status.and_then(|s| s.ready_replicas).unwrap_or(0),
        available: status.and_then(|s| s.available_replicas).unwrap_or(0),
        updated: status.and_then(|s| s.updated_replicas).unwrap_or(0),
    }
}

fn extract_summary(dep: &Deployment) -> DeploymentSummary {
    let meta = &dep.metadata;
    DeploymentSummary {
        name: meta.name.clone().unwrap_or_default(),
        namespace: meta.namespace.clone().unwrap_or_default(),
        image: primary_image(dep),
        replicas: replica_counts(dep),
        created_at: meta.creation_timestamp.as_ref().map(|t| t.0.to_string()),
        labels: meta.labels.clone().unwrap_or_default(),
    }
}

fn deployment_properties() -> Value {
    json!({
        "replicas": { "type": "integer", "description": "Number of replicas (default: 1)" },
        "strategy": {
            "type": "object",
            "description": "Rollout strategy",
            "properties": {
                "type": { "type": "string", "enum": ["RollingUpdate", "Recreate"] },
                "max_surge": { "type": ["integer", "string"], "description": "Extra pods during a rolling update (number or percentage, default 25%)" },
                "max_unavailable": { "type": ["integer", "string"], "description": "Pods that may be unavailable during a rolling update (number or percentage, default 25%)" }
            },
            "additionalProperties": false
        },
        "min_ready_seconds": { "type": "integer", "description": "Seconds a new pod must be ready before it counts as available" },
        "revision_history_limit": { "type": "integer", "description": "Old ReplicaSets to keep for rollback (default: 10)" },
        "progress_deadline_seconds": { "type": "integer", "description": "Seconds before a stalled rollout is reported as failed (default: 600)" }
    })
}

fn create_deployment_schema() -> Value {
    podspec::workload_schema(
        WorkloadKind::Deployment,
        deployment_properties(),
        &["namespace", "name"],
    )
}

fn update_deployment_schema() -> Value {
    podspec::workload_update_schema(WorkloadKind::Deployment, deployment_properties())
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

fn strategy_from_args(args: &Value) -> Result<Option<DeploymentStrategy>, String> {
    let Some(s) = args.get("strategy").filter(|v| !v.is_null()) else {
        return Ok(None);
    };
    let obj = s.as_object().ok_or("strategy must be an object")?;
    if let Some(k) = obj.keys().find(|k| {
        ![
            "type",
            "max_surge",
            "max_unavailable",
            "maxSurge",
            "maxUnavailable",
        ]
        .contains(&k.as_str())
    }) {
        return Err(format!(
            "strategy: unknown field '{k}' (expected type, max_surge, max_unavailable)"
        ));
    }
    let type_ = s["type"].as_str().unwrap_or("RollingUpdate");
    podspec::check_enum(type_, &["RollingUpdate", "Recreate"], "strategy.type")?;
    let get = |snake: &str, camel: &str| {
        s.get(snake)
            .or_else(|| s.get(camel))
            .filter(|v| !v.is_null())
    };
    let max_surge = get("max_surge", "maxSurge")
        .map(|v| podspec::int_or_string(v, "strategy.max_surge"))
        .transpose()?;
    let max_unavailable = get("max_unavailable", "maxUnavailable")
        .map(|v| podspec::int_or_string(v, "strategy.max_unavailable"))
        .transpose()?;
    if type_ == "Recreate" && (max_surge.is_some() || max_unavailable.is_some()) {
        return Err(
            "strategy: max_surge and max_unavailable only apply to RollingUpdate".to_string(),
        );
    }
    Ok(Some(DeploymentStrategy {
        type_: Some(type_.to_string()),
        rolling_update: (max_surge.is_some() || max_unavailable.is_some()).then_some(
            RollingUpdateDeployment {
                max_surge,
                max_unavailable,
            },
        ),
    }))
}

/// Format the current UTC time as an RFC 3339 string using only std.
fn utc_now_rfc3339() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();

    // Decompose seconds into date/time components.
    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    // Civil date from days since epoch (1970-01-01).
    let (year, month, day) = civil_from_days(days as i64);

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, hours, minutes, seconds
    )
}

/// Convert days since 1970-01-01 to (year, month, day).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    // Algorithm from Howard Hinnant's chrono-compatible date algorithms.
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

// ---------------------------------------------------------------------------
// Tool definitions
// ---------------------------------------------------------------------------

pub fn tool_definitions() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "name": "create_deployment",
            "description": format!(
                "Create a Deployment (pods selected by app=<name>). Deployment settings: replicas, strategy (RollingUpdate with max_surge/max_unavailable, or Recreate), min_ready_seconds, revision_history_limit, progress_deadline_seconds, pod_annotations. {}",
                podspec::CREATE_DESCRIPTION
            ),
            "inputSchema": create_deployment_schema()
        }),
        serde_json::json!({
            "name": "update_deployment",
            "description": format!(
                "Update (patch) an existing Deployment: replicas, strategy, rollout settings, and any container or pod field (image, env, env_from, resources, probes, volumes, …). {}",
                podspec::UPDATE_DESCRIPTION
            ),
            "inputSchema": update_deployment_schema()
        }),
        serde_json::json!({
            "name": "delete_deployment",
            "description": "Delete a deployment by name from a namespace.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "Deployment name" }
                },
                "required": ["namespace", "name"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "restart_deployment",
            "description": "Trigger a rolling restart of a deployment by patching the pod template restart annotation.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "Deployment name" }
                },
                "required": ["namespace", "name"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "scale_deployment",
            "description": "Scale a deployment to the specified number of replicas.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "Deployment name" },
                    "replicas": { "type": "integer", "description": "Desired replica count" }
                },
                "required": ["namespace", "name", "replicas"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "rollback_deployment",
            "description": "Rollback a deployment to a specific revision by restoring the pod template from the matching ReplicaSet.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "Deployment name" },
                    "revision": { "type": "integer", "description": "Revision number to rollback to" }
                },
                "required": ["namespace", "name", "revision"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "update_deployment_labels",
            "description": "Replace pod template labels on a deployment. The supplied labels replace the entire existing set — send the full desired label map. To clear all labels, send an empty object.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "Deployment name" },
                    "labels": {
                        "type": "object",
                        "description": "Full desired set of pod template labels as key-value string pairs",
                        "additionalProperties": { "type": "string" }
                    }
                },
                "required": ["namespace", "name", "labels"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "update_deployment_annotations",
            "description": "Replace pod template annotations on a deployment. The supplied annotations replace the entire existing set — send the full desired annotation map. To clear all annotations, send an empty object.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "Deployment name" },
                    "annotations": {
                        "type": "object",
                        "description": "Full desired set of pod template annotations as key-value string pairs",
                        "additionalProperties": { "type": "string" }
                    }
                },
                "required": ["namespace", "name", "annotations"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "update_deployment_affinity",
            "description": "Set node selector and node affinity rules on a deployment's pod template. All fields are optional — omit any field to leave it unchanged. Send an empty object for node_selector to clear it. Send arrays with zero elements for required/preferred to clear node affinity.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "Deployment name" },
                    "node_selector": {
                        "type": "object",
                        "description": "Simple key=value node selector. Replaces the existing nodeSelector entirely.",
                        "additionalProperties": { "type": "string" }
                    },
                    "required_node_affinity": {
                        "type": "array",
                        "description": "requiredDuringSchedulingIgnoredDuringExecution terms. Each term is an OR; expressions within a term are AND.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "match_expressions": {
                                    "type": "array",
                                    "items": {
                                        "type": "object",
                                        "properties": {
                                            "key": { "type": "string" },
                                            "operator": { "type": "string", "enum": ["In", "NotIn", "Exists", "DoesNotExist"] },
                                            "values": { "type": "array", "items": { "type": "string" } }
                                        },
                                        "required": ["key", "operator"]
                                    }
                                }
                            },
                            "required": ["match_expressions"]
                        }
                    },
                    "preferred_node_affinity": {
                        "type": "array",
                        "description": "preferredDuringSchedulingIgnoredDuringExecution terms. Higher weight = stronger preference (1–100).",
                        "items": {
                            "type": "object",
                            "properties": {
                                "weight": { "type": "integer", "minimum": 1, "maximum": 100 },
                                "match_expressions": {
                                    "type": "array",
                                    "items": {
                                        "type": "object",
                                        "properties": {
                                            "key": { "type": "string" },
                                            "operator": { "type": "string", "enum": ["In", "NotIn", "Exists", "DoesNotExist"] },
                                            "values": { "type": "array", "items": { "type": "string" } }
                                        },
                                        "required": ["key", "operator"]
                                    }
                                }
                            },
                            "required": ["weight", "match_expressions"]
                        }
                    }
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
        "create_deployment" => create_deployment(client, args).await,
        "update_deployment" => update_deployment(client, args).await,
        "delete_deployment" => delete_deployment(client, args).await,
        "restart_deployment" => restart_deployment(client, args).await,
        "scale_deployment" => scale_deployment(client, args).await,
        "rollback_deployment" => rollback_deployment(client, args).await,
        "update_deployment_labels" => update_deployment_labels(client, args).await,
        "update_deployment_annotations" => update_deployment_annotations(client, args).await,
        "update_deployment_affinity" => update_deployment_affinity(client, args).await,
        _ => return None,
    };
    Some(result)
}

// ---------------------------------------------------------------------------
// Tool implementations
// ---------------------------------------------------------------------------

async fn create_deployment(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let args = podspec::normalize_args(args, &create_deployment_schema())?;
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;
    let dep_api = api(client, ns)?;

    let (meta, template) =
        podspec::build_template(&args, name, WorkloadKind::Deployment, client.policy())?;
    let dep = Deployment {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            namespace: Some(ns.to_string()),
            labels: Some(meta.labels),
            annotations: meta.annotations,
            ..Default::default()
        },
        spec: Some(DeploymentSpec {
            replicas: Some(opt_i32(&args, "replicas")?.unwrap_or(1)),
            selector: LabelSelector {
                match_labels: Some(meta.selector),
                ..Default::default()
            },
            template,
            strategy: strategy_from_args(&args)?,
            min_ready_seconds: opt_i32(&args, "min_ready_seconds")?,
            revision_history_limit: opt_i32(&args, "revision_history_limit")?,
            progress_deadline_seconds: opt_i32(&args, "progress_deadline_seconds")?,
            ..Default::default()
        }),
        ..Default::default()
    };
    crate::policy::check_typed(client, &dep)?;

    let created = dep_api
        .create(&PostParams::default(), &dep)
        .await
        .map_err(|e| e.to_string())?;

    let summary = extract_summary(&created);
    serde_json::to_string_pretty(&serde_json::to_value(summary).unwrap_or_default())
        .map_err(|e| e.to_string())
}

async fn update_deployment(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let args = podspec::normalize_args(args, &update_deployment_schema())?;
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;
    let dep_api = api(client, ns)?;

    let target = if podspec::touches_container(&args) {
        let current = dep_api.get(name).await.map_err(|e| e.to_string())?;
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
    if let Some(strategy) = strategy_from_args(&args)? {
        let mut value = serde_json::to_value(&strategy).map_err(|e| e.to_string())?;
        if strategy.type_.as_deref() == Some("Recreate") {
            // Recreate can't coexist with rollingUpdate parameters.
            value["rollingUpdate"] = Value::Null;
        }
        spec.insert("strategy".into(), value);
    }
    for (arg, field) in [
        ("min_ready_seconds", "minReadySeconds"),
        ("revision_history_limit", "revisionHistoryLimit"),
        ("progress_deadline_seconds", "progressDeadlineSeconds"),
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
            "Nothing to update: give at least one field, e.g. image, replicas, env, env_from, resources, \
             liveness_probe, readiness_probe, startup_probe, volumes or strategy"
                .to_string(),
        );
    }

    let patched = crate::policy::patch_checked(
        client,
        &dep_api,
        name,
        json!({ "spec": spec }),
        PatchKind::Strategic,
        Source::Typed,
    )
    .await?;

    let summary = extract_summary(&patched);
    serde_json::to_string_pretty(&serde_json::to_value(summary).unwrap_or_default())
        .map_err(|e| e.to_string())
}

async fn delete_deployment(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;

    let dep_api = api(client, ns)?;
    dep_api
        .delete(name, &DeleteParams::default())
        .await
        .map_err(|e| e.to_string())?;

    Ok(format!("Deployment '{name}' in namespace '{ns}' deleted"))
}

async fn restart_deployment(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;

    let now = utc_now_rfc3339();
    let patch = serde_json::json!({
        "spec": {
            "template": {
                "metadata": {
                    "annotations": {
                        "kubectl.kubernetes.io/restartedAt": now
                    }
                }
            }
        }
    });

    let dep_api = api(client, ns)?;
    let patched = dep_api
        .patch(name, &PatchParams::default(), &Patch::Strategic(patch))
        .await
        .map_err(|e| e.to_string())?;

    let summary = extract_summary(&patched);
    let result = serde_json::json!({
        "restarted": true,
        "restartedAt": now,
        "deployment": serde_json::to_value(summary).unwrap_or_default(),
    });
    serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
}

async fn scale_deployment(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;
    let replicas = args["replicas"].as_i64().ok_or("replicas is required")? as i32;

    let patch = serde_json::json!({
        "spec": {
            "replicas": replicas
        }
    });

    let dep_api = api(client, ns)?;
    let patched = dep_api
        .patch(name, &PatchParams::default(), &Patch::Strategic(patch))
        .await
        .map_err(|e| e.to_string())?;

    let summary = extract_summary(&patched);
    serde_json::to_string_pretty(&serde_json::to_value(summary).unwrap_or_default())
        .map_err(|e| e.to_string())
}

async fn rollback_deployment(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;
    let revision = args["revision"].as_i64().ok_or("revision is required")?;

    // Get the deployment to find its label selector.
    let dep_api = api(client, ns)?;
    let dep = dep_api.get(name).await.map_err(|e| e.to_string())?;

    let selector = dep
        .spec
        .as_ref()
        .and_then(|s| s.selector.match_labels.as_ref())
        .map(|labels| {
            labels
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join(",")
        })
        .unwrap_or_default();

    // Find the ReplicaSet with the target revision annotation.
    let rsa = rs_api(client, ns)?;
    let lp = ListParams::default().labels(&selector);
    let rs_list = rsa.list(&lp).await.map_err(|e| e.to_string())?;

    let target_rs = rs_list
        .items
        .iter()
        .find(|rs| {
            rs.metadata
                .annotations
                .as_ref()
                .and_then(|a| a.get("deployment.kubernetes.io/revision"))
                .and_then(|v| v.parse::<i64>().ok())
                == Some(revision)
        })
        .ok_or_else(|| format!("ReplicaSet with revision {revision} not found"))?;

    // Extract the pod template from the target ReplicaSet.
    let template = target_rs
        .spec
        .as_ref()
        .and_then(|s| s.template.clone())
        .ok_or("Target ReplicaSet has no pod template")?;

    // Patch the deployment with the restored pod template. An old revision
    // may predate the guardrails, so the result is policy-checked.
    let patch = serde_json::json!({
        "spec": {
            "template": serde_json::to_value(&template).map_err(|e| e.to_string())?
        }
    });

    let patched = crate::policy::patch_checked(
        client,
        &dep_api,
        name,
        patch,
        PatchKind::Strategic,
        Source::Typed,
    )
    .await?;

    let summary = extract_summary(&patched);
    let result = serde_json::json!({
        "rolledBack": true,
        "revision": revision,
        "deployment": serde_json::to_value(summary).unwrap_or_default(),
    });
    serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
}

async fn update_deployment_labels(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;
    let labels = args["labels"]
        .as_object()
        .ok_or("labels must be an object")?
        .iter()
        .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))
        .collect::<std::collections::HashMap<_, _>>();

    let patch = serde_json::json!({
        "spec": {
            "template": {
                "metadata": {
                    "labels": labels
                }
            }
        }
    });

    let dep_api = api(client, ns)?;
    let patched = crate::policy::patch_checked(
        client,
        &dep_api,
        name,
        patch,
        PatchKind::Strategic,
        Source::Typed,
    )
    .await?;

    let summary = extract_summary(&patched);
    serde_json::to_string_pretty(&serde_json::to_value(summary).unwrap_or_default())
        .map_err(|e| e.to_string())
}

async fn update_deployment_annotations(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;
    let annotations = args["annotations"]
        .as_object()
        .ok_or("annotations must be an object")?
        .iter()
        .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))
        .collect::<std::collections::HashMap<_, _>>();

    let patch = serde_json::json!({
        "spec": {
            "template": {
                "metadata": {
                    "annotations": annotations
                }
            }
        }
    });

    let dep_api = api(client, ns)?;
    let patched = crate::policy::patch_checked(
        client,
        &dep_api,
        name,
        patch,
        PatchKind::Strategic,
        Source::Typed,
    )
    .await?;

    let summary = extract_summary(&patched);
    serde_json::to_string_pretty(&serde_json::to_value(summary).unwrap_or_default())
        .map_err(|e| e.to_string())
}

async fn update_deployment_affinity(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;

    // Build the pod spec patch incrementally so that only provided fields
    // are included — omitted fields are left untouched by strategic merge.
    let mut pod_spec_patch = serde_json::Map::new();

    if let Some(ns_obj) = args.get("node_selector") {
        pod_spec_patch.insert("nodeSelector".to_string(), ns_obj.clone());
    }

    if args.get("required_node_affinity").is_some() || args.get("preferred_node_affinity").is_some()
    {
        let mut node_affinity = serde_json::Map::new();

        if let Some(required) = args.get("required_node_affinity") {
            // The input uses snake_case (match_expressions); the API needs
            // matchExpressions, or the terms are silently dropped.
            node_affinity.insert(
                "requiredDuringSchedulingIgnoredDuringExecution".to_string(),
                serde_json::json!({ "nodeSelectorTerms": podspec::camelize_keys(required) }),
            );
        }

        if let Some(preferred) = args.get("preferred_node_affinity") {
            // Convert our input shape {weight, match_expressions} →
            // k8s shape {weight, preference: {matchExpressions}}
            let terms: Vec<serde_json::Value> = preferred
                .as_array()
                .unwrap_or(&vec![])
                .iter()
                .map(|t| {
                    serde_json::json!({
                        "weight": t["weight"],
                        "preference": {
                            "matchExpressions": t["match_expressions"]
                        }
                    })
                })
                .collect();
            node_affinity.insert(
                "preferredDuringSchedulingIgnoredDuringExecution".to_string(),
                serde_json::Value::Array(terms),
            );
        }

        pod_spec_patch.insert(
            "affinity".to_string(),
            serde_json::json!({ "nodeAffinity": node_affinity }),
        );
    }

    if pod_spec_patch.is_empty() {
        return Err(
            "At least one of node_selector, required_node_affinity, or preferred_node_affinity must be provided"
                .to_string(),
        );
    }

    let patch = serde_json::json!({
        "spec": {
            "template": {
                "spec": serde_json::Value::Object(pod_spec_patch)
            }
        }
    });

    let dep_api = api(client, ns)?;
    let patched = crate::policy::patch_checked(
        client,
        &dep_api,
        name,
        patch,
        PatchKind::Strategic,
        Source::Typed,
    )
    .await?;

    let summary = extract_summary(&patched);
    serde_json::to_string_pretty(&serde_json::to_value(summary).unwrap_or_default())
        .map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::api::apps::v1::{Deployment, DeploymentSpec, DeploymentStatus};
    use k8s_openapi::api::core::v1::{Container, EnvVar, PodSpec, PodTemplateSpec};
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::{LabelSelector, ObjectMeta};
    use std::collections::BTreeMap;

    /// Build a minimal Deployment for testing extraction.
    fn make_test_deployment() -> Deployment {
        let mut labels = BTreeMap::new();
        labels.insert("app".to_string(), "test-dep".to_string());
        labels.insert(
            "app.kubernetes.io/managed-by".to_string(),
            "mcp-k8s".to_string(),
        );

        Deployment {
            metadata: ObjectMeta {
                name: Some("test-dep".to_string()),
                namespace: Some("default".to_string()),
                labels: Some(labels.clone()),
                ..Default::default()
            },
            spec: Some(DeploymentSpec {
                replicas: Some(3),
                selector: LabelSelector {
                    match_labels: Some({
                        let mut sel = BTreeMap::new();
                        sel.insert("app".to_string(), "test-dep".to_string());
                        sel
                    }),
                    ..Default::default()
                },
                template: PodTemplateSpec {
                    metadata: Some(ObjectMeta {
                        labels: Some({
                            let mut pod_labels = BTreeMap::new();
                            pod_labels.insert("app".to_string(), "test-dep".to_string());
                            pod_labels
                        }),
                        ..Default::default()
                    }),
                    spec: Some(PodSpec {
                        containers: vec![Container {
                            name: "test-dep".to_string(),
                            image: Some("nginx:latest".to_string()),
                            env: Some(vec![EnvVar {
                                name: "FOO".to_string(),
                                value: Some("bar".to_string()),
                                ..Default::default()
                            }]),
                            ..Default::default()
                        }],
                        ..Default::default()
                    }),
                },
                ..Default::default()
            }),
            status: Some(DeploymentStatus {
                ready_replicas: Some(2),
                available_replicas: Some(2),
                updated_replicas: Some(3),
                replicas: Some(3),
                ..Default::default()
            }),
        }
    }

    #[test]
    fn tool_definitions_returns_six_unique_tools() {
        let defs = tool_definitions();
        assert_eq!(defs.len(), 9);

        let names: Vec<&str> = defs.iter().filter_map(|d| d["name"].as_str()).collect();
        assert_eq!(names.len(), 9);

        let mut unique = names.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), 9, "tool names must be unique");

        assert!(names.contains(&"create_deployment"));
        assert!(names.contains(&"update_deployment"));
        assert!(names.contains(&"delete_deployment"));
        assert!(names.contains(&"restart_deployment"));
        assert!(names.contains(&"scale_deployment"));
        assert!(names.contains(&"update_deployment_labels"));
        assert!(names.contains(&"update_deployment_annotations"));
        assert!(names.contains(&"update_deployment_affinity"));
        assert!(names.contains(&"rollback_deployment"));
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
    fn extract_summary_from_deployment() {
        let dep = make_test_deployment();
        let summary = extract_summary(&dep);

        assert_eq!(summary.name, "test-dep");
        assert_eq!(summary.namespace, "default");
        assert_eq!(summary.image, "nginx:latest");
        assert_eq!(summary.replicas.desired, 3);
        assert_eq!(summary.replicas.ready, 2);
        assert_eq!(summary.replicas.available, 2);
        assert_eq!(summary.replicas.updated, 3);
        assert_eq!(
            summary.labels.get("app").map(|s| s.as_str()),
            Some("test-dep")
        );
    }

    #[test]
    fn extract_summary_from_minimal_deployment() {
        let dep = Deployment {
            metadata: ObjectMeta {
                name: Some("minimal".to_string()),
                namespace: Some("ns".to_string()),
                ..Default::default()
            },
            ..Default::default()
        };

        let summary = extract_summary(&dep);
        assert_eq!(summary.name, "minimal");
        assert_eq!(summary.namespace, "ns");
        assert_eq!(summary.image, "");
        assert_eq!(summary.replicas.desired, 1);
        assert_eq!(summary.replicas.ready, 0);
        assert_eq!(summary.replicas.available, 0);
        assert_eq!(summary.replicas.updated, 0);
        assert!(summary.labels.is_empty());
        assert!(summary.created_at.is_none());
    }

    #[test]
    fn summary_serialization() {
        let summary = DeploymentSummary {
            name: "my-dep".to_string(),
            namespace: "prod".to_string(),
            image: "redis:7".to_string(),
            replicas: DeploymentReplicaCounts {
                desired: 3,
                ready: 3,
                available: 3,
                updated: 3,
            },
            created_at: Some("2025-01-01T00:00:00Z".to_string()),
            labels: BTreeMap::new(),
        };

        let json = serde_json::to_string_pretty(&summary).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["name"], "my-dep");
        assert_eq!(parsed["namespace"], "prod");
        assert_eq!(parsed["image"], "redis:7");
        assert_eq!(parsed["replicas"]["desired"], 3);
        assert_eq!(parsed["replicas"]["ready"], 3);
        assert_eq!(parsed["replicas"]["available"], 3);
        assert_eq!(parsed["replicas"]["updated"], 3);
        assert_eq!(parsed["created_at"], "2025-01-01T00:00:00Z");
    }

    /// The probe built from `args[key]`, as the JSON sent to the API server.
    fn probe_from_args(args: &serde_json::Value, key: &str) -> Option<serde_json::Value> {
        crate::resources::podspec::probe_from_args(args, key)
            .expect("valid probe")
            .map(|p| serde_json::to_value(p).unwrap())
    }

    fn built_container(args: serde_json::Value) -> k8s_openapi::api::core::v1::Container {
        let policy = crate::policy::WorkloadPolicy::default();
        let spec = crate::resources::podspec::build_pod_spec(
            &args,
            "web",
            crate::resources::podspec::WorkloadKind::Deployment,
            &policy,
        )
        .unwrap();
        spec.containers.into_iter().next().unwrap()
    }

    #[test]
    fn env_object_form_still_parses() {
        // The original create_deployment env shape: {NAME: "value"}.
        let c = built_container(serde_json::json!({
            "image": "nginx",
            "env": { "FOO": "bar", "BAZ": "qux" }
        }));
        let env = c.env.unwrap();
        assert_eq!(env.len(), 2);
        let names: Vec<&str> = env.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"FOO"));
        assert!(names.contains(&"BAZ"));
    }

    #[test]
    fn env_absent_means_no_env() {
        let c = built_container(serde_json::json!({ "image": "nginx" }));
        assert!(c.env.is_none());
    }

    #[test]
    fn create_deployment_schema_keeps_the_original_arguments() {
        let schema = create_deployment_schema();
        let props = schema["properties"].as_object().unwrap();
        for key in [
            "namespace",
            "name",
            "image",
            "replicas",
            "port",
            "env",
            "env_from",
        ] {
            assert!(props.contains_key(key), "missing {key}");
        }
        for key in [
            "strategy",
            "min_ready_seconds",
            "revision_history_limit",
            "progress_deadline_seconds",
            "liveness_probe",
            "volumes",
            "security_context",
        ] {
            assert!(props.contains_key(key), "missing {key}");
        }
        assert_eq!(schema["required"], serde_json::json!(["namespace", "name"]));
    }

    #[test]
    fn update_deployment_schema_keeps_the_original_arguments() {
        let schema = update_deployment_schema();
        let props = schema["properties"].as_object().unwrap();
        for key in [
            "image",
            "replicas",
            "env",
            "env_from",
            "liveness_probe",
            "readiness_probe",
            "startup_probe",
            "container",
            "strategy",
        ] {
            assert!(props.contains_key(key), "missing {key}");
        }
        assert!(!props.contains_key("labels"), "labels have their own tool");
    }

    #[test]
    fn strategy_parsing() {
        let s = strategy_from_args(&serde_json::json!({
            "strategy": { "type": "RollingUpdate", "max_surge": "50%", "max_unavailable": 0 }
        }))
        .unwrap()
        .unwrap();
        let ru = s.rolling_update.unwrap();
        assert_eq!(
            ru.max_surge,
            Some(k8s_openapi::apimachinery::pkg::util::intstr::IntOrString::String("50%".into()))
        );
        assert_eq!(
            ru.max_unavailable,
            Some(k8s_openapi::apimachinery::pkg::util::intstr::IntOrString::Int(0))
        );
        let recreate =
            strategy_from_args(&serde_json::json!({ "strategy": { "type": "Recreate" } }))
                .unwrap()
                .unwrap();
        assert!(recreate.rolling_update.is_none());
        assert!(strategy_from_args(
            &serde_json::json!({ "strategy": { "type": "Recreate", "max_surge": 1 } })
        )
        .is_err());
        assert!(
            strategy_from_args(&serde_json::json!({ "strategy": { "type": "BlueGreen" } }))
                .is_err()
        );
        assert!(strategy_from_args(
            &serde_json::json!({ "strategy": { "maxSurge": 1, "bogus": 1 } })
        )
        .is_err());
    }

    #[test]
    fn utc_now_rfc3339_format() {
        let ts = utc_now_rfc3339();
        // Should match pattern YYYY-MM-DDTHH:MM:SSZ
        assert!(ts.ends_with('Z'), "timestamp should end with Z");
        assert_eq!(ts.len(), 20, "timestamp should be 20 chars");
        assert_eq!(&ts[4..5], "-");
        assert_eq!(&ts[7..8], "-");
        assert_eq!(&ts[10..11], "T");
        assert_eq!(&ts[13..14], ":");
        assert_eq!(&ts[16..17], ":");
    }

    #[test]
    fn civil_from_days_epoch() {
        let (y, m, d) = civil_from_days(0);
        assert_eq!((y, m, d), (1970, 1, 1));
    }

    #[test]
    fn civil_from_days_known_date() {
        // 2025-01-01 is day 20089 from epoch
        let (y, m, d) = civil_from_days(20089);
        assert_eq!((y, m, d), (2025, 1, 1));
    }

    #[test]
    fn primary_image_from_deployment() {
        let dep = make_test_deployment();
        assert_eq!(primary_image(&dep), "nginx:latest");
    }

    #[test]
    fn primary_image_empty_when_no_spec() {
        let dep = Deployment::default();
        assert_eq!(primary_image(&dep), "");
    }

    #[test]
    fn replica_counts_defaults() {
        let dep = Deployment::default();
        let counts = replica_counts(&dep);
        assert_eq!(counts.desired, 1);
        assert_eq!(counts.ready, 0);
        assert_eq!(counts.available, 0);
        assert_eq!(counts.updated, 0);
    }

    // -----------------------------------------------------------------------
    // probe_from_args tests
    // -----------------------------------------------------------------------

    #[test]
    fn probe_from_args_returns_none_when_key_absent() {
        let args = serde_json::json!({});
        assert!(probe_from_args(&args, "liveness_probe").is_none());
    }

    #[test]
    fn probe_from_args_http_get_minimal() {
        let args = serde_json::json!({
            "liveness_probe": {
                "probe_type": "httpGet",
                "path": "/healthz",
                "port": 8080
            }
        });
        let probe = probe_from_args(&args, "liveness_probe").expect("should parse");
        assert_eq!(probe["httpGet"]["path"], "/healthz");
        assert_eq!(probe["httpGet"]["port"], 8080);
        // Unset timing fields should be absent.
        assert!(probe.get("initialDelaySeconds").is_none());
    }

    #[test]
    fn probe_from_args_http_get_with_timing() {
        let args = serde_json::json!({
            "readiness_probe": {
                "probe_type": "httpGet",
                "path": "/ready",
                "port": 9000,
                "initial_delay_seconds": 5,
                "period_seconds": 10,
                "timeout_seconds": 2,
                "failure_threshold": 3,
                "success_threshold": 1
            }
        });
        let probe = probe_from_args(&args, "readiness_probe").expect("should parse");
        assert_eq!(probe["httpGet"]["path"], "/ready");
        assert_eq!(probe["httpGet"]["port"], 9000);
        assert_eq!(probe["initialDelaySeconds"], 5);
        assert_eq!(probe["periodSeconds"], 10);
        assert_eq!(probe["timeoutSeconds"], 2);
        assert_eq!(probe["failureThreshold"], 3);
        assert_eq!(probe["successThreshold"], 1);
    }

    #[test]
    fn probe_from_args_http_get_default_port() {
        let args = serde_json::json!({
            "liveness_probe": {
                "probe_type": "httpGet",
                "path": "/healthz"
                // no port — should default to 80
            }
        });
        let probe = probe_from_args(&args, "liveness_probe").expect("should parse");
        assert_eq!(probe["httpGet"]["port"], 80);
    }

    #[test]
    fn probe_from_args_exec() {
        let args = serde_json::json!({
            "startup_probe": {
                "probe_type": "exec",
                "command": ["cat", "/tmp/healthy"],
                "failure_threshold": 30,
                "period_seconds": 10
            }
        });
        let probe = probe_from_args(&args, "startup_probe").expect("should parse");
        assert_eq!(probe["exec"]["command"][0], "cat");
        assert_eq!(probe["exec"]["command"][1], "/tmp/healthy");
        assert_eq!(probe["failureThreshold"], 30);
        assert_eq!(probe["periodSeconds"], 10);
    }

    #[test]
    fn probe_from_args_tcp_socket() {
        let args = serde_json::json!({
            "liveness_probe": {
                "probe_type": "tcpSocket",
                "port": 5432
            }
        });
        let probe = probe_from_args(&args, "liveness_probe").expect("should parse");
        assert_eq!(probe["tcpSocket"]["port"], 5432);
        assert!(probe.get("httpGet").is_none());
        assert!(probe.get("exec").is_none());
    }

    #[test]
    fn probe_from_args_grpc() {
        // grpc used to be "unknown" and produced a probe without a handler,
        // which the API server rejects; it is now supported.
        let args = serde_json::json!({
            "liveness_probe": {
                "probe_type": "grpc",
                "port": 50051,
                "period_seconds": 5
            }
        });
        let probe = probe_from_args(&args, "liveness_probe").expect("should parse");
        assert!(probe.get("httpGet").is_none());
        assert!(probe.get("tcpSocket").is_none());
        assert!(probe.get("exec").is_none());
        assert_eq!(probe["grpc"]["port"], 50051);
        assert_eq!(probe["periodSeconds"], 5);
    }

    #[test]
    fn probe_from_args_unknown_type_is_an_error() {
        let args = serde_json::json!({ "liveness_probe": { "probe_type": "carrier-pigeon" } });
        let err = crate::resources::podspec::probe_from_args(&args, "liveness_probe").unwrap_err();
        assert!(err.contains("probe_type"), "{err}");
    }
}
