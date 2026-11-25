use std::collections::BTreeMap;

use k8s_openapi::api::batch::v1::{CronJob, CronJobSpec, JobTemplateSpec};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::api::{DeleteParams, ListParams, PostParams};
use serde::Serialize;
use serde_json::{json, Map, Value};

use crate::client::K8sClient;
use crate::extract;
use crate::policy::{PatchKind, Source};
use crate::resources::job::{job_properties, job_spec_from_args};
use crate::resources::podspec::{self, WorkloadKind};

fn api(client: &K8sClient, ns: &str) -> Result<kube::Api<CronJob>, String> {
    if !client.namespace_permitted(ns) {
        return Err(format!("Namespace '{ns}' is not in the allowed list"));
    }
    Ok(kube::Api::namespaced(client.inner().clone(), ns))
}

pub fn tool_definitions() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "name": "list_cronjobs",
            "description": "List cronjobs in a namespace with schedule, suspend status, active count, and last schedule time.",
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
            "name": "get_cronjob",
            "description": "Get detailed info for a single cronjob including labels, annotations, concurrency policy, history limits, and job template image.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "CronJob name" }
                },
                "required": ["namespace", "name"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "create_cronjob",
            "description": format!(
                "Create a Kubernetes CronJob (restart_policy defaults to OnFailure). CronJob settings: schedule, time_zone, concurrency_policy, suspend, starting_deadline_seconds, successful_jobs_history_limit, failed_jobs_history_limit; job template settings as in create_job (backoff_limit, ttl_seconds_after_finished, completions, parallelism, completion_mode, pod_failure_policy, …). {}",
                podspec::CREATE_DESCRIPTION
            ),
            "inputSchema": create_cronjob_schema()
        }),
        serde_json::json!({
            "name": "update_cronjob",
            "description": format!(
                "Update (patch) an existing CronJob: schedule, suspend, time_zone, concurrency_policy, history limits, job settings, and the pod template. {}",
                podspec::UPDATE_DESCRIPTION
            ),
            "inputSchema": update_cronjob_schema()
        }),
        serde_json::json!({
            "name": "delete_cronjob",
            "description": "Delete a Kubernetes CronJob by name.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "CronJob name" }
                },
                "required": ["namespace", "name"],
                "additionalProperties": false
            }
        }),
    ]
}

fn cronjob_properties() -> Value {
    let mut props = job_properties(false);
    let extra = json!({
        "schedule": { "type": "string", "description": "Cron schedule expression (e.g. '*/5 * * * *')" },
        "time_zone": { "type": "string", "description": "IANA time zone for the schedule (e.g. 'Europe/Berlin'; default: the controller's)" },
        "concurrency_policy": { "type": "string", "enum": ["Allow", "Forbid", "Replace"], "description": "What to do when the previous run is still active (default: Allow)" },
        "suspend": { "type": "boolean", "description": "Suspend future runs" },
        "starting_deadline_seconds": { "type": "integer", "description": "Skip a run that can't start within this many seconds of its schedule" },
        "successful_jobs_history_limit": { "type": "integer", "description": "Finished jobs to keep (default: 3)" },
        "failed_jobs_history_limit": { "type": "integer", "description": "Failed jobs to keep (default: 1)" }
    });
    if let (Some(p), Some(e)) = (props.as_object_mut(), extra.as_object()) {
        p.extend(e.clone());
    }
    props
}

fn create_cronjob_schema() -> Value {
    podspec::workload_schema(
        WorkloadKind::CronJob,
        cronjob_properties(),
        &["namespace", "name", "schedule"],
    )
}

fn update_cronjob_schema() -> Value {
    podspec::workload_update_schema(WorkloadKind::CronJob, cronjob_properties())
}

fn check_concurrency(args: &Value) -> Result<(), String> {
    if let Some(p) = args.get("concurrency_policy").and_then(Value::as_str) {
        podspec::check_enum(p, &["Allow", "Forbid", "Replace"], "concurrency_policy")?;
    }
    Ok(())
}

fn opt_i32(args: &Value, key: &str) -> Option<i32> {
    args.get(key)
        .and_then(Value::as_i64)
        .and_then(|n| i32::try_from(n).ok())
}

pub async fn handle_tool(
    client: &K8sClient,
    name: &str,
    args: &serde_json::Value,
) -> Option<Result<String, String>> {
    let result = match name {
        "list_cronjobs" => list_cronjobs(client, args).await,
        "get_cronjob" => get_cronjob(client, args).await,
        "create_cronjob" => create_cronjob(client, args).await,
        "update_cronjob" => update_cronjob(client, args).await,
        "delete_cronjob" => delete_cronjob(client, args).await,
        _ => return None,
    };
    Some(result)
}

#[derive(Serialize, Debug)]
struct CronJobDetail {
    name: String,
    namespace: String,
    schedule: String,
    suspend: bool,
    active_count: i32,
    last_schedule_time: Option<String>,
    created_at: Option<String>,
    labels: BTreeMap<String, String>,
    annotations: BTreeMap<String, String>,
    concurrency_policy: String,
    successful_jobs_history_limit: Option<i32>,
    failed_jobs_history_limit: Option<i32>,
    image: String,
}

fn cronjob_detail(cj: &CronJob) -> CronJobDetail {
    let meta = &cj.metadata;
    let spec = cj.spec.as_ref();
    let status = cj.status.as_ref();

    let concurrency_policy = spec
        .and_then(|s| s.concurrency_policy.clone())
        .unwrap_or_else(|| "Allow".to_string());

    let successful_jobs_history_limit = spec.and_then(|s| s.successful_jobs_history_limit);
    let failed_jobs_history_limit = spec.and_then(|s| s.failed_jobs_history_limit);

    let image = spec
        .and_then(|s| s.job_template.spec.as_ref())
        .and_then(|js| js.template.spec.as_ref())
        .and_then(|ps| ps.containers.first())
        .and_then(|c| c.image.clone())
        .unwrap_or_default();

    CronJobDetail {
        name: meta.name.clone().unwrap_or_default(),
        namespace: meta.namespace.clone().unwrap_or_default(),
        schedule: spec.map(|s| s.schedule.clone()).unwrap_or_default(),
        suspend: spec.and_then(|s| s.suspend).unwrap_or(false),
        active_count: status
            .and_then(|s| s.active.as_ref())
            .map(|a| a.len() as i32)
            .unwrap_or(0),
        last_schedule_time: status
            .and_then(|s| s.last_schedule_time.as_ref())
            .map(|t| t.0.to_string()),
        created_at: meta.creation_timestamp.as_ref().map(|t| t.0.to_string()),
        labels: meta.labels.clone().unwrap_or_default(),
        annotations: meta.annotations.clone().unwrap_or_default(),
        concurrency_policy,
        successful_jobs_history_limit,
        failed_jobs_history_limit,
        image,
    }
}

async fn list_cronjobs(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let cj_api = api(client, ns)?;
    let label_selector = args["label_selector"].as_str();
    let mut lp = ListParams::default();
    if let Some(sel) = label_selector {
        lp = lp.labels(sel);
    }
    let list = cj_api.list(&lp).await.map_err(|e| e.to_string())?;

    let summaries: Vec<serde_json::Value> = list
        .iter()
        .map(|cj| serde_json::to_value(extract::cronjob_summary(cj)).unwrap_or_default())
        .collect();

    serde_json::to_string_pretty(&summaries).map_err(|e| e.to_string())
}

async fn get_cronjob(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;

    let cj_api = api(client, ns)?;
    let cj = cj_api.get(name).await.map_err(|e| e.to_string())?;
    let detail = cronjob_detail(&cj);

    serde_json::to_string_pretty(&detail).map_err(|e| e.to_string())
}

async fn create_cronjob(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let args = podspec::normalize_args(args, &create_cronjob_schema())?;
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;
    let schedule = args["schedule"].as_str().ok_or("schedule is required")?;
    check_concurrency(&args)?;
    let cj_api = api(client, ns)?;

    let (meta, template) =
        podspec::build_template(&args, name, WorkloadKind::CronJob, client.policy())?;
    let cj = CronJob {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            namespace: Some(ns.to_string()),
            labels: Some(meta.labels.clone()),
            annotations: meta.annotations,
            ..Default::default()
        },
        spec: Some(CronJobSpec {
            schedule: schedule.to_string(),
            time_zone: args["time_zone"].as_str().map(str::to_string),
            concurrency_policy: args["concurrency_policy"].as_str().map(str::to_string),
            suspend: args["suspend"].as_bool(),
            starting_deadline_seconds: args["starting_deadline_seconds"].as_i64(),
            successful_jobs_history_limit: opt_i32(&args, "successful_jobs_history_limit"),
            failed_jobs_history_limit: opt_i32(&args, "failed_jobs_history_limit"),
            job_template: JobTemplateSpec {
                metadata: Some(ObjectMeta {
                    labels: Some(meta.labels),
                    ..Default::default()
                }),
                spec: Some(job_spec_from_args(&args, template, None, false)?),
            },
        }),
        ..Default::default()
    };
    crate::policy::check_typed(client, &cj)?;

    let created = cj_api
        .create(&PostParams::default(), &cj)
        .await
        .map_err(|e| e.to_string())?;

    let detail = cronjob_detail(&created);
    serde_json::to_string_pretty(&detail).map_err(|e| e.to_string())
}

async fn update_cronjob(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let args = podspec::normalize_args(args, &update_cronjob_schema())?;
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;
    check_concurrency(&args)?;
    let cj_api = api(client, ns)?;

    let target = if podspec::touches_container(&args) {
        let current = cj_api.get(name).await.map_err(|e| e.to_string())?;
        let pod_spec = current
            .spec
            .as_ref()
            .and_then(|s| s.job_template.spec.as_ref())
            .and_then(|j| j.template.spec.as_ref());
        Some(podspec::resolve_container(
            pod_spec,
            args["container"].as_str(),
            name,
        )?)
    } else {
        None
    };

    let mut spec = Map::new();
    for (arg, field) in [
        ("schedule", "schedule"),
        ("time_zone", "timeZone"),
        ("concurrency_policy", "concurrencyPolicy"),
        ("suspend", "suspend"),
        ("starting_deadline_seconds", "startingDeadlineSeconds"),
        (
            "successful_jobs_history_limit",
            "successfulJobsHistoryLimit",
        ),
        ("failed_jobs_history_limit", "failedJobsHistoryLimit"),
    ] {
        if let Some(v) = args.get(arg) {
            spec.insert(field.to_string(), v.clone());
        }
    }

    let mut job_spec = Map::new();
    let job_fields = job_spec_from_args(&args, Default::default(), None, false)?;
    if let Some(o) = serde_json::to_value(&job_fields)
        .map_err(|e| e.to_string())?
        .as_object()
    {
        for (k, v) in o {
            if k != "template" {
                job_spec.insert(k.clone(), v.clone());
            }
        }
    }
    if let Some(template) = podspec::build_template_patch(&args, target.as_deref())? {
        job_spec.insert("template".to_string(), template);
    }
    if !job_spec.is_empty() {
        spec.insert("jobTemplate".to_string(), json!({ "spec": job_spec }));
    }
    if spec.is_empty() {
        return Err("Nothing to update: give at least one CronJob, job or pod template field (e.g. schedule, suspend, image)".to_string());
    }

    let patched = crate::policy::patch_checked(
        client,
        &cj_api,
        name,
        json!({ "spec": spec }),
        PatchKind::Strategic,
        Source::Typed,
    )
    .await?;

    let detail = cronjob_detail(&patched);
    serde_json::to_string_pretty(&detail).map_err(|e| e.to_string())
}

async fn delete_cronjob(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;

    let cj_api = api(client, ns)?;
    cj_api
        .delete(name, &DeleteParams::default())
        .await
        .map_err(|e| e.to_string())?;

    Ok(format!("CronJob '{name}' deleted from namespace '{ns}'"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::api::batch::v1::{CronJobStatus, JobSpec, JobTemplateSpec};
    use k8s_openapi::api::core::v1::{Container, PodSpec, PodTemplateSpec};
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
    use std::collections::BTreeMap;

    fn make_cronjob(name: &str, namespace: &str, schedule: &str, image: &str) -> CronJob {
        let mut labels = BTreeMap::new();
        labels.insert("app".to_string(), name.to_string());

        CronJob {
            metadata: ObjectMeta {
                name: Some(name.to_string()),
                namespace: Some(namespace.to_string()),
                labels: Some(labels),
                ..Default::default()
            },
            spec: Some(CronJobSpec {
                schedule: schedule.to_string(),
                suspend: Some(false),
                concurrency_policy: Some("Forbid".to_string()),
                successful_jobs_history_limit: Some(3),
                failed_jobs_history_limit: Some(1),
                job_template: JobTemplateSpec {
                    spec: Some(JobSpec {
                        template: PodTemplateSpec {
                            spec: Some(PodSpec {
                                containers: vec![Container {
                                    name: name.to_string(),
                                    image: Some(image.to_string()),
                                    command: Some(vec![
                                        "/bin/sh".to_string(),
                                        "-c".to_string(),
                                        "echo hello".to_string(),
                                    ]),
                                    ..Default::default()
                                }],
                                restart_policy: Some("OnFailure".to_string()),
                                ..Default::default()
                            }),
                            ..Default::default()
                        },
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                ..Default::default()
            }),
            status: Some(CronJobStatus {
                active: Some(vec![]),
                last_schedule_time: None,
                ..Default::default()
            }),
        }
    }

    #[test]
    fn tool_definitions_returns_five_unique_tools() {
        let tools = tool_definitions();
        assert_eq!(tools.len(), 5);

        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();

        let mut unique_names = names.clone();
        unique_names.sort();
        unique_names.dedup();
        assert_eq!(names.len(), unique_names.len(), "tool names must be unique");

        assert!(names.contains(&"list_cronjobs"));
        assert!(names.contains(&"get_cronjob"));
        assert!(names.contains(&"create_cronjob"));
        assert!(names.contains(&"update_cronjob"));
        assert!(names.contains(&"delete_cronjob"));
    }

    #[test]
    fn extraction_from_cronjob_object() {
        let cj = make_cronjob("my-job", "default", "*/5 * * * *", "busybox:latest");

        let summary = extract::cronjob_summary(&cj);
        assert_eq!(summary.name, "my-job");
        assert_eq!(summary.namespace, "default");
        assert_eq!(summary.schedule, "*/5 * * * *");
        assert!(!summary.suspend);
        assert_eq!(summary.active_count, 0);
        assert!(summary.last_schedule_time.is_none());
    }

    #[test]
    fn cronjob_detail_serialization() {
        let cj = make_cronjob("backup", "prod", "0 2 * * *", "alpine:3.18");
        let detail = cronjob_detail(&cj);

        assert_eq!(detail.name, "backup");
        assert_eq!(detail.namespace, "prod");
        assert_eq!(detail.schedule, "0 2 * * *");
        assert!(!detail.suspend);
        assert_eq!(detail.concurrency_policy, "Forbid");
        assert_eq!(detail.successful_jobs_history_limit, Some(3));
        assert_eq!(detail.failed_jobs_history_limit, Some(1));
        assert_eq!(detail.image, "alpine:3.18");

        let json = serde_json::to_string_pretty(&detail);
        assert!(json.is_ok());

        let value: serde_json::Value = serde_json::from_str(&json.unwrap()).unwrap();
        assert_eq!(value["name"], "backup");
        assert_eq!(value["namespace"], "prod");
        assert_eq!(value["schedule"], "0 2 * * *");
        assert_eq!(value["concurrency_policy"], "Forbid");
        assert_eq!(value["successful_jobs_history_limit"], 3);
        assert_eq!(value["failed_jobs_history_limit"], 1);
        assert_eq!(value["image"], "alpine:3.18");
    }
}
