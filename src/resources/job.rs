use std::collections::BTreeMap;

use k8s_openapi::api::batch::v1::{Job, JobSpec};
use k8s_openapi::api::core::v1::{Container, PodSpec, PodTemplateSpec};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::api::{DeleteParams, ListParams, PostParams, PropagationPolicy};
use serde::Serialize;

use crate::client::K8sClient;

fn api(client: &K8sClient, ns: &str) -> Result<kube::Api<Job>, String> {
    if !client.is_namespace_allowed(ns) {
        return Err(format!("Namespace '{ns}' is not in the allowed list"));
    }
    Ok(kube::Api::namespaced(client.inner().clone(), ns))
}

#[derive(Serialize, Debug, PartialEq)]
pub struct JobSummary {
    pub name: String,
    pub namespace: String,
    pub status: String,
    pub completions: String,
    pub start_time: Option<String>,
    pub completion_time: Option<String>,
    pub duration: Option<String>,
    pub created_at: Option<String>,
    pub labels: BTreeMap<String, String>,
}

#[derive(Serialize, Debug)]
pub struct JobConditionSummary {
    pub condition_type: String,
    pub status: String,
    pub reason: Option<String>,
    pub message: Option<String>,
    pub last_transition: Option<String>,
}

#[derive(Serialize, Debug)]
pub struct JobDetail {
    pub name: String,
    pub namespace: String,
    pub status: String,
    pub completions: String,
    pub start_time: Option<String>,
    pub completion_time: Option<String>,
    pub duration: Option<String>,
    pub created_at: Option<String>,
    pub labels: BTreeMap<String, String>,
    pub annotations: BTreeMap<String, String>,
    pub conditions: Vec<JobConditionSummary>,
    pub image: String,
    pub command: Vec<String>,
    pub backoff_limit: Option<i32>,
    pub active_deadline_seconds: Option<i64>,
}

fn job_status(job: &Job) -> String {
    let status = match &job.status {
        Some(s) => s,
        None => return "Active".to_string(),
    };

    if let Some(conditions) = &status.conditions {
        for c in conditions {
            if c.type_ == "Complete" && c.status == "True" {
                return "Succeeded".to_string();
            }
            if c.type_ == "Failed" && c.status == "True" {
                return "Failed".to_string();
            }
        }
    }

    "Active".to_string()
}

fn job_completions_string(job: &Job) -> String {
    let total = job.spec.as_ref().and_then(|s| s.completions).unwrap_or(1);
    let succeeded = job.status.as_ref().and_then(|s| s.succeeded).unwrap_or(0);
    format!("{}/{}", succeeded, total)
}

fn job_duration(job: &Job) -> Option<String> {
    let status = job.status.as_ref()?;
    let start = status.start_time.as_ref()?;
    let end = status.completion_time.as_ref()?;
    let duration = end.0.duration_since(start.0);
    let secs = duration.as_secs();
    if secs < 0 {
        return None;
    }
    if secs < 60 {
        Some(format!("{}s", secs))
    } else if secs < 3600 {
        Some(format!("{}m{}s", secs / 60, secs % 60))
    } else {
        Some(format!(
            "{}h{}m{}s",
            secs / 3600,
            (secs % 3600) / 60,
            secs % 60
        ))
    }
}

fn primary_image(job: &Job) -> String {
    job.spec
        .as_ref()
        .and_then(|s| s.template.spec.as_ref())
        .and_then(|s| s.containers.first())
        .and_then(|c| c.image.clone())
        .unwrap_or_default()
}

fn primary_command(job: &Job) -> Vec<String> {
    job.spec
        .as_ref()
        .and_then(|s| s.template.spec.as_ref())
        .and_then(|s| s.containers.first())
        .and_then(|c| c.command.clone())
        .unwrap_or_default()
}

fn extract_summary(job: &Job) -> JobSummary {
    let meta = &job.metadata;
    JobSummary {
        name: meta.name.clone().unwrap_or_default(),
        namespace: meta.namespace.clone().unwrap_or_default(),
        status: job_status(job),
        completions: job_completions_string(job),
        start_time: job
            .status
            .as_ref()
            .and_then(|s| s.start_time.as_ref())
            .map(|t| t.0.to_string()),
        completion_time: job
            .status
            .as_ref()
            .and_then(|s| s.completion_time.as_ref())
            .map(|t| t.0.to_string()),
        duration: job_duration(job),
        created_at: meta.creation_timestamp.as_ref().map(|t| t.0.to_string()),
        labels: meta.labels.clone().unwrap_or_default(),
    }
}

fn extract_detail(job: &Job) -> JobDetail {
    let meta = &job.metadata;
    let spec = job.spec.as_ref();

    let conditions = job
        .status
        .as_ref()
        .and_then(|s| s.conditions.as_ref())
        .map(|conds| {
            conds
                .iter()
                .map(|c| JobConditionSummary {
                    condition_type: c.type_.clone(),
                    status: c.status.clone(),
                    reason: c.reason.clone(),
                    message: c.message.clone(),
                    last_transition: c.last_transition_time.as_ref().map(|t| t.0.to_string()),
                })
                .collect()
        })
        .unwrap_or_default();

    JobDetail {
        name: meta.name.clone().unwrap_or_default(),
        namespace: meta.namespace.clone().unwrap_or_default(),
        status: job_status(job),
        completions: job_completions_string(job),
        start_time: job
            .status
            .as_ref()
            .and_then(|s| s.start_time.as_ref())
            .map(|t| t.0.to_string()),
        completion_time: job
            .status
            .as_ref()
            .and_then(|s| s.completion_time.as_ref())
            .map(|t| t.0.to_string()),
        duration: job_duration(job),
        created_at: meta.creation_timestamp.as_ref().map(|t| t.0.to_string()),
        labels: meta.labels.clone().unwrap_or_default(),
        annotations: meta.annotations.clone().unwrap_or_default(),
        conditions,
        image: primary_image(job),
        command: primary_command(job),
        backoff_limit: spec.and_then(|s| s.backoff_limit),
        active_deadline_seconds: spec.and_then(|s| s.active_deadline_seconds),
    }
}

pub fn tool_definitions() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "name": "list_jobs",
            "description": "List jobs in a namespace. Returns name, namespace, status (Active/Succeeded/Failed), completions, start_time, completion_time, duration, and created_at.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "label_selector": { "type": "string", "description": "Label selector to filter results (e.g. app=nginx)" },
                    "field_selector": { "type": "string", "description": "Field selector to filter results (e.g. metadata.name=foo, status.successful=1)" }
                },
                "required": ["namespace"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "get_job",
            "description": "Get detailed info for a single job including conditions, image, command, backoff_limit, active_deadline_seconds, labels, and annotations.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "Job name" }
                },
                "required": ["namespace", "name"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "create_job",
            "description": "Create a Kubernetes Job. Accepts namespace, name, image, optional command, restart_policy (default Never), and backoff_limit (default 6).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "Job name" },
                    "image": { "type": "string", "description": "Container image to run" },
                    "command": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Command to run in the container (optional)"
                    },
                    "restart_policy": {
                        "type": "string",
                        "description": "Pod restart policy (default: Never)",
                        "enum": ["Never", "OnFailure"]
                    },
                    "backoff_limit": {
                        "type": "integer",
                        "description": "Number of retries before marking the job as failed (default: 6)"
                    }
                },
                "required": ["namespace", "name", "image"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "delete_job",
            "description": "Delete a job by name. Uses Foreground propagation policy to also clean up owned pods.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "Job name" }
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
        "list_jobs" => list_jobs(client, args).await,
        "get_job" => get_job(client, args).await,
        "create_job" => create_job(client, args).await,
        "delete_job" => delete_job(client, args).await,
        _ => return None,
    };
    Some(result)
}

async fn list_jobs(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let job_api = api(client, ns)?;
    let label_selector = args["label_selector"].as_str();
    let field_selector = args["field_selector"].as_str();
    let mut lp = ListParams::default();
    if let Some(sel) = label_selector {
        lp = lp.labels(sel);
    }
    if let Some(sel) = field_selector {
        lp = lp.fields(sel);
    }
    let list = job_api.list(&lp).await.map_err(|e| e.to_string())?;

    let summaries: Vec<JobSummary> = list.iter().map(extract_summary).collect();

    serde_json::to_string_pretty(&summaries).map_err(|e| e.to_string())
}

async fn get_job(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;
    let job_api = api(client, ns)?;
    let job = job_api.get(name).await.map_err(|e| e.to_string())?;

    let detail = extract_detail(&job);
    serde_json::to_string_pretty(&detail).map_err(|e| e.to_string())
}

async fn create_job(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;
    let image = args["image"].as_str().ok_or("image is required")?;
    let command: Option<Vec<String>> = args
        .get("command")
        .and_then(|v| serde_json::from_value(v.clone()).ok());
    let restart_policy = args["restart_policy"]
        .as_str()
        .unwrap_or("Never")
        .to_string();
    let backoff_limit = args["backoff_limit"].as_i64().unwrap_or(6) as i32;

    let mut labels = BTreeMap::new();
    labels.insert("app".to_string(), name.to_string());
    labels.insert(
        "app.kubernetes.io/managed-by".to_string(),
        "mcp-k8s".to_string(),
    );

    let job = Job {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            namespace: Some(ns.to_string()),
            labels: Some(labels.clone()),
            ..Default::default()
        },
        spec: Some(JobSpec {
            backoff_limit: Some(backoff_limit),
            template: PodTemplateSpec {
                metadata: Some(ObjectMeta {
                    labels: Some(labels),
                    ..Default::default()
                }),
                spec: Some(PodSpec {
                    containers: vec![Container {
                        name: name.to_string(),
                        image: Some(image.to_string()),
                        command,
                        ..Default::default()
                    }],
                    restart_policy: Some(restart_policy),
                    ..Default::default()
                }),
            },
            ..Default::default()
        }),
        ..Default::default()
    };

    let job_api = api(client, ns)?;
    let created = job_api
        .create(&PostParams::default(), &job)
        .await
        .map_err(|e| e.to_string())?;

    let summary = extract_summary(&created);
    serde_json::to_string_pretty(&summary).map_err(|e| e.to_string())
}

async fn delete_job(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;

    let job_api = api(client, ns)?;
    let dp = DeleteParams {
        propagation_policy: Some(PropagationPolicy::Foreground),
        ..Default::default()
    };
    job_api.delete(name, &dp).await.map_err(|e| e.to_string())?;

    let result = serde_json::json!({
        "deleted": name,
        "namespace": ns,
    });
    serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::api::batch::v1::JobCondition;
    use k8s_openapi::api::batch::v1::JobStatus;
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::Time;
    use k8s_openapi::jiff;

    #[test]
    fn tool_definitions_returns_four_tools() {
        let defs = tool_definitions();
        assert_eq!(defs.len(), 4);

        let names: Vec<&str> = defs.iter().map(|d| d["name"].as_str().unwrap()).collect();

        let mut unique = names.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(names.len(), unique.len(), "tool names must be unique");

        assert!(names.contains(&"list_jobs"));
        assert!(names.contains(&"get_job"));
        assert!(names.contains(&"create_job"));
        assert!(names.contains(&"delete_job"));
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
    fn job_summary_serialization() {
        let summary = JobSummary {
            name: "my-job".to_string(),
            namespace: "default".to_string(),
            status: "Succeeded".to_string(),
            completions: "1/1".to_string(),
            start_time: Some("2024-01-01T00:00:00Z".to_string()),
            completion_time: Some("2024-01-01T00:01:00Z".to_string()),
            duration: Some("1m0s".to_string()),
            created_at: Some("2024-01-01T00:00:00Z".to_string()),
            labels: BTreeMap::from([("app".to_string(), "test".to_string())]),
        };

        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["name"], "my-job");
        assert_eq!(json["namespace"], "default");
        assert_eq!(json["status"], "Succeeded");
        assert_eq!(json["completions"], "1/1");
        assert_eq!(json["start_time"], "2024-01-01T00:00:00Z");
        assert_eq!(json["completion_time"], "2024-01-01T00:01:00Z");
        assert_eq!(json["duration"], "1m0s");
        assert_eq!(json["created_at"], "2024-01-01T00:00:00Z");
        assert_eq!(json["labels"]["app"], "test");
    }

    #[test]
    fn job_summary_serialization_empty_fields() {
        let summary = JobSummary {
            name: "empty".to_string(),
            namespace: "ns".to_string(),
            status: "Active".to_string(),
            completions: "0/1".to_string(),
            start_time: None,
            completion_time: None,
            duration: None,
            created_at: None,
            labels: BTreeMap::new(),
        };

        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["name"], "empty");
        assert_eq!(json["status"], "Active");
        assert!(json["start_time"].is_null());
        assert!(json["completion_time"].is_null());
        assert!(json["duration"].is_null());
        assert!(json["created_at"].is_null());
        assert!(json["labels"].as_object().unwrap().is_empty());
    }

    #[test]
    fn extract_summary_from_succeeded_job() {
        let job = Job {
            metadata: ObjectMeta {
                name: Some("build-123".to_string()),
                namespace: Some("ci".to_string()),
                labels: Some(BTreeMap::from([("app".to_string(), "builder".to_string())])),
                ..Default::default()
            },
            spec: Some(JobSpec {
                completions: Some(1),
                template: PodTemplateSpec {
                    spec: Some(PodSpec {
                        containers: vec![Container {
                            name: "build".to_string(),
                            image: Some("alpine:latest".to_string()),
                            command: Some(vec!["echo".to_string(), "hello".to_string()]),
                            ..Default::default()
                        }],
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                ..Default::default()
            }),
            status: Some(JobStatus {
                succeeded: Some(1),
                start_time: Some(Time(jiff::Timestamp::from_second(1704067200).unwrap())),
                completion_time: Some(Time(jiff::Timestamp::from_second(1704067530).unwrap())),
                conditions: Some(vec![JobCondition {
                    type_: "Complete".to_string(),
                    status: "True".to_string(),
                    reason: Some("Job completed".to_string()),
                    message: Some("Job completed successfully".to_string()),
                    last_transition_time: Some(Time(
                        jiff::Timestamp::from_second(1704067530).unwrap(),
                    )),
                    ..Default::default()
                }]),
                ..Default::default()
            }),
        };

        let summary = extract_summary(&job);
        assert_eq!(summary.name, "build-123");
        assert_eq!(summary.namespace, "ci");
        assert_eq!(summary.status, "Succeeded");
        assert_eq!(summary.completions, "1/1");
        assert!(summary.start_time.is_some());
        assert!(summary.completion_time.is_some());
        assert_eq!(summary.duration, Some("5m30s".to_string()));
        assert_eq!(summary.labels.get("app").unwrap(), "builder");
    }

    #[test]
    fn extract_summary_from_active_job() {
        let job = Job {
            metadata: ObjectMeta {
                name: Some("running-job".to_string()),
                namespace: Some("default".to_string()),
                ..Default::default()
            },
            spec: Some(JobSpec {
                completions: Some(3),
                template: PodTemplateSpec::default(),
                ..Default::default()
            }),
            status: Some(JobStatus {
                active: Some(2),
                succeeded: Some(1),
                start_time: Some(Time(jiff::Timestamp::from_second(1718452800).unwrap())),
                ..Default::default()
            }),
        };

        let summary = extract_summary(&job);
        assert_eq!(summary.name, "running-job");
        assert_eq!(summary.status, "Active");
        assert_eq!(summary.completions, "1/3");
        assert!(summary.start_time.is_some());
        assert!(summary.completion_time.is_none());
        assert!(summary.duration.is_none());
    }

    #[test]
    fn extract_summary_from_failed_job() {
        let job = Job {
            metadata: ObjectMeta {
                name: Some("failed-job".to_string()),
                namespace: Some("default".to_string()),
                ..Default::default()
            },
            spec: Some(JobSpec {
                completions: Some(1),
                template: PodTemplateSpec::default(),
                ..Default::default()
            }),
            status: Some(JobStatus {
                failed: Some(3),
                conditions: Some(vec![JobCondition {
                    type_: "Failed".to_string(),
                    status: "True".to_string(),
                    reason: Some("BackoffLimitExceeded".to_string()),
                    message: Some("Job has reached the specified backoff limit".to_string()),
                    ..Default::default()
                }]),
                ..Default::default()
            }),
        };

        let summary = extract_summary(&job);
        assert_eq!(summary.status, "Failed");
        assert_eq!(summary.completions, "0/1");
    }

    #[test]
    fn extract_detail_includes_all_fields() {
        let job = Job {
            metadata: ObjectMeta {
                name: Some("detail-job".to_string()),
                namespace: Some("prod".to_string()),
                labels: Some(BTreeMap::from([("app".to_string(), "worker".to_string())])),
                annotations: Some(BTreeMap::from([(
                    "note".to_string(),
                    "important".to_string(),
                )])),
                ..Default::default()
            },
            spec: Some(JobSpec {
                completions: Some(1),
                backoff_limit: Some(4),
                active_deadline_seconds: Some(600),
                template: PodTemplateSpec {
                    spec: Some(PodSpec {
                        containers: vec![Container {
                            name: "worker".to_string(),
                            image: Some("myapp:v1".to_string()),
                            command: Some(vec![
                                "/bin/sh".to_string(),
                                "-c".to_string(),
                                "echo hi".to_string(),
                            ]),
                            ..Default::default()
                        }],
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                ..Default::default()
            }),
            status: Some(JobStatus {
                succeeded: Some(1),
                conditions: Some(vec![JobCondition {
                    type_: "Complete".to_string(),
                    status: "True".to_string(),
                    reason: Some("Job completed".to_string()),
                    message: None,
                    last_transition_time: Some(Time(
                        jiff::Timestamp::from_second(1709285400).unwrap(),
                    )),
                    ..Default::default()
                }]),
                ..Default::default()
            }),
        };

        let detail = extract_detail(&job);
        assert_eq!(detail.name, "detail-job");
        assert_eq!(detail.namespace, "prod");
        assert_eq!(detail.status, "Succeeded");
        assert_eq!(detail.image, "myapp:v1");
        assert_eq!(detail.command, vec!["/bin/sh", "-c", "echo hi"]);
        assert_eq!(detail.backoff_limit, Some(4));
        assert_eq!(detail.active_deadline_seconds, Some(600));
        assert_eq!(detail.labels.get("app").unwrap(), "worker");
        assert_eq!(detail.annotations.get("note").unwrap(), "important");
        assert_eq!(detail.conditions.len(), 1);
        assert_eq!(detail.conditions[0].condition_type, "Complete");
        assert_eq!(detail.conditions[0].status, "True");
    }

    #[test]
    fn extract_summary_no_spec_no_status() {
        let job = Job {
            metadata: ObjectMeta {
                name: Some("bare-job".to_string()),
                namespace: Some("default".to_string()),
                ..Default::default()
            },
            ..Default::default()
        };

        let summary = extract_summary(&job);
        assert_eq!(summary.name, "bare-job");
        assert_eq!(summary.status, "Active");
        assert_eq!(summary.completions, "0/1");
        assert!(summary.start_time.is_none());
        assert!(summary.duration.is_none());
        assert!(summary.labels.is_empty());
    }
}
