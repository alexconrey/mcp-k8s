use std::collections::BTreeMap;

use k8s_openapi::api::batch::v1::{CronJob, Job, JobSpec, PodFailurePolicy, SuccessPolicy};
use k8s_openapi::api::core::v1::PodTemplateSpec;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{ObjectMeta, OwnerReference};
use kube::api::{DeleteParams, ListParams, PostParams, PropagationPolicy};
use serde::Serialize;
use serde_json::{json, Value};

use crate::client::K8sClient;
use crate::policy::{MANAGED_BY_LABEL, MANAGED_BY_VALUE};
use crate::resources::podspec::{self, WorkloadKind};

fn api(client: &K8sClient, ns: &str) -> Result<kube::Api<Job>, String> {
    if !client.namespace_permitted(ns) {
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
            "description": format!(
                "Create a Kubernetes Job (backoff_limit defaults to 6, restart_policy to Never). Job settings: backoff_limit, ttl_seconds_after_finished, active_deadline_seconds, completions, parallelism, completion_mode (Indexed), backoff_limit_per_index, max_failed_indexes, pod_failure_policy, success_policy, pod_replacement_policy, suspend. {}",
                podspec::CREATE_DESCRIPTION
            ),
            "inputSchema": create_job_schema()
        }),
        serde_json::json!({
            "name": "create_job_from_cronjob",
            "description": "Run a CronJob now: create a Job from its job template, like `kubectl create job --from=cronjob/NAME`. Preferred over apply_manifest. The Job is owned by the CronJob and annotated cronjob.kubernetes.io/instantiate=manual. The template is checked against the server's guardrails like any other workload.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "cronjob": { "type": "string", "description": "Name of the CronJob" },
                    "name": { "type": "string", "description": "Job name (default: <cronjob>-manual-<timestamp>)" }
                },
                "required": ["namespace", "cronjob"],
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

/// Job spec fields shared by `create_job` and (for the job template)
/// `create_cronjob`. `with_suspend` adds the Job-level `suspend`.
pub(crate) fn job_properties(with_suspend: bool) -> Value {
    let mut props = json!({
        "backoff_limit": { "type": "integer", "description": "Retries before the job is marked failed (Kubernetes default: 6)" },
        "ttl_seconds_after_finished": { "type": "integer", "description": "Delete the job this many seconds after it finishes" },
        "active_deadline_seconds": { "type": "integer", "description": "Fail the job if it runs longer than this" },
        "completions": { "type": "integer", "description": "Successful pods needed (default: 1)" },
        "parallelism": { "type": "integer", "description": "Pods running at once (default: 1)" },
        "completion_mode": { "type": "string", "enum": ["NonIndexed", "Indexed"], "description": "Indexed gives each pod a completion index (JOB_COMPLETION_INDEX)" },
        "backoff_limit_per_index": { "type": "integer", "description": "Retries per index (Indexed jobs)" },
        "max_failed_indexes": { "type": "integer", "description": "Failed indexes tolerated before the job fails (Indexed jobs, with backoff_limit_per_index)" },
        "pod_failure_policy": { "type": "object", "description": "Kubernetes PodFailurePolicy: {rules: [{action: FailJob|Ignore|Count|FailIndex, onExitCodes: {operator, values} | onPodConditions: [{type, status}]}]}; snake_case keys are accepted too" },
        "success_policy": { "type": "object", "description": "Kubernetes SuccessPolicy for Indexed jobs: {rules: [{succeededIndexes, succeededCount}]}" },
        "pod_replacement_policy": { "type": "string", "enum": ["TerminatingOrFailed", "Failed"] }
    });
    if with_suspend {
        props["suspend"] = json!({ "type": "boolean", "description": "Create the job suspended" });
    }
    props
}

fn create_job_schema() -> Value {
    podspec::workload_schema(
        WorkloadKind::Job,
        job_properties(true),
        &["namespace", "name"],
    )
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

/// Build a JobSpec from the job settings in `args` and a pod template.
///
/// `with_suspend` reads the Job-level `suspend`; CronJobs pass `false`
/// because their `suspend` argument belongs to the CronJob.
pub(crate) fn job_spec_from_args(
    args: &Value,
    template: PodTemplateSpec,
    default_backoff_limit: Option<i32>,
    with_suspend: bool,
) -> Result<JobSpec, String> {
    if let Some(mode) = args.get("completion_mode").and_then(Value::as_str) {
        podspec::check_enum(mode, &["NonIndexed", "Indexed"], "completion_mode")?;
    }
    if let Some(p) = args.get("pod_replacement_policy").and_then(Value::as_str) {
        podspec::check_enum(
            p,
            &["TerminatingOrFailed", "Failed"],
            "pod_replacement_policy",
        )?;
    }
    Ok(JobSpec {
        template,
        backoff_limit: opt_i32(args, "backoff_limit")?.or(default_backoff_limit),
        ttl_seconds_after_finished: opt_i32(args, "ttl_seconds_after_finished")?,
        active_deadline_seconds: args.get("active_deadline_seconds").and_then(Value::as_i64),
        completions: opt_i32(args, "completions")?,
        parallelism: opt_i32(args, "parallelism")?,
        completion_mode: args
            .get("completion_mode")
            .and_then(Value::as_str)
            .map(str::to_string),
        backoff_limit_per_index: opt_i32(args, "backoff_limit_per_index")?,
        max_failed_indexes: opt_i32(args, "max_failed_indexes")?,
        pod_failure_policy: podspec::passthrough::<PodFailurePolicy>(args, "pod_failure_policy")?,
        success_policy: podspec::passthrough::<SuccessPolicy>(args, "success_policy")?,
        pod_replacement_policy: args
            .get("pod_replacement_policy")
            .and_then(Value::as_str)
            .map(str::to_string),
        suspend: if with_suspend {
            args.get("suspend").and_then(Value::as_bool)
        } else {
            None
        },
        ..Default::default()
    })
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
        "create_job_from_cronjob" => create_job_from_cronjob(client, args).await,
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
    let args = podspec::normalize_args(args, &create_job_schema())?;
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;
    let job_api = api(client, ns)?;

    let (meta, template) =
        podspec::build_template(&args, name, WorkloadKind::Job, client.policy())?;
    let job = Job {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            namespace: Some(ns.to_string()),
            labels: Some(meta.labels),
            annotations: meta.annotations,
            ..Default::default()
        },
        spec: Some(job_spec_from_args(&args, template, Some(6), true)?),
        ..Default::default()
    };
    crate::policy::check_typed(client, &job)?;

    let created = job_api
        .create(&PostParams::default(), &job)
        .await
        .map_err(|e| e.to_string())?;

    let summary = extract_summary(&created);
    serde_json::to_string_pretty(&summary).map_err(|e| e.to_string())
}

/// `<cronjob>-manual-<hex unix time>`, kept within the 63-character label limit.
fn manual_job_name(cronjob: &str) -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();
    let suffix = format!("-manual-{secs:x}");
    let keep = 63usize.saturating_sub(suffix.len()).min(cronjob.len());
    format!("{}{suffix}", cronjob[..keep].trim_end_matches('-'))
}

async fn create_job_from_cronjob(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let cronjob_name = args["cronjob"].as_str().ok_or("cronjob is required")?;
    let job_api = api(client, ns)?;
    let cj_api: kube::Api<CronJob> = kube::Api::namespaced(client.inner().clone(), ns);

    let cronjob = cj_api.get(cronjob_name).await.map_err(|e| e.to_string())?;
    let template = cronjob
        .spec
        .as_ref()
        .map(|s| s.job_template.clone())
        .ok_or_else(|| format!("CronJob '{cronjob_name}' has no spec"))?;
    let name = args["name"]
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| manual_job_name(cronjob_name));

    let template_meta = template.metadata.unwrap_or_default();
    let mut labels = template_meta.labels.unwrap_or_default();
    labels.insert(MANAGED_BY_LABEL.to_string(), MANAGED_BY_VALUE.to_string());
    let mut annotations = template_meta.annotations.unwrap_or_default();
    annotations.insert(
        "cronjob.kubernetes.io/instantiate".to_string(),
        "manual".to_string(),
    );

    let job = Job {
        metadata: ObjectMeta {
            name: Some(name),
            namespace: Some(ns.to_string()),
            labels: Some(labels),
            annotations: Some(annotations),
            owner_references: cronjob.metadata.uid.clone().map(|uid| {
                vec![OwnerReference {
                    api_version: "batch/v1".to_string(),
                    kind: "CronJob".to_string(),
                    name: cronjob_name.to_string(),
                    uid,
                    controller: Some(true),
                    block_owner_deletion: Some(true),
                }]
            }),
            ..Default::default()
        },
        spec: template.spec,
        ..Default::default()
    };
    crate::policy::check_typed(client, &job)?;

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
    use k8s_openapi::api::core::v1::{Container, PodSpec};
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::Time;
    use k8s_openapi::jiff;

    #[test]
    fn tool_definitions_returns_five_tools() {
        let defs = tool_definitions();
        assert_eq!(defs.len(), 5);

        let names: Vec<&str> = defs.iter().map(|d| d["name"].as_str().unwrap()).collect();

        let mut unique = names.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(names.len(), unique.len(), "tool names must be unique");

        assert!(names.contains(&"list_jobs"));
        assert!(names.contains(&"get_job"));
        assert!(names.contains(&"create_job"));
        assert!(names.contains(&"create_job_from_cronjob"));
        assert!(names.contains(&"delete_job"));
    }

    #[test]
    fn manual_job_names_fit_the_label_limit() {
        let short = manual_job_name("nightly");
        assert!(short.starts_with("nightly-manual-"), "{short}");
        let long = manual_job_name(&"x".repeat(52));
        assert!(long.len() <= 63, "{} chars: {long}", long.len());
        assert!(long.contains("-manual-"));
    }

    #[test]
    fn job_spec_parses_every_job_setting() {
        let args = json!({
            "backoff_limit": 2,
            "ttl_seconds_after_finished": 300,
            "active_deadline_seconds": 600,
            "completions": 5,
            "parallelism": 2,
            "completion_mode": "Indexed",
            "backoff_limit_per_index": 1,
            "max_failed_indexes": 2,
            "pod_failure_policy": { "rules": [{ "action": "FailJob", "on_exit_codes": { "operator": "In", "values": [42] } }] },
            "success_policy": { "rules": [{ "succeededCount": 3 }] },
            "pod_replacement_policy": "Failed",
            "suspend": true
        });
        let spec = job_spec_from_args(&args, PodTemplateSpec::default(), Some(6), true).unwrap();
        assert_eq!(spec.backoff_limit, Some(2));
        assert_eq!(spec.ttl_seconds_after_finished, Some(300));
        assert_eq!(spec.active_deadline_seconds, Some(600));
        assert_eq!(spec.completions, Some(5));
        assert_eq!(spec.parallelism, Some(2));
        assert_eq!(spec.completion_mode.as_deref(), Some("Indexed"));
        assert_eq!(spec.backoff_limit_per_index, Some(1));
        assert_eq!(spec.max_failed_indexes, Some(2));
        let rule = &spec.pod_failure_policy.unwrap().rules[0];
        assert_eq!(rule.action, "FailJob");
        assert_eq!(rule.on_exit_codes.as_ref().unwrap().values, vec![42]);
        assert_eq!(
            spec.success_policy.unwrap().rules[0].succeeded_count,
            Some(3)
        );
        assert_eq!(spec.pod_replacement_policy.as_deref(), Some("Failed"));
        assert_eq!(spec.suspend, Some(true));
    }

    #[test]
    fn job_spec_defaults_and_validation() {
        let spec =
            job_spec_from_args(&json!({}), PodTemplateSpec::default(), Some(6), true).unwrap();
        assert_eq!(
            spec.backoff_limit,
            Some(6),
            "create_job keeps its default of 6"
        );
        assert!(spec.completion_mode.is_none());
        let err = job_spec_from_args(
            &json!({"completion_mode": "Sometimes"}),
            PodTemplateSpec::default(),
            None,
            true,
        )
        .unwrap_err();
        assert!(err.contains("completion_mode"), "{err}");
        // A CronJob's suspend belongs to the CronJob, not its job template.
        let spec = job_spec_from_args(
            &json!({"suspend": true}),
            PodTemplateSpec::default(),
            None,
            false,
        )
        .unwrap();
        assert_eq!(spec.suspend, None);
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
