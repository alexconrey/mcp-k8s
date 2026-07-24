use std::collections::BTreeMap;

use k8s_openapi::api::apps::v1::{Deployment, DeploymentSpec, ReplicaSet};
use k8s_openapi::api::core::v1::{Container, ContainerPort, EnvVar, PodSpec, PodTemplateSpec};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{LabelSelector, ObjectMeta};
use kube::api::{DeleteParams, ListParams, Patch, PatchParams, PostParams};
use serde::Serialize;

use crate::client::K8sClient;

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

/// Build env vars from an optional JSON object of key-value pairs.
fn env_from_args(args: &serde_json::Value) -> Option<Vec<EnvVar>> {
    args.get("env").and_then(|v| v.as_object()).map(|obj| {
        obj.iter()
            .map(|(k, v)| EnvVar {
                name: k.clone(),
                value: Some(v.as_str().unwrap_or_default().to_string()),
                ..Default::default()
            })
            .collect()
    })
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
            "description": "Create a new deployment in the specified namespace.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "Deployment name" },
                    "image": { "type": "string", "description": "Container image" },
                    "replicas": { "type": "integer", "description": "Number of replicas (default: 1)" },
                    "port": { "type": "integer", "description": "Container port to expose (optional)" },
                    "env": {
                        "type": "object",
                        "description": "Environment variables as key-value string pairs (optional)",
                        "additionalProperties": { "type": "string" }
                    }
                },
                "required": ["namespace", "name", "image"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "update_deployment",
            "description": "Update (merge patch) an existing deployment. Supports changing image, replicas, and env vars.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "Deployment name" },
                    "image": { "type": "string", "description": "New container image (optional)" },
                    "replicas": { "type": "integer", "description": "New replica count (optional)" },
                    "env": {
                        "type": "object",
                        "description": "Environment variables as key-value string pairs (optional, replaces all env vars)",
                        "additionalProperties": { "type": "string" }
                    }
                },
                "required": ["namespace", "name"],
                "additionalProperties": false
            }
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
        _ => return None,
    };
    Some(result)
}

// ---------------------------------------------------------------------------
// Tool implementations
// ---------------------------------------------------------------------------

async fn create_deployment(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;
    let image = args["image"].as_str().ok_or("image is required")?;
    let replicas = args["replicas"].as_i64().unwrap_or(1) as i32;
    let port = args["port"].as_i64().map(|p| p as i32);
    let env = env_from_args(args);

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

    let dep = Deployment {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            namespace: Some(ns.to_string()),
            labels: Some(labels.clone()),
            ..Default::default()
        },
        spec: Some(DeploymentSpec {
            replicas: Some(replicas),
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
                        env,
                        ..Default::default()
                    }],
                    ..Default::default()
                }),
            },
            ..Default::default()
        }),
        ..Default::default()
    };

    let dep_api = api(client, ns)?;
    let created = dep_api
        .create(&PostParams::default(), &dep)
        .await
        .map_err(|e| e.to_string())?;

    let summary = extract_summary(&created);
    serde_json::to_string_pretty(&serde_json::to_value(summary).unwrap_or_default())
        .map_err(|e| e.to_string())
}

async fn update_deployment(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;
    let image = args["image"].as_str();
    let replicas = args["replicas"].as_i64().map(|r| r as i32);
    let env = env_from_args(args);

    if image.is_none() && replicas.is_none() && env.is_none() {
        return Err("At least one of 'image', 'replicas', or 'env' must be provided".to_string());
    }

    let mut patch = serde_json::json!({ "spec": {} });

    if let Some(r) = replicas {
        patch["spec"]["replicas"] = serde_json::json!(r);
    }

    if image.is_some() || env.is_some() {
        let mut container = serde_json::json!({ "name": name });
        if let Some(img) = image {
            container["image"] = serde_json::json!(img);
        }
        if let Some(env_vars) = &env {
            let env_json: Vec<serde_json::Value> = env_vars
                .iter()
                .map(|e| {
                    serde_json::json!({
                        "name": e.name,
                        "value": e.value,
                    })
                })
                .collect();
            container["env"] = serde_json::json!(env_json);
        }
        patch["spec"]["template"] = serde_json::json!({
            "spec": {
                "containers": [container]
            }
        });
    }

    let dep_api = api(client, ns)?;
    let patched = dep_api
        .patch(name, &PatchParams::apply("mcp-k8s"), &Patch::Merge(&patch))
        .await
        .map_err(|e| e.to_string())?;

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
        .patch(name, &PatchParams::apply("mcp-k8s"), &Patch::Merge(&patch))
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
        .patch(name, &PatchParams::apply("mcp-k8s"), &Patch::Merge(&patch))
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

    // Patch the deployment with the restored pod template.
    let patch = serde_json::json!({
        "spec": {
            "template": serde_json::to_value(&template).map_err(|e| e.to_string())?
        }
    });

    let patched = dep_api
        .patch(name, &PatchParams::apply("mcp-k8s"), &Patch::Merge(&patch))
        .await
        .map_err(|e| e.to_string())?;

    let summary = extract_summary(&patched);
    let result = serde_json::json!({
        "rolledBack": true,
        "revision": revision,
        "deployment": serde_json::to_value(summary).unwrap_or_default(),
    });
    serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
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
        assert_eq!(defs.len(), 6);

        let names: Vec<&str> = defs.iter().filter_map(|d| d["name"].as_str()).collect();
        assert_eq!(names.len(), 6);

        let mut unique = names.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), 6, "tool names must be unique");

        assert!(names.contains(&"create_deployment"));
        assert!(names.contains(&"update_deployment"));
        assert!(names.contains(&"delete_deployment"));
        assert!(names.contains(&"restart_deployment"));
        assert!(names.contains(&"scale_deployment"));
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

    #[test]
    fn env_from_args_parses_object() {
        let args = serde_json::json!({
            "env": {
                "FOO": "bar",
                "BAZ": "qux"
            }
        });
        let env = env_from_args(&args).unwrap();
        assert_eq!(env.len(), 2);

        let names: Vec<&str> = env.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"FOO"));
        assert!(names.contains(&"BAZ"));
    }

    #[test]
    fn env_from_args_returns_none_when_absent() {
        let args = serde_json::json!({});
        assert!(env_from_args(&args).is_none());
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
}
