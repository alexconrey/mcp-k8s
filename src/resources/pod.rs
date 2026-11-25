use std::collections::BTreeMap;

use k8s_openapi::api::core::v1::Pod;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::api::{AttachParams, DeleteParams, EvictParams, ListParams, PostParams};
use serde::Serialize;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::time::{Duration, Instant};

use crate::client::K8sClient;
use crate::extract::pod_summary;
use crate::resources::podspec::{self, WorkloadKind};
use crate::types::PodSummary;

fn api(client: &K8sClient, ns: &str) -> Result<kube::Api<Pod>, String> {
    if !client.is_namespace_allowed(ns) {
        return Err(format!("Namespace '{ns}' is not in the allowed list"));
    }
    Ok(kube::Api::namespaced(client.inner().clone(), ns))
}

#[derive(Serialize, Debug)]
pub struct PodDetail {
    #[serde(flatten)]
    pub summary: PodSummary,
    pub namespace: String,
    pub volumes: Vec<String>,
    pub service_account: Option<String>,
    pub node_selector: BTreeMap<String, String>,
    pub tolerations: Vec<TolerationSummary>,
    pub annotations: BTreeMap<String, String>,
}

#[derive(Serialize, Debug)]
pub struct TolerationSummary {
    pub key: Option<String>,
    pub operator: Option<String>,
    pub value: Option<String>,
    pub effect: Option<String>,
    pub toleration_seconds: Option<i64>,
}

fn extract_detail(pod: &Pod) -> PodDetail {
    let summary = pod_summary(pod);
    let meta = &pod.metadata;
    let spec = pod.spec.as_ref();

    let volumes = spec
        .map(|s| {
            s.volumes
                .as_ref()
                .map(|vols| vols.iter().map(|v| v.name.clone()).collect())
                .unwrap_or_default()
        })
        .unwrap_or_default();

    let service_account = spec.and_then(|s| s.service_account_name.clone());

    let node_selector = spec
        .and_then(|s| s.node_selector.clone())
        .unwrap_or_default();

    let tolerations = spec
        .and_then(|s| s.tolerations.as_ref())
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

    let annotations = meta.annotations.clone().unwrap_or_default();

    PodDetail {
        summary,
        namespace: meta.namespace.clone().unwrap_or_default(),
        volumes,
        service_account,
        node_selector,
        tolerations,
        annotations,
    }
}

pub fn tool_definitions() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "name": "list_pods",
            "description": "List pods in a namespace. Returns name, phase, readiness, restart count, node, and container statuses. Optionally filter by label selector.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "label_selector": { "type": "string", "description": "Label selector to filter pods (e.g. app=nginx)" },
                    "field_selector": { "type": "string", "description": "Field selector to filter results (e.g. metadata.name=foo, status.phase=Running, spec.nodeName=node1)" }
                },
                "required": ["namespace"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "get_pod",
            "description": "Get a pod by name. Returns detailed information including phase, containers, volumes, service account, node selector, tolerations, and annotations.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "Pod name" }
                },
                "required": ["namespace", "name"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "create_pod",
            "description": format!(
                "Create a standalone pod: debugging, one-shot tasks, or a quick container (restart_policy defaults to Never). {}",
                podspec::CREATE_DESCRIPTION
            ),
            "inputSchema": create_pod_schema()
        }),
        serde_json::json!({
            "name": "delete_pod",
            "description": "Delete a pod by name from a namespace. Optionally specify a grace period in seconds.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "Pod name" },
                    "grace_period_seconds": { "type": "integer", "description": "Grace period in seconds before force-killing the pod" }
                },
                "required": ["namespace", "name"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "evict_pod",
            "description": "Evict a pod respecting PodDisruptionBudgets. Safer than delete for production pods.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "Pod name" },
                    "grace_period_seconds": { "type": "integer", "description": "Grace period in seconds (optional)" }
                },
                "required": ["namespace", "name"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "exec_pod",
            "description": "Execute a command in a pod container and return its output. Not for long-running commands: it stops after timeout_secs (default 60, max 300) and returns the output collected so far with timed_out: true. Each of stdout and stderr is capped at 1 MiB (truncated with a marker). Refused for pods with node-level access (privileged containers, host namespaces, hostPath volumes, added capabilities, unconfined profiles) unless the operator allows those settings.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "Pod name" },
                    "command": { "type": "array", "items": { "type": "string" }, "description": "Command to execute (e.g. [\"ls\", \"-la\"])" },
                    "container": { "type": "string", "description": "Container name (optional, defaults to first)" },
                    "timeout_secs": { "type": "integer", "description": "Seconds to wait for the command to finish (default 60, max 300)" }
                },
                "required": ["namespace", "name", "command"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "port_forward_check",
            "description": "Check if a port on a pod is reachable by establishing and immediately closing a port-forward tunnel. Gives up after 10 seconds.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "Pod name" },
                    "port": { "type": "integer", "description": "Container port to check" }
                },
                "required": ["namespace", "name", "port"],
                "additionalProperties": false
            }
        }),
    ]
}

fn create_pod_schema() -> serde_json::Value {
    podspec::workload_schema(
        WorkloadKind::Pod,
        serde_json::json!({}),
        &["namespace", "name"],
    )
}

pub async fn handle_tool(
    client: &K8sClient,
    name: &str,
    args: &serde_json::Value,
) -> Option<Result<String, String>> {
    let result = match name {
        "list_pods" => list_pods(client, args).await,
        "get_pod" => get_pod(client, args).await,
        "create_pod" => create_pod(client, args).await,
        "delete_pod" => delete_pod(client, args).await,
        "evict_pod" => evict_pod(client, args).await,
        "exec_pod" => exec_pod(client, args).await,
        "port_forward_check" => port_forward_check(client, args).await,
        _ => return None,
    };
    Some(result)
}

async fn list_pods(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let pod_api = api(client, ns)?;

    let mut lp = ListParams::default();
    if let Some(sel) = args["label_selector"].as_str() {
        lp = lp.labels(sel);
    }
    if let Some(sel) = args["field_selector"].as_str() {
        lp = lp.fields(sel);
    }

    let list = pod_api.list(&lp).await.map_err(|e| e.to_string())?;

    let summaries: Vec<serde_json::Value> = list
        .iter()
        .map(|pod| serde_json::to_value(pod_summary(pod)).unwrap_or_default())
        .collect();

    serde_json::to_string_pretty(&summaries).map_err(|e| e.to_string())
}

async fn get_pod(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;
    let pod_api = api(client, ns)?;
    let pod = pod_api.get(name).await.map_err(|e| e.to_string())?;

    let detail = extract_detail(&pod);
    serde_json::to_string_pretty(&detail).map_err(|e| e.to_string())
}

async fn create_pod(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let args = podspec::normalize_args(args, &create_pod_schema())?;
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;
    let pod_api = api(client, ns)?;

    let (meta, template) =
        podspec::build_template(&args, name, WorkloadKind::Pod, client.policy())?;
    let pod = Pod {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            namespace: Some(ns.to_string()),
            labels: Some(meta.labels),
            annotations: meta.annotations,
            ..Default::default()
        },
        spec: template.spec,
        ..Default::default()
    };
    crate::policy::check_typed(client, &pod)?;

    let created = pod_api
        .create(&PostParams::default(), &pod)
        .await
        .map_err(|e| e.to_string())?;

    let result = serde_json::json!({
        "name": created.metadata.name,
        "namespace": ns,
        "phase": created.status.as_ref().and_then(|s| s.phase.clone()).unwrap_or_else(|| "Pending".to_string()),
    });
    serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
}

async fn delete_pod(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;

    let mut dp = DeleteParams::default();
    if let Some(grace) = args["grace_period_seconds"].as_i64() {
        dp = dp.grace_period(grace as u32);
    }

    let pod_api = api(client, ns)?;
    pod_api.delete(name, &dp).await.map_err(|e| e.to_string())?;

    let result = serde_json::json!({
        "deleted": name,
        "namespace": ns,
    });
    serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
}

async fn evict_pod(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;

    let mut ep = EvictParams::default();
    if let Some(grace) = args["grace_period_seconds"].as_i64() {
        let dp = DeleteParams::default().grace_period(grace as u32);
        ep.delete_options = Some(dp);
    }

    let pod_api = api(client, ns)?;
    pod_api.evict(name, &ep).await.map_err(|e| e.to_string())?;

    let result = serde_json::json!({
        "evicted": name,
        "namespace": ns,
    });
    serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
}

/// Default and maximum `timeout_secs` for `exec_pod`.
const EXEC_DEFAULT_TIMEOUT_SECS: u64 = 60;
const EXEC_MAX_TIMEOUT_SECS: u64 = 300;
/// Per-stream output cap for `exec_pod`.
const EXEC_OUTPUT_CAP: usize = 1024 * 1024;
/// How long `port_forward_check` waits for the tunnel.
const PORT_FORWARD_TIMEOUT_SECS: u64 = 10;

/// Workload policy settings whose violation means a pod has node-level
/// access. `exec_pod` refuses such pods: a shell in a privileged or
/// host-namespace container is a shell on the node. Other rules (registries,
/// volume types other than hostPath, host ports, privilege escalation) are
/// about what may be created, not about what an exec session can reach.
const EXEC_NODE_ACCESS_SETTINGS: &[&str] = &[
    crate::policy::flags::ALLOW_PRIVILEGED.field,
    crate::policy::flags::ALLOW_HOST_NAMESPACES.field,
    crate::policy::flags::ALLOW_HOST_PATH.field,
    crate::policy::flags::ALLOW_CAPABILITIES.field,
    crate::policy::flags::ALLOW_UNCONFINED.field,
];

/// The workload policy's pod rules that make `exec_pod` refuse `pod`. Empty
/// when the pod has no node-level access, or the operator allows it.
pub(crate) fn exec_violations(
    policy: &crate::policy::WorkloadPolicy,
    pod: &Pod,
) -> Vec<crate::policy::Violation> {
    let value = serde_json::to_value(pod).unwrap_or_default();
    let mut out = Vec::new();
    if let Some(spec) = value.get("spec") {
        crate::policy::check_pod_spec(policy, spec, value.get("metadata"), "spec", &mut out);
    }
    // A probe or lifecycle hook aimed at another host falls under
    // allow_unconfined too, but it's not access from inside the pod.
    out.retain(|v| {
        EXEC_NODE_ACCESS_SETTINGS.contains(&v.flag().field)
            && !v.location.contains("Probe")
            && !v.location.contains(".lifecycle.")
    });
    out
}

/// Output collected from one stream, capped at [`EXEC_OUTPUT_CAP`].
#[derive(Default)]
struct CappedOutput {
    data: Vec<u8>,
    truncated: bool,
}

impl CappedOutput {
    fn push(&mut self, chunk: &[u8]) {
        let room = EXEC_OUTPUT_CAP.saturating_sub(self.data.len());
        if chunk.len() > room {
            self.truncated = true;
        }
        self.data.extend_from_slice(&chunk[..chunk.len().min(room)]);
    }

    fn into_string(self) -> String {
        let mut s = String::from_utf8_lossy(&self.data).into_owned();
        if self.truncated {
            s.push_str("\n[output truncated at 1 MiB]");
        }
        s
    }
}

async fn read_some<R: AsyncRead + Unpin>(
    reader: &mut Option<R>,
    buf: &mut [u8],
) -> std::io::Result<usize> {
    match reader {
        Some(r) => r.read(buf).await,
        None => std::future::pending().await,
    }
}

/// Read both streams concurrently until each ends or `deadline` passes.
/// Reading them one after the other can deadlock: the exec streams have
/// small buffers, so a command that fills stderr blocks before closing
/// stdout. Output past the cap is read and discarded so the command isn't
/// blocked. Returns `(stdout, stderr, timed_out)`.
async fn collect_output<O, E>(
    mut stdout: Option<O>,
    mut stderr: Option<E>,
    deadline: Instant,
) -> (CappedOutput, CappedOutput, bool)
where
    O: AsyncRead + Unpin,
    E: AsyncRead + Unpin,
{
    let mut out = CappedOutput::default();
    let mut err = CappedOutput::default();
    let mut out_buf = vec![0u8; 8192];
    let mut err_buf = vec![0u8; 8192];
    let sleep = tokio::time::sleep_until(deadline);
    tokio::pin!(sleep);
    while stdout.is_some() || stderr.is_some() {
        tokio::select! {
            _ = &mut sleep => return (out, err, true),
            r = read_some(&mut stdout, &mut out_buf) => match r {
                Ok(0) | Err(_) => stdout = None,
                Ok(n) => out.push(&out_buf[..n]),
            },
            r = read_some(&mut stderr, &mut err_buf) => match r {
                Ok(0) | Err(_) => stderr = None,
                Ok(n) => err.push(&err_buf[..n]),
            },
        }
    }
    (out, err, false)
}

async fn exec_pod(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;
    let command: Vec<String> = serde_json::from_value(args["command"].clone())
        .map_err(|e| format!("invalid command: {e}"))?;
    let container = args["container"].as_str().map(|s| s.to_string());
    let timeout_secs = match args.get("timeout_secs") {
        None | Some(serde_json::Value::Null) => EXEC_DEFAULT_TIMEOUT_SECS,
        Some(v) => v
            .as_u64()
            .filter(|n| *n > 0)
            .ok_or("timeout_secs must be a positive integer")?
            .min(EXEC_MAX_TIMEOUT_SECS),
    };

    let pods_api = api(client, ns)?;

    // Exec into a pod with node-level access is node-level access: hold the
    // pod to the same workload policy rules create_pod would.
    let pod = pods_api.get(name).await.map_err(|e| e.to_string())?;
    let violations = exec_violations(client.policy(), &pod);
    if let Err(e) = crate::policy::into_result(violations) {
        return Err(format!(
            "exec_pod refused: this pod has node-level access, so a command in it runs with node-level access.\n{e}"
        ));
    }

    let ap = AttachParams {
        stdout: true,
        stderr: true,
        stdin: false,
        tty: false,
        container,
        ..AttachParams::default()
    };

    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    let mut process = tokio::time::timeout_at(deadline, pods_api.exec(name, command, &ap))
        .await
        .map_err(|_| format!("exec_pod timed out after {timeout_secs}s connecting to the pod"))?
        .map_err(|e| e.to_string())?;

    let status = process.take_status();
    let (stdout, stderr, mut timed_out) =
        collect_output(process.stdout(), process.stderr(), deadline).await;
    let status = match status {
        Some(status) if !timed_out => match tokio::time::timeout_at(deadline, status).await {
            Ok(status) => status,
            Err(_) => {
                timed_out = true;
                None
            }
        },
        _ => None,
    };
    if timed_out {
        // Close the exec session. Without a TTY, whether the container
        // runtime then kills the command is up to the runtime.
        process.abort();
    }
    let exit_code = status.as_ref().and_then(|s| {
        s.status
            .as_ref()
            .and_then(|st| if st == "Success" { Some(0i32) } else { None })
    });

    let mut result = serde_json::json!({
        "stdout": stdout.into_string(),
        "stderr": stderr.into_string(),
        "exit_code": exit_code,
        "timed_out": timed_out,
    });
    if timed_out {
        result["note"] = format!(
            "TIMED OUT: the command did not finish within {timeout_secs}s; this is the output collected so far, and the exec session was closed"
        )
        .into();
    }
    serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
}

async fn port_forward_check(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;
    let port: u16 = args["port"]
        .as_i64()
        .ok_or("port is required")?
        .try_into()
        .map_err(|_| "port must be a valid u16 (0-65535)".to_string())?;

    let pods_api = api(client, ns)?;

    // Attempt to establish a port-forward tunnel, without waiting forever.
    let mut pf = tokio::time::timeout(
        Duration::from_secs(PORT_FORWARD_TIMEOUT_SECS),
        pods_api.portforward(name, &[port]),
    )
    .await
    .map_err(|_| format!("port_forward_check timed out after {PORT_FORWARD_TIMEOUT_SECS}s"))?
    .map_err(|e| e.to_string())?;

    // Take the stream to verify the connection was established
    let stream = pf.take_stream(port);
    let reachable = stream.is_some();

    // Close the tunnel immediately
    drop(stream);
    pf.abort();
    drop(pf);

    let result = serde_json::json!({
        "pod": name,
        "namespace": ns,
        "port": port,
        "reachable": reachable,
    });
    serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::api::core::v1::{
        Container, ContainerState, ContainerStateRunning, ContainerStatus, EmptyDirVolumeSource,
        PodCondition, PodSpec, PodStatus, Toleration, Volume,
    };
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

    #[test]
    fn tool_definitions_returns_seven_tools() {
        let defs = tool_definitions();
        assert_eq!(defs.len(), 7);

        let names: Vec<&str> = defs.iter().map(|d| d["name"].as_str().unwrap()).collect();

        let mut unique = names.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(names.len(), unique.len(), "tool names must be unique");

        assert!(names.contains(&"list_pods"));
        assert!(names.contains(&"get_pod"));
        assert!(names.contains(&"create_pod"));
        assert!(names.contains(&"delete_pod"));
        assert!(names.contains(&"evict_pod"));
        assert!(names.contains(&"exec_pod"));
        assert!(names.contains(&"port_forward_check"));
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
    fn pod_detail_serialization() {
        let detail = PodDetail {
            summary: PodSummary {
                name: "my-pod".to_string(),
                phase: "Running".to_string(),
                ready: true,
                restart_count: 0,
                node: Some("node-1".to_string()),
                started_at: Some("2024-01-01T00:00:00Z".to_string()),
                conditions: vec![],
                container_statuses: vec![],
                oom_killed: false,
            },
            namespace: "default".to_string(),
            volumes: vec!["data-vol".to_string(), "config-vol".to_string()],
            service_account: Some("my-sa".to_string()),
            node_selector: BTreeMap::from([("disktype".to_string(), "ssd".to_string())]),
            tolerations: vec![TolerationSummary {
                key: Some("key1".to_string()),
                operator: Some("Equal".to_string()),
                value: Some("value1".to_string()),
                effect: Some("NoSchedule".to_string()),
                toleration_seconds: None,
            }],
            annotations: BTreeMap::from([("note".to_string(), "test".to_string())]),
        };

        let json = serde_json::to_value(&detail).unwrap();
        // Flattened summary fields
        assert_eq!(json["name"], "my-pod");
        assert_eq!(json["phase"], "Running");
        assert_eq!(json["ready"], true);
        assert_eq!(json["node"], "node-1");
        // Detail-specific fields
        assert_eq!(json["namespace"], "default");
        assert_eq!(json["volumes"].as_array().unwrap().len(), 2);
        assert_eq!(json["service_account"], "my-sa");
        assert_eq!(json["node_selector"]["disktype"], "ssd");
        assert_eq!(json["tolerations"].as_array().unwrap().len(), 1);
        assert_eq!(json["tolerations"][0]["key"], "key1");
        assert_eq!(json["annotations"]["note"], "test");
    }

    #[test]
    fn pod_detail_serialization_empty_fields() {
        let detail = PodDetail {
            summary: PodSummary {
                name: "bare-pod".to_string(),
                phase: "Pending".to_string(),
                ready: false,
                restart_count: 0,
                node: None,
                started_at: None,
                conditions: vec![],
                container_statuses: vec![],
                oom_killed: false,
            },
            namespace: "ns".to_string(),
            volumes: vec![],
            service_account: None,
            node_selector: BTreeMap::new(),
            tolerations: vec![],
            annotations: BTreeMap::new(),
        };

        let json = serde_json::to_value(&detail).unwrap();
        assert_eq!(json["name"], "bare-pod");
        assert!(json["node"].is_null());
        assert!(json["service_account"].is_null());
        assert!(json["volumes"].as_array().unwrap().is_empty());
        assert!(json["node_selector"].as_object().unwrap().is_empty());
        assert!(json["tolerations"].as_array().unwrap().is_empty());
        assert!(json["annotations"].as_object().unwrap().is_empty());
    }

    #[test]
    fn extract_detail_from_constructed_pod() {
        let pod = Pod {
            metadata: ObjectMeta {
                name: Some("test-pod".to_string()),
                namespace: Some("prod".to_string()),
                annotations: Some(BTreeMap::from([(
                    "kubectl.kubernetes.io/last-applied".to_string(),
                    "{}".to_string(),
                )])),
                ..Default::default()
            },
            spec: Some(PodSpec {
                containers: vec![Container {
                    name: "app".to_string(),
                    image: Some("nginx:1.25".to_string()),
                    ..Default::default()
                }],
                service_account_name: Some("my-service-account".to_string()),
                node_name: Some("worker-1".to_string()),
                node_selector: Some(BTreeMap::from([(
                    "zone".to_string(),
                    "us-east-1a".to_string(),
                )])),
                volumes: Some(vec![Volume {
                    name: "cache-volume".to_string(),
                    empty_dir: Some(EmptyDirVolumeSource::default()),
                    ..Default::default()
                }]),
                tolerations: Some(vec![
                    Toleration {
                        key: Some("dedicated".to_string()),
                        operator: Some("Equal".to_string()),
                        value: Some("gpu".to_string()),
                        effect: Some("NoSchedule".to_string()),
                        toleration_seconds: None,
                    },
                    Toleration {
                        key: Some("node.kubernetes.io/not-ready".to_string()),
                        operator: Some("Exists".to_string()),
                        effect: Some("NoExecute".to_string()),
                        toleration_seconds: Some(300),
                        ..Default::default()
                    },
                ]),
                ..Default::default()
            }),
            status: Some(PodStatus {
                phase: Some("Running".to_string()),
                start_time: None,
                container_statuses: Some(vec![ContainerStatus {
                    name: "app".to_string(),
                    ready: true,
                    restart_count: 2,
                    image: "nginx:1.25".to_string(),
                    image_id: "sha256:abc".to_string(),
                    state: Some(ContainerState {
                        running: Some(ContainerStateRunning { started_at: None }),
                        ..Default::default()
                    }),
                    ..Default::default()
                }]),
                conditions: Some(vec![PodCondition {
                    type_: "Ready".to_string(),
                    status: "True".to_string(),
                    ..Default::default()
                }]),
                ..Default::default()
            }),
        };

        let detail = extract_detail(&pod);

        // Summary fields
        assert_eq!(detail.summary.name, "test-pod");
        assert_eq!(detail.summary.phase, "Running");
        assert!(detail.summary.ready);
        assert_eq!(detail.summary.restart_count, 2);
        assert_eq!(detail.summary.node, Some("worker-1".to_string()));
        assert!(!detail.summary.oom_killed);
        assert_eq!(detail.summary.container_statuses.len(), 1);
        assert_eq!(detail.summary.container_statuses[0].name, "app");
        assert_eq!(detail.summary.container_statuses[0].image, "nginx:1.25");

        // Detail fields
        assert_eq!(detail.namespace, "prod");
        assert_eq!(detail.volumes, vec!["cache-volume".to_string()]);
        assert_eq!(
            detail.service_account,
            Some("my-service-account".to_string())
        );
        assert_eq!(detail.node_selector.get("zone").unwrap(), "us-east-1a");
        assert_eq!(detail.tolerations.len(), 2);
        assert_eq!(detail.tolerations[0].key, Some("dedicated".to_string()));
        assert_eq!(detail.tolerations[0].effect, Some("NoSchedule".to_string()));
        assert_eq!(detail.tolerations[1].toleration_seconds, Some(300));
        assert!(detail
            .annotations
            .contains_key("kubectl.kubernetes.io/last-applied"));
    }

    #[test]
    fn extract_detail_from_minimal_pod() {
        let pod = Pod {
            metadata: ObjectMeta {
                name: Some("minimal".to_string()),
                namespace: Some("default".to_string()),
                ..Default::default()
            },
            spec: Some(PodSpec {
                containers: vec![Container {
                    name: "main".to_string(),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        };

        let detail = extract_detail(&pod);
        assert_eq!(detail.summary.name, "minimal");
        assert_eq!(detail.summary.phase, "Unknown");
        assert!(!detail.summary.ready);
        assert_eq!(detail.namespace, "default");
        assert!(detail.volumes.is_empty());
        assert!(detail.service_account.is_none());
        assert!(detail.node_selector.is_empty());
        assert!(detail.tolerations.is_empty());
        assert!(detail.annotations.is_empty());
    }

    // -- exec_pod: output collection and the node-access check ---------------

    use tokio::io::AsyncWriteExt;

    fn far_deadline() -> Instant {
        Instant::now() + Duration::from_secs(30)
    }

    #[tokio::test]
    async fn collect_output_reads_both_streams_concurrently() {
        // stderr fills far beyond the stream buffer before stdout closes:
        // reading stdout to EOF first would deadlock.
        let (out_r, mut out_w) = tokio::io::simplex(1024);
        let (err_r, mut err_w) = tokio::io::simplex(1024);
        tokio::spawn(async move {
            err_w.write_all(&vec![b'e'; 64 * 1024]).await.unwrap();
            err_w.shutdown().await.unwrap();
            out_w.write_all(b"done").await.unwrap();
            out_w.shutdown().await.unwrap();
        });
        let (out, err, timed_out) = collect_output(Some(out_r), Some(err_r), far_deadline()).await;
        assert!(!timed_out);
        assert_eq!(out.into_string(), "done");
        assert_eq!(err.data.len(), 64 * 1024);
        assert!(!err.truncated);
    }

    #[tokio::test]
    async fn collect_output_caps_each_stream() {
        let (out_r, mut out_w) = tokio::io::simplex(64 * 1024);
        tokio::spawn(async move {
            let chunk = vec![b'x'; 256 * 1024];
            for _ in 0..6 {
                out_w.write_all(&chunk).await.unwrap();
            }
            out_w.shutdown().await.unwrap();
        });
        let none: Option<tokio::io::ReadHalf<tokio::io::SimplexStream>> = None;
        let (out, _, timed_out) = collect_output(Some(out_r), none, far_deadline()).await;
        assert!(!timed_out);
        assert!(out.truncated);
        assert_eq!(out.data.len(), EXEC_OUTPUT_CAP);
        assert!(out.into_string().ends_with("[output truncated at 1 MiB]"));
    }

    #[tokio::test]
    async fn collect_output_stops_at_the_deadline_and_keeps_partial_output() {
        let (out_r, mut out_w) = tokio::io::simplex(1024);
        out_w.write_all(b"partial").await.unwrap();
        // out_w stays open: the "command" never finishes.
        let none: Option<tokio::io::ReadHalf<tokio::io::SimplexStream>> = None;
        let deadline = Instant::now() + Duration::from_millis(100);
        let (out, _, timed_out) = collect_output(Some(out_r), none, deadline).await;
        assert!(timed_out);
        assert_eq!(out.into_string(), "partial");
        drop(out_w);
    }

    fn pod_from(spec: serde_json::Value) -> Pod {
        serde_json::from_value(serde_json::json!({
            "metadata": { "name": "p", "namespace": "default" },
            "spec": spec
        }))
        .unwrap()
    }

    #[test]
    fn exec_violations_flag_node_level_access_only() {
        let policy = crate::policy::WorkloadPolicy::default();
        let privileged = pod_from(serde_json::json!({
            "hostIPC": true,
            "containers": [{ "name": "c", "image": "busybox",
                "securityContext": { "privileged": true, "capabilities": { "add": ["SYS_ADMIN"] } } }]
        }));
        let fields: Vec<&str> = exec_violations(&policy, &privileged)
            .iter()
            .map(|v| v.flag().field)
            .collect();
        assert!(fields.contains(&"allow_privileged"), "{fields:?}");
        assert!(fields.contains(&"allow_host_namespaces"), "{fields:?}");
        assert!(fields.contains(&"allowed_capabilities"), "{fields:?}");

        // Creation-time rules that don't give an exec session node access.
        let restricted = crate::policy::WorkloadPolicy {
            allowed_registries: vec!["ghcr.io/me".to_string()],
            ..Default::default()
        };
        let ordinary = pod_from(serde_json::json!({
            "containers": [{ "name": "c", "image": "docker.io/nginx",
                "ports": [{ "containerPort": 80, "hostPort": 80 }],
                "livenessProbe": { "httpGet": { "host": "example.com", "port": 80 } },
                "securityContext": { "allowPrivilegeEscalation": true } }],
            "volumes": [{ "name": "v", "nfs": { "server": "s", "path": "/" } }]
        }));
        assert!(exec_violations(&restricted, &ordinary).is_empty());

        // The operator's opt-in lifts the refusal.
        let allow = crate::policy::WorkloadPolicy {
            allow_privileged: true,
            ..Default::default()
        };
        let only_privileged = pod_from(serde_json::json!({
            "containers": [{ "name": "c", "image": "busybox", "securityContext": { "privileged": true } }]
        }));
        assert!(exec_violations(&allow, &only_privileged).is_empty());
    }
}
