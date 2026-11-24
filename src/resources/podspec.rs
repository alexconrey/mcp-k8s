//! Shared pod template input schema and builder for the typed workload tools
//! (`create_pod`, `create_job`, `create_cronjob`, `create_deployment`,
//! `create_statefulset`, `create_daemonset` and their `update_*` tools).
//!
//! Every workload tool takes the same container and pod arguments, parsed by
//! the same code into `k8s_openapi` types. Argument names are snake_case; the
//! Kubernetes camelCase spelling of each field is accepted as an alias.
//! Unknown fields are rejected with the list of valid ones, so a typo can't
//! silently drop a setting.
//!
//! Guardrails live in [`crate::policy`]: the tools check the object built here
//! with [`crate::policy::check_object`] before creating it, exactly as
//! `apply_manifest` checks a raw manifest. This module applies the
//! *defaults* (restricted securityContext, `automountServiceAccountToken:
//! false`, `app.kubernetes.io/managed-by: mcp-k8s`) when
//! [`crate::policy::WorkloadPolicy::secure_defaults`] is on.

use std::collections::BTreeMap;

use k8s_openapi::api::core::v1::{
    Affinity, AppArmorProfile, CSIVolumeSource, Capabilities, ConfigMapEnvSource,
    ConfigMapKeySelector, ConfigMapProjection, ConfigMapVolumeSource, Container, ContainerPort,
    DownwardAPIProjection, DownwardAPIVolumeFile, DownwardAPIVolumeSource, EmptyDirVolumeSource,
    EnvFromSource, EnvVar, EnvVarSource, ExecAction, GRPCAction, HTTPGetAction, HTTPHeader,
    HostAlias, HostPathVolumeSource, KeyToPath, Lifecycle, LifecycleHandler, LocalObjectReference,
    NFSVolumeSource, ObjectFieldSelector, PersistentVolumeClaimVolumeSource, PodDNSConfig,
    PodSecurityContext, PodSpec, PodTemplateSpec, Probe, ProjectedVolumeSource,
    ResourceFieldSelector, ResourceRequirements, SeccompProfile, SecretEnvSource,
    SecretKeySelector, SecretProjection, SecretVolumeSource, SecurityContext,
    ServiceAccountTokenProjection, SleepAction, Sysctl, TCPSocketAction, Toleration,
    TopologySpreadConstraint, Volume, VolumeMount, VolumeProjection,
};
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::policy::{WorkloadPolicy, MANAGED_BY_LABEL, MANAGED_BY_VALUE};

/// Description shared by the typed workload create tools.
pub const CREATE_DESCRIPTION: &str = "Preferred over apply_manifest: arguments are validated and the server's guardrails are enforced. \
Containers: image (+ command, args, env with secret/configMap/field references, env_from, ports, resources, volume_mounts, liveness/readiness/startup probes, lifecycle hooks, working_dir, image_pull_policy) for one container, or containers for several; init_containers (restart_policy: Always makes a native sidecar). \
Pod: volumes (configMap, secret, emptyDir, persistentVolumeClaim, projected, downwardAPI), service_account_name, image_pull_secrets, node_selector, tolerations, affinity, topology_spread_constraints, priority_class_name, dns_policy/dns_config, host_aliases, runtime_class_name, security_context, labels, annotations. \
Defaults: runAsNonRoot, seccomp RuntimeDefault, allowPrivilegeEscalation false and capabilities drop ALL (set security_context.run_as_non_root=false for images that must run as root), and no service account token unless service_account_name is set. \
Privileged containers, host namespaces, hostPath volumes, host ports, extra capabilities and risky volume types are rejected unless the server operator allows them; the error names the setting.";

/// Description shared by the typed workload update tools.
pub const UPDATE_DESCRIPTION: &str = "Preferred over apply_manifest. Takes the same container and pod fields as the matching create tool, all optional; only the fields you give change (strategic merge: containers, env, ports, volumes and volume_mounts merge by name or key). \
Container fields apply to `container` (default: the container named after the workload, or the only one). \
The result is checked against the server's guardrails with a server-side dry run before it is written, so an update can't add a disallowed setting or modify a workload that already has one.";

/// The selector label the typed workload tools use: `app: <name>`.
pub const APP_LABEL: &str = "app";

/// Container fields that may appear at the top level of a workload tool's
/// arguments as a shorthand for a single container.
pub const CONTAINER_SHORTHAND_KEYS: &[&str] = &[
    "image",
    "command",
    "args",
    "working_dir",
    "image_pull_policy",
    "env",
    "env_from",
    "port",
    "ports",
    "resources",
    "volume_mounts",
    "liveness_probe",
    "readiness_probe",
    "startup_probe",
    "lifecycle",
];

/// Top-level pod fields shared by every workload tool.
pub const POD_KEYS: &[&str] = &[
    "containers",
    "init_containers",
    "container_name",
    "volumes",
    "service_account_name",
    "automount_service_account_token",
    "image_pull_secrets",
    "node_selector",
    "tolerations",
    "affinity",
    "topology_spread_constraints",
    "priority_class_name",
    "termination_grace_period_seconds",
    "dns_policy",
    "dns_config",
    "host_aliases",
    "runtime_class_name",
    "hostname",
    "subdomain",
    "share_process_namespace",
    "enable_service_links",
    "host_network",
    "host_pid",
    "host_ipc",
    "security_context",
    "restart_policy",
    "labels",
    "annotations",
    "pod_annotations",
];

/// The kind of workload a pod template is built for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkloadKind {
    Pod,
    Job,
    CronJob,
    Deployment,
    StatefulSet,
    DaemonSet,
}

impl WorkloadKind {
    fn restart_policies(self) -> &'static [&'static str] {
        match self {
            WorkloadKind::Pod => &["Never", "OnFailure", "Always"],
            WorkloadKind::Job | WorkloadKind::CronJob => &["Never", "OnFailure"],
            _ => &["Always"],
        }
    }

    fn default_restart_policy(self) -> Option<&'static str> {
        match self {
            WorkloadKind::Pod | WorkloadKind::Job => Some("Never"),
            WorkloadKind::CronJob => Some("OnFailure"),
            _ => None,
        }
    }

    /// Whether the workload selects its pods by the `app` label.
    fn has_app_selector(self) -> bool {
        matches!(
            self,
            WorkloadKind::Deployment | WorkloadKind::StatefulSet | WorkloadKind::DaemonSet
        )
    }
}

// ---------------------------------------------------------------------------
// Argument helpers
// ---------------------------------------------------------------------------

/// `"serviceAccountName"` → `"service_account_name"`.
fn snake_case(key: &str) -> String {
    let mut out = String::with_capacity(key.len() + 4);
    for (i, c) in key.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// Normalize a tool's top-level arguments against its input schema: accept
/// the camelCase spelling of any snake_case property (and a few aliases), and
/// reject unknown arguments, listing the valid ones.
pub fn normalize_args(args: &Value, schema: &Value) -> Result<Value, String> {
    let props = schema["properties"]
        .as_object()
        .ok_or("tool schema has no properties")?;
    let Some(input) = args.as_object() else {
        return if args.is_null() {
            Ok(json!({}))
        } else {
            Err("arguments must be an object".to_string())
        };
    };
    let mut out = Map::new();
    for (key, value) in input {
        let canonical = if props.contains_key(key) {
            key.clone()
        } else {
            let snake = snake_case(key);
            let alias = match snake.as_str() {
                "service_account" => "service_account_name".to_string(),
                _ => snake,
            };
            if !props.contains_key(&alias) {
                let mut valid: Vec<&str> = props.keys().map(String::as_str).collect();
                valid.sort_unstable();
                return Err(format!(
                    "unknown argument '{key}'. Valid arguments: {}",
                    valid.join(", ")
                ));
            }
            alias
        };
        if out.contains_key(&canonical) {
            return Err(format!(
                "argument '{canonical}' was given twice (as '{key}' too)"
            ));
        }
        out.insert(canonical, value.clone());
    }
    Ok(Value::Object(out))
}

/// Deserialize a nested argument with a readable error.
pub(crate) fn parse<T: DeserializeOwned>(value: &Value, what: &str) -> Result<T, String> {
    serde_json::from_value(value.clone()).map_err(|e| format!("{what}: {e}"))
}

pub(crate) fn opt<T: DeserializeOwned>(args: &Value, key: &str) -> Result<Option<T>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => parse(v, key).map(Some),
    }
}

/// A string, or a number/bool rendered as one (env values, quantities).
pub(crate) fn scalar_string(value: &Value, what: &str) -> Result<String, String> {
    match value {
        Value::String(s) => Ok(s.clone()),
        Value::Number(n) => Ok(n.to_string()),
        Value::Bool(b) => Ok(b.to_string()),
        _ => Err(format!("{what} must be a string")),
    }
}

/// An integer, or a string (a named port or a percentage).
pub(crate) fn int_or_string(value: &Value, what: &str) -> Result<IntOrString, String> {
    match value {
        Value::Number(n) => n
            .as_i64()
            .and_then(|n| i32::try_from(n).ok())
            .map(IntOrString::Int)
            .ok_or_else(|| format!("{what} must be a 32-bit integer")),
        Value::String(s) => Ok(match s.parse::<i32>() {
            Ok(n) => IntOrString::Int(n),
            Err(_) => IntOrString::String(s.clone()),
        }),
        _ => Err(format!("{what} must be an integer or a string")),
    }
}

fn quantity_map(
    map: &BTreeMap<String, Value>,
    what: &str,
) -> Result<BTreeMap<String, Quantity>, String> {
    map.iter()
        .map(|(k, v)| {
            Ok((
                k.clone(),
                Quantity(scalar_string(v, &format!("{what}.{k}"))?),
            ))
        })
        .collect()
}

pub(crate) fn check_enum(value: &str, allowed: &[&str], what: &str) -> Result<(), String> {
    if allowed.contains(&value) {
        Ok(())
    } else {
        Err(format!(
            "{what} must be one of {}, got {value:?}",
            allowed.join(", ")
        ))
    }
}

/// Convert snake_case keys of a Kubernetes-shaped object to camelCase, except
/// inside free-form maps (`matchLabels`). Used for the pass-through fields
/// (`affinity`, `topology_spread_constraints`, `pod_failure_policy`) so both
/// spellings work.
pub fn camelize_keys(value: &Value) -> Value {
    fn camel(key: &str) -> String {
        let mut out = String::with_capacity(key.len());
        let mut upper = false;
        for c in key.chars() {
            if c == '_' {
                upper = true;
            } else if upper {
                out.push(c.to_ascii_uppercase());
                upper = false;
            } else {
                out.push(c);
            }
        }
        out
    }
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, v)| {
                    let key = camel(k);
                    let v = if key == "matchLabels" {
                        v.clone()
                    } else {
                        camelize_keys(v)
                    };
                    (key, v)
                })
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(camelize_keys).collect()),
        other => other.clone(),
    }
}

/// Parse a Kubernetes-shaped pass-through argument (camelCase or snake_case).
pub fn passthrough<T: DeserializeOwned>(args: &Value, key: &str) -> Result<Option<T>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => parse(&camelize_keys(v), key).map(Some),
    }
}

// ---------------------------------------------------------------------------
// Input types
// ---------------------------------------------------------------------------

#[derive(Deserialize, Debug, Default, Clone)]
#[serde(deny_unknown_fields)]
struct ContainerArgs {
    name: Option<String>,
    image: Option<String>,
    command: Option<Vec<String>>,
    args: Option<Vec<String>>,
    #[serde(alias = "workingDir")]
    working_dir: Option<String>,
    #[serde(alias = "imagePullPolicy")]
    image_pull_policy: Option<String>,
    env: Option<Value>,
    #[serde(alias = "envFrom")]
    env_from: Option<Vec<EnvFromArgs>>,
    port: Option<i32>,
    ports: Option<Vec<Value>>,
    resources: Option<ResourcesArgs>,
    #[serde(alias = "volumeMounts")]
    volume_mounts: Option<Vec<VolumeMountArgs>>,
    #[serde(alias = "livenessProbe")]
    liveness_probe: Option<ProbeArgs>,
    #[serde(alias = "readinessProbe")]
    readiness_probe: Option<ProbeArgs>,
    #[serde(alias = "startupProbe")]
    startup_probe: Option<ProbeArgs>,
    lifecycle: Option<LifecycleArgs>,
    #[serde(alias = "securityContext")]
    security_context: Option<SecurityArgs>,
    #[serde(alias = "restartPolicy")]
    restart_policy: Option<String>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct EnvVarArgs {
    name: String,
    value: Option<Value>,
    #[serde(alias = "valueFrom")]
    value_from: Option<EnvSourceArgs>,
}

#[derive(Deserialize, Debug, Default, Clone)]
#[serde(deny_unknown_fields)]
struct EnvSourceArgs {
    #[serde(alias = "secretKeyRef")]
    secret_key_ref: Option<KeyRefArgs>,
    #[serde(alias = "configMapKeyRef")]
    config_map_key_ref: Option<KeyRefArgs>,
    #[serde(alias = "fieldRef")]
    field_ref: Option<FieldRefArgs>,
    #[serde(alias = "resourceFieldRef")]
    resource_field_ref: Option<ResourceFieldRefArgs>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct KeyRefArgs {
    name: String,
    key: String,
    optional: Option<bool>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct FieldRefArgs {
    #[serde(alias = "fieldPath")]
    field_path: String,
    #[serde(alias = "apiVersion")]
    api_version: Option<String>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct ResourceFieldRefArgs {
    resource: String,
    #[serde(alias = "containerName")]
    container_name: Option<String>,
    divisor: Option<Value>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct EnvFromArgs {
    #[serde(alias = "secretRef")]
    secret_ref: Option<Value>,
    #[serde(alias = "configMapRef")]
    config_map_ref: Option<Value>,
    prefix: Option<String>,
    optional: Option<bool>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct PortArgs {
    #[serde(alias = "containerPort")]
    container_port: i32,
    name: Option<String>,
    protocol: Option<String>,
    #[serde(alias = "hostPort")]
    host_port: Option<i32>,
    #[serde(alias = "hostIP", alias = "hostIp")]
    host_ip: Option<String>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct ResourcesArgs {
    requests: Option<BTreeMap<String, Value>>,
    limits: Option<BTreeMap<String, Value>>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct VolumeMountArgs {
    name: String,
    #[serde(alias = "mountPath")]
    mount_path: String,
    #[serde(alias = "subPath")]
    sub_path: Option<String>,
    #[serde(alias = "subPathExpr")]
    sub_path_expr: Option<String>,
    #[serde(alias = "readOnly")]
    read_only: Option<bool>,
    #[serde(alias = "mountPropagation")]
    mount_propagation: Option<String>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct NameValue {
    name: String,
    value: String,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct ProbeArgs {
    #[serde(alias = "probeType", alias = "type")]
    probe_type: String,
    path: Option<String>,
    port: Option<Value>,
    scheme: Option<String>,
    #[serde(alias = "httpHeaders")]
    http_headers: Option<Vec<NameValue>>,
    command: Option<Vec<String>>,
    service: Option<String>,
    #[serde(alias = "initialDelaySeconds")]
    initial_delay_seconds: Option<i32>,
    #[serde(alias = "periodSeconds")]
    period_seconds: Option<i32>,
    #[serde(alias = "timeoutSeconds")]
    timeout_seconds: Option<i32>,
    #[serde(alias = "failureThreshold")]
    failure_threshold: Option<i32>,
    #[serde(alias = "successThreshold")]
    success_threshold: Option<i32>,
    #[serde(alias = "terminationGracePeriodSeconds")]
    termination_grace_period_seconds: Option<i64>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct LifecycleArgs {
    #[serde(alias = "postStart")]
    post_start: Option<HandlerArgs>,
    #[serde(alias = "preStop")]
    pre_stop: Option<HandlerArgs>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct HandlerArgs {
    #[serde(alias = "handlerType", alias = "type")]
    handler_type: String,
    command: Option<Vec<String>>,
    path: Option<String>,
    port: Option<Value>,
    scheme: Option<String>,
    seconds: Option<i64>,
}

/// Security settings. At the top level of a workload tool these are
/// pod-wide (`run_as_*`, `fs_group`, `seccomp_profile`, … go to the pod's
/// securityContext; `privileged`, `allow_privilege_escalation`,
/// `capabilities`, `read_only_root_filesystem` become every container's
/// default). Inside a `containers[]` entry only container fields are allowed.
#[derive(Deserialize, Debug, Default, Clone)]
#[serde(deny_unknown_fields)]
struct SecurityArgs {
    #[serde(alias = "runAsNonRoot")]
    run_as_non_root: Option<bool>,
    #[serde(alias = "runAsUser")]
    run_as_user: Option<i64>,
    #[serde(alias = "runAsGroup")]
    run_as_group: Option<i64>,
    #[serde(alias = "readOnlyRootFilesystem")]
    read_only_root_filesystem: Option<bool>,
    #[serde(alias = "allowPrivilegeEscalation")]
    allow_privilege_escalation: Option<bool>,
    privileged: Option<bool>,
    capabilities: Option<CapabilitiesArgs>,
    #[serde(alias = "seccompProfile")]
    seccomp_profile: Option<Value>,
    #[serde(alias = "appArmorProfile")]
    app_armor_profile: Option<Value>,
    // Pod-level only:
    #[serde(alias = "fsGroup")]
    fs_group: Option<i64>,
    #[serde(alias = "fsGroupChangePolicy")]
    fs_group_change_policy: Option<String>,
    #[serde(alias = "supplementalGroups")]
    supplemental_groups: Option<Vec<i64>>,
    sysctls: Option<Vec<NameValue>>,
}

impl SecurityArgs {
    fn pod_only_fields(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        if self.fs_group.is_some() {
            out.push("fs_group");
        }
        if self.fs_group_change_policy.is_some() {
            out.push("fs_group_change_policy");
        }
        if self.supplemental_groups.is_some() {
            out.push("supplemental_groups");
        }
        if self.sysctls.is_some() {
            out.push("sysctls");
        }
        out
    }
}

#[derive(Deserialize, Debug, Default, Clone)]
#[serde(deny_unknown_fields)]
struct CapabilitiesArgs {
    add: Option<Vec<String>>,
    drop: Option<Vec<String>>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct VolumeArgs {
    name: String,
    #[serde(alias = "configMap")]
    config_map: Option<ConfigMapVolumeArgs>,
    secret: Option<SecretVolumeArgs>,
    #[serde(alias = "emptyDir")]
    empty_dir: Option<EmptyDirArgs>,
    #[serde(alias = "persistentVolumeClaim")]
    persistent_volume_claim: Option<PvcVolumeArgs>,
    projected: Option<ProjectedArgs>,
    #[serde(alias = "downwardAPI", alias = "downwardApi")]
    downward_api: Option<DownwardApiArgs>,
    #[serde(alias = "hostPath")]
    host_path: Option<HostPathArgs>,
    nfs: Option<NfsArgs>,
    csi: Option<CsiArgs>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct ConfigMapVolumeArgs {
    name: String,
    items: Option<Vec<KeyPathArgs>>,
    #[serde(alias = "defaultMode")]
    default_mode: Option<i32>,
    optional: Option<bool>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct SecretVolumeArgs {
    #[serde(alias = "secretName", alias = "name")]
    secret_name: String,
    items: Option<Vec<KeyPathArgs>>,
    #[serde(alias = "defaultMode")]
    default_mode: Option<i32>,
    optional: Option<bool>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct KeyPathArgs {
    key: String,
    path: String,
    mode: Option<i32>,
}

#[derive(Deserialize, Debug, Default, Clone)]
#[serde(deny_unknown_fields)]
struct EmptyDirArgs {
    medium: Option<String>,
    #[serde(alias = "sizeLimit")]
    size_limit: Option<Value>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct PvcVolumeArgs {
    #[serde(alias = "claimName")]
    claim_name: String,
    #[serde(alias = "readOnly")]
    read_only: Option<bool>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct ProjectedArgs {
    sources: Vec<ProjectionArgs>,
    #[serde(alias = "defaultMode")]
    default_mode: Option<i32>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct ProjectionArgs {
    #[serde(alias = "configMap")]
    config_map: Option<ConfigMapProjectionArgs>,
    secret: Option<ConfigMapProjectionArgs>,
    #[serde(alias = "serviceAccountToken")]
    service_account_token: Option<SaTokenArgs>,
    #[serde(alias = "downwardAPI", alias = "downwardApi")]
    downward_api: Option<DownwardApiArgs>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct ConfigMapProjectionArgs {
    name: String,
    items: Option<Vec<KeyPathArgs>>,
    optional: Option<bool>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct SaTokenArgs {
    path: String,
    audience: Option<String>,
    #[serde(alias = "expirationSeconds")]
    expiration_seconds: Option<i64>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct DownwardApiArgs {
    items: Vec<DownwardItemArgs>,
    #[serde(alias = "defaultMode")]
    default_mode: Option<i32>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct DownwardItemArgs {
    path: String,
    #[serde(alias = "fieldRef")]
    field_ref: Option<FieldRefArgs>,
    #[serde(alias = "resourceFieldRef")]
    resource_field_ref: Option<ResourceFieldRefArgs>,
    mode: Option<i32>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct HostPathArgs {
    path: String,
    #[serde(rename = "type")]
    type_: Option<String>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct NfsArgs {
    server: String,
    path: String,
    #[serde(alias = "readOnly")]
    read_only: Option<bool>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct CsiArgs {
    driver: String,
    #[serde(alias = "readOnly")]
    read_only: Option<bool>,
    #[serde(alias = "volumeAttributes")]
    volume_attributes: Option<BTreeMap<String, String>>,
    #[serde(alias = "fsType")]
    fs_type: Option<String>,
    #[serde(alias = "nodePublishSecretRef")]
    node_publish_secret_ref: Option<String>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct TolerationArgs {
    key: Option<String>,
    operator: Option<String>,
    value: Option<String>,
    effect: Option<String>,
    #[serde(alias = "tolerationSeconds")]
    toleration_seconds: Option<i64>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct HostAliasArgs {
    ip: String,
    hostnames: Vec<String>,
}

// ---------------------------------------------------------------------------
// Conversions
// ---------------------------------------------------------------------------

fn key_paths(items: Option<Vec<KeyPathArgs>>) -> Option<Vec<KeyToPath>> {
    items.map(|items| {
        items
            .into_iter()
            .map(|i| KeyToPath {
                key: i.key,
                path: i.path,
                mode: i.mode,
            })
            .collect()
    })
}

fn field_selector(f: FieldRefArgs) -> ObjectFieldSelector {
    ObjectFieldSelector {
        field_path: f.field_path,
        api_version: f.api_version,
    }
}

fn resource_selector(r: ResourceFieldRefArgs, what: &str) -> Result<ResourceFieldSelector, String> {
    Ok(ResourceFieldSelector {
        resource: r.resource,
        container_name: r.container_name,
        divisor: r
            .divisor
            .map(|d| scalar_string(&d, &format!("{what}.divisor")).map(Quantity))
            .transpose()?,
    })
}

fn env_source(src: EnvSourceArgs, what: &str) -> Result<EnvVarSource, String> {
    let set = [
        src.secret_key_ref.is_some(),
        src.config_map_key_ref.is_some(),
        src.field_ref.is_some(),
        src.resource_field_ref.is_some(),
    ]
    .iter()
    .filter(|b| **b)
    .count();
    if set != 1 {
        return Err(format!(
            "{what}: set exactly one of secret_key_ref, config_map_key_ref, field_ref, resource_field_ref"
        ));
    }
    // `..Default::default()` is redundant with k8s-openapi v1_33 but needed
    // with v1_34+, which adds `file_key_ref`; the library supports both.
    #[allow(clippy::needless_update)]
    Ok(EnvVarSource {
        secret_key_ref: src.secret_key_ref.map(|r| SecretKeySelector {
            name: r.name,
            key: r.key,
            optional: r.optional,
        }),
        config_map_key_ref: src.config_map_key_ref.map(|r| ConfigMapKeySelector {
            name: r.name,
            key: r.key,
            optional: r.optional,
        }),
        field_ref: src.field_ref.map(field_selector),
        resource_field_ref: src
            .resource_field_ref
            .map(|r| resource_selector(r, what))
            .transpose()?,
        ..Default::default()
    })
}

/// `env` as `{NAME: "value" | {secret_key_ref: …}}` or
/// `[{name, value | value_from}]`.
fn env_vars(env: &Value, what: &str) -> Result<Vec<EnvVar>, String> {
    match env {
        Value::Object(map) => map
            .iter()
            .map(|(name, v)| {
                let field = format!("{what}.{name}");
                Ok(match v {
                    Value::Object(_) => EnvVar {
                        name: name.clone(),
                        value_from: Some(env_source(parse(v, &field)?, &field)?),
                        ..Default::default()
                    },
                    Value::Null => EnvVar {
                        name: name.clone(),
                        ..Default::default()
                    },
                    other => EnvVar {
                        name: name.clone(),
                        value: Some(scalar_string(other, &field)?),
                        ..Default::default()
                    },
                })
            })
            .collect(),
        Value::Array(items) => items
            .iter()
            .enumerate()
            .map(|(i, item)| {
                let field = format!("{what}[{i}]");
                let var: EnvVarArgs = parse(item, &field)?;
                if var.value.is_some() && var.value_from.is_some() {
                    return Err(format!("{field}: set value or value_from, not both"));
                }
                Ok(EnvVar {
                    name: var.name,
                    value: var
                        .value
                        .map(|v| scalar_string(&v, &field))
                        .transpose()?,
                    value_from: var
                        .value_from
                        .map(|src| env_source(src, &field))
                        .transpose()?,
                })
            })
            .collect(),
        _ => Err(format!(
            "{what} must be an object of NAME: value pairs or an array of {{name, value | value_from}}"
        )),
    }
}

/// `secret_ref` / `config_map_ref` as a name or `{name, optional}`.
fn ref_name(v: &Value, what: &str) -> Result<(String, Option<bool>), String> {
    match v {
        Value::String(s) => Ok((s.clone(), None)),
        Value::Object(o) => {
            let name = o
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| format!("{what}.name is required"))?;
            Ok((name.to_string(), o.get("optional").and_then(Value::as_bool)))
        }
        _ => Err(format!("{what} must be a name or {{name}}")),
    }
}

fn env_from_sources(items: Vec<EnvFromArgs>, what: &str) -> Result<Vec<EnvFromSource>, String> {
    items
        .into_iter()
        .enumerate()
        .map(|(i, item)| {
            let field = format!("{what}[{i}]");
            match (item.secret_ref, item.config_map_ref) {
                (Some(s), None) => {
                    let (name, optional) = ref_name(&s, &format!("{field}.secret_ref"))?;
                    Ok(EnvFromSource {
                        secret_ref: Some(SecretEnvSource {
                            name,
                            optional: optional.or(item.optional),
                        }),
                        prefix: item.prefix,
                        ..Default::default()
                    })
                }
                (None, Some(c)) => {
                    let (name, optional) = ref_name(&c, &format!("{field}.config_map_ref"))?;
                    Ok(EnvFromSource {
                        config_map_ref: Some(ConfigMapEnvSource {
                            name,
                            optional: optional.or(item.optional),
                        }),
                        prefix: item.prefix,
                        ..Default::default()
                    })
                }
                _ => Err(format!(
                    "{field}: set exactly one of secret_ref or config_map_ref"
                )),
            }
        })
        .collect()
}

fn container_ports(
    port: Option<i32>,
    ports: Option<Vec<Value>>,
    what: &str,
) -> Result<Option<Vec<ContainerPort>>, String> {
    let mut out = Vec::new();
    if let Some(p) = port {
        out.push(ContainerPort {
            container_port: p,
            ..Default::default()
        });
    }
    for (i, p) in ports.unwrap_or_default().into_iter().enumerate() {
        let field = format!("{what}[{i}]");
        let parsed = match &p {
            Value::Number(_) => PortArgs {
                container_port: parse(&p, &field)?,
                name: None,
                protocol: None,
                host_port: None,
                host_ip: None,
            },
            _ => parse(&p, &field)?,
        };
        if let Some(proto) = &parsed.protocol {
            check_enum(proto, &["TCP", "UDP", "SCTP"], &format!("{field}.protocol"))?;
        }
        out.push(ContainerPort {
            container_port: parsed.container_port,
            name: parsed.name,
            protocol: parsed.protocol,
            host_port: parsed.host_port,
            host_ip: parsed.host_ip,
        });
    }
    Ok(if out.is_empty() { None } else { Some(out) })
}

fn probe(p: ProbeArgs, what: &str) -> Result<Probe, String> {
    let port = |default: i32| -> Result<IntOrString, String> {
        p.port
            .as_ref()
            .map(|v| int_or_string(v, &format!("{what}.port")))
            .transpose()
            .map(|v| v.unwrap_or(IntOrString::Int(default)))
    };
    let mut out = Probe {
        initial_delay_seconds: p.initial_delay_seconds,
        period_seconds: p.period_seconds,
        timeout_seconds: p.timeout_seconds,
        failure_threshold: p.failure_threshold,
        success_threshold: p.success_threshold,
        termination_grace_period_seconds: p.termination_grace_period_seconds,
        ..Default::default()
    };
    match p.probe_type.as_str() {
        "httpGet" | "http_get" | "http" => {
            if let Some(s) = &p.scheme {
                check_enum(s, &["HTTP", "HTTPS"], &format!("{what}.scheme"))?;
            }
            out.http_get = Some(HTTPGetAction {
                path: p.path.clone(),
                port: port(80)?,
                scheme: p.scheme.clone(),
                http_headers: p.http_headers.clone().map(|hs| {
                    hs.into_iter()
                        .map(|h| HTTPHeader {
                            name: h.name,
                            value: h.value,
                        })
                        .collect()
                }),
                ..Default::default()
            });
        }
        "tcpSocket" | "tcp_socket" | "tcp" => {
            out.tcp_socket = Some(TCPSocketAction {
                port: port(80)?,
                ..Default::default()
            });
        }
        "exec" => {
            let command = p
                .command
                .clone()
                .filter(|c| !c.is_empty())
                .ok_or_else(|| format!("{what}: exec probes need a command"))?;
            out.exec = Some(ExecAction {
                command: Some(command),
            });
        }
        "grpc" => {
            let port = match port(0)? {
                IntOrString::Int(n) if n > 0 => n,
                _ => return Err(format!("{what}: grpc probes need a numeric port")),
            };
            out.grpc = Some(GRPCAction {
                port,
                service: p.service.clone(),
            });
        }
        other => {
            return Err(format!(
                "{what}.probe_type must be one of httpGet, tcpSocket, exec, grpc, got {other:?}"
            ))
        }
    }
    Ok(out)
}

/// Build a probe from the `liveness_probe`/`readiness_probe`/`startup_probe`
/// argument shape. Returns `Ok(None)` when `key` is absent.
pub fn probe_from_args(args: &Value, key: &str) -> Result<Option<Probe>, String> {
    opt::<ProbeArgs>(args, key)?
        .map(|p| probe(p, key))
        .transpose()
}

fn handler(h: HandlerArgs, what: &str) -> Result<LifecycleHandler, String> {
    Ok(match h.handler_type.as_str() {
        "exec" => LifecycleHandler {
            exec: Some(ExecAction {
                command: Some(
                    h.command
                        .filter(|c| !c.is_empty())
                        .ok_or_else(|| format!("{what}: exec hooks need a command"))?,
                ),
            }),
            ..Default::default()
        },
        "httpGet" | "http_get" | "http" => LifecycleHandler {
            http_get: Some(HTTPGetAction {
                path: h.path,
                port: h
                    .port
                    .as_ref()
                    .map(|v| int_or_string(v, &format!("{what}.port")))
                    .transpose()?
                    .unwrap_or(IntOrString::Int(80)),
                scheme: h.scheme,
                ..Default::default()
            }),
            ..Default::default()
        },
        "sleep" => LifecycleHandler {
            sleep: Some(SleepAction {
                seconds: h
                    .seconds
                    .ok_or_else(|| format!("{what}: sleep hooks need seconds"))?,
            }),
            ..Default::default()
        },
        other => {
            return Err(format!(
                "{what}.handler_type must be one of exec, httpGet, sleep, got {other:?}"
            ))
        }
    })
}

/// `"RuntimeDefault"` or `{type, localhost_profile}`.
fn profile(v: &Value, what: &str) -> Result<(String, Option<String>), String> {
    let (type_, localhost) = match v {
        Value::String(s) => (s.clone(), None),
        Value::Object(o) => (
            o.get("type")
                .and_then(Value::as_str)
                .ok_or_else(|| format!("{what}.type is required"))?
                .to_string(),
            o.get("localhost_profile")
                .or_else(|| o.get("localhostProfile"))
                .and_then(Value::as_str)
                .map(str::to_string),
        ),
        _ => {
            return Err(format!(
                "{what} must be a profile type or {{type, localhost_profile}}"
            ))
        }
    };
    check_enum(
        &type_,
        &["RuntimeDefault", "Localhost", "Unconfined"],
        &format!("{what}.type"),
    )?;
    if type_ == "Localhost" && localhost.is_none() {
        return Err(format!("{what}: Localhost profiles need localhost_profile"));
    }
    Ok((type_, localhost))
}

fn seccomp(v: &Value, what: &str) -> Result<SeccompProfile, String> {
    let (type_, localhost_profile) = profile(v, what)?;
    Ok(SeccompProfile {
        type_,
        localhost_profile,
    })
}

fn apparmor(v: &Value, what: &str) -> Result<AppArmorProfile, String> {
    let (type_, localhost_profile) = profile(v, what)?;
    Ok(AppArmorProfile {
        type_,
        localhost_profile,
    })
}

/// Container-level security fields from `s`.
fn container_security(s: &SecurityArgs, what: &str) -> Result<SecurityContext, String> {
    Ok(SecurityContext {
        run_as_non_root: s.run_as_non_root,
        run_as_user: s.run_as_user,
        run_as_group: s.run_as_group,
        read_only_root_filesystem: s.read_only_root_filesystem,
        allow_privilege_escalation: s.allow_privilege_escalation,
        privileged: s.privileged,
        capabilities: s.capabilities.clone().map(|c| Capabilities {
            add: c.add,
            drop: c.drop,
        }),
        seccomp_profile: s
            .seccomp_profile
            .as_ref()
            .map(|p| seccomp(p, &format!("{what}.seccomp_profile")))
            .transpose()?,
        app_armor_profile: s
            .app_armor_profile
            .as_ref()
            .map(|p| apparmor(p, &format!("{what}.app_armor_profile")))
            .transpose()?,
        ..Default::default()
    })
}

fn is_empty_security(sc: &SecurityContext) -> bool {
    sc == &SecurityContext::default()
}

/// Build a container. `defaults` are the pod-wide container security
/// defaults from the top-level `security_context`.
fn container(c: ContainerArgs, what: &str, require_image: bool) -> Result<Container, String> {
    let name = c
        .name
        .clone()
        .ok_or_else(|| format!("{what}.name is required"))?;
    if require_image && c.image.as_deref().is_none_or(str::is_empty) {
        return Err(format!("{what}.image is required"));
    }
    if let Some(p) = &c.image_pull_policy {
        check_enum(
            p,
            &["Always", "IfNotPresent", "Never"],
            &format!("{what}.image_pull_policy"),
        )?;
    }
    let security_context = match &c.security_context {
        Some(s) => {
            let pod_only = s.pod_only_fields();
            if !pod_only.is_empty() {
                return Err(format!(
                    "{what}.security_context: {} can only be set in the top-level security_context",
                    pod_only.join(", ")
                ));
            }
            Some(container_security(s, &format!("{what}.security_context"))?)
        }
        None => None,
    };
    Ok(Container {
        name,
        image: c.image,
        command: c.command,
        args: c.args,
        working_dir: c.working_dir,
        image_pull_policy: c.image_pull_policy,
        env: c
            .env
            .as_ref()
            .map(|e| env_vars(e, &format!("{what}.env")))
            .transpose()?,
        env_from: c
            .env_from
            .map(|e| env_from_sources(e, &format!("{what}.env_from")))
            .transpose()?,
        ports: container_ports(c.port, c.ports, &format!("{what}.ports"))?,
        resources: c
            .resources
            .map(|r| -> Result<ResourceRequirements, String> {
                Ok(ResourceRequirements {
                    requests: r
                        .requests
                        .as_ref()
                        .map(|m| quantity_map(m, &format!("{what}.resources.requests")))
                        .transpose()?,
                    limits: r
                        .limits
                        .as_ref()
                        .map(|m| quantity_map(m, &format!("{what}.resources.limits")))
                        .transpose()?,
                    ..Default::default()
                })
            })
            .transpose()?,
        volume_mounts: c.volume_mounts.map(|mounts| {
            mounts
                .into_iter()
                .map(|m| VolumeMount {
                    name: m.name,
                    mount_path: m.mount_path,
                    sub_path: m.sub_path,
                    sub_path_expr: m.sub_path_expr,
                    read_only: m.read_only,
                    mount_propagation: m.mount_propagation,
                    ..Default::default()
                })
                .collect()
        }),
        liveness_probe: c
            .liveness_probe
            .map(|p| probe(p, &format!("{what}.liveness_probe")))
            .transpose()?,
        readiness_probe: c
            .readiness_probe
            .map(|p| probe(p, &format!("{what}.readiness_probe")))
            .transpose()?,
        startup_probe: c
            .startup_probe
            .map(|p| probe(p, &format!("{what}.startup_probe")))
            .transpose()?,
        lifecycle: c
            .lifecycle
            .map(|l| -> Result<Lifecycle, String> {
                Ok(Lifecycle {
                    post_start: l
                        .post_start
                        .map(|h| handler(h, &format!("{what}.lifecycle.post_start")))
                        .transpose()?,
                    pre_stop: l
                        .pre_stop
                        .map(|h| handler(h, &format!("{what}.lifecycle.pre_stop")))
                        .transpose()?,
                    ..Default::default()
                })
            })
            .transpose()?,
        security_context,
        restart_policy: c.restart_policy,
        ..Default::default()
    })
}

fn volume(v: VolumeArgs, what: &str) -> Result<Volume, String> {
    let sources = [
        v.config_map.is_some(),
        v.secret.is_some(),
        v.empty_dir.is_some(),
        v.persistent_volume_claim.is_some(),
        v.projected.is_some(),
        v.downward_api.is_some(),
        v.host_path.is_some(),
        v.nfs.is_some(),
        v.csi.is_some(),
    ]
    .iter()
    .filter(|b| **b)
    .count();
    if sources != 1 {
        return Err(format!(
            "{what} ({}): set exactly one of config_map, secret, empty_dir, persistent_volume_claim, projected, downward_api, host_path, nfs, csi",
            v.name
        ));
    }
    let downward = |d: DownwardApiArgs| -> Result<Vec<DownwardAPIVolumeFile>, String> {
        d.items
            .into_iter()
            .map(|i| {
                Ok(DownwardAPIVolumeFile {
                    path: i.path,
                    field_ref: i.field_ref.map(field_selector),
                    resource_field_ref: i
                        .resource_field_ref
                        .map(|r| resource_selector(r, what))
                        .transpose()?,
                    mode: i.mode,
                })
            })
            .collect()
    };
    Ok(Volume {
        name: v.name,
        config_map: v.config_map.map(|c| ConfigMapVolumeSource {
            name: c.name,
            items: key_paths(c.items),
            default_mode: c.default_mode,
            optional: c.optional,
        }),
        secret: v.secret.map(|s| SecretVolumeSource {
            secret_name: Some(s.secret_name),
            items: key_paths(s.items),
            default_mode: s.default_mode,
            optional: s.optional,
        }),
        empty_dir: v
            .empty_dir
            .map(|e| -> Result<EmptyDirVolumeSource, String> {
                if let Some(m) = &e.medium {
                    check_enum(m, &["", "Memory"], &format!("{what}.empty_dir.medium"))?;
                }
                Ok(EmptyDirVolumeSource {
                    medium: e.medium,
                    size_limit: e
                        .size_limit
                        .map(|s| {
                            scalar_string(&s, &format!("{what}.empty_dir.size_limit")).map(Quantity)
                        })
                        .transpose()?,
                })
            })
            .transpose()?,
        persistent_volume_claim: v.persistent_volume_claim.map(|p| {
            PersistentVolumeClaimVolumeSource {
                claim_name: p.claim_name,
                read_only: p.read_only,
            }
        }),
        projected: v
            .projected
            .map(|p| -> Result<ProjectedVolumeSource, String> {
                let sources = p
                    .sources
                    .into_iter()
                    .map(|s| {
                        Ok(VolumeProjection {
                            config_map: s.config_map.map(|c| ConfigMapProjection {
                                name: c.name,
                                items: key_paths(c.items),
                                optional: c.optional,
                            }),
                            secret: s.secret.map(|c| SecretProjection {
                                name: c.name,
                                items: key_paths(c.items),
                                optional: c.optional,
                            }),
                            service_account_token: s.service_account_token.map(|t| {
                                ServiceAccountTokenProjection {
                                    path: t.path,
                                    audience: t.audience,
                                    expiration_seconds: t.expiration_seconds,
                                }
                            }),
                            downward_api: s
                                .downward_api
                                .map(|d| -> Result<DownwardAPIProjection, String> {
                                    Ok(DownwardAPIProjection {
                                        items: Some(downward(d)?),
                                    })
                                })
                                .transpose()?,
                            ..Default::default()
                        })
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                Ok(ProjectedVolumeSource {
                    sources: Some(sources),
                    default_mode: p.default_mode,
                })
            })
            .transpose()?,
        downward_api: v
            .downward_api
            .map(|d| -> Result<DownwardAPIVolumeSource, String> {
                let default_mode = d.default_mode;
                Ok(DownwardAPIVolumeSource {
                    items: Some(downward(d)?),
                    default_mode,
                })
            })
            .transpose()?,
        host_path: v.host_path.map(|h| HostPathVolumeSource {
            path: h.path,
            type_: h.type_,
        }),
        nfs: v.nfs.map(|n| NFSVolumeSource {
            server: n.server,
            path: n.path,
            read_only: n.read_only,
        }),
        csi: v.csi.map(|c| CSIVolumeSource {
            driver: c.driver,
            read_only: c.read_only,
            volume_attributes: c.volume_attributes,
            fs_type: c.fs_type,
            node_publish_secret_ref: c
                .node_publish_secret_ref
                .map(|name| LocalObjectReference { name }),
        }),
        ..Default::default()
    })
}

pub(crate) fn string_map(
    args: &Value,
    key: &str,
) -> Result<Option<BTreeMap<String, String>>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Object(map)) => map
            .iter()
            .map(|(k, v)| Ok((k.clone(), scalar_string(v, &format!("{key}.{k}"))?)))
            .collect::<Result<BTreeMap<_, _>, String>>()
            .map(Some),
        Some(_) => Err(format!("{key} must be an object of string values")),
    }
}

// ---------------------------------------------------------------------------
// Pod spec assembly
// ---------------------------------------------------------------------------

/// Pod-level fields parsed from the arguments (everything except containers
/// and metadata).
struct PodFields {
    spec: PodSpec,
    /// Container-level defaults from the top-level `security_context`.
    container_defaults: SecurityContext,
    /// Whether the caller set `automount_service_account_token`.
    automount_set: bool,
}

fn pod_fields(args: &Value, kind: WorkloadKind) -> Result<PodFields, String> {
    let security: Option<SecurityArgs> = opt(args, "security_context")?;
    let restart_policy = match args.get("restart_policy").and_then(Value::as_str) {
        Some(p) => {
            check_enum(p, kind.restart_policies(), "restart_policy")?;
            Some(p.to_string())
        }
        None => kind.default_restart_policy().map(str::to_string),
    };
    if let Some(p) = args.get("dns_policy").and_then(Value::as_str) {
        check_enum(
            p,
            &["ClusterFirst", "ClusterFirstWithHostNet", "Default", "None"],
            "dns_policy",
        )?;
    }

    let (pod_security, container_defaults) = match &security {
        Some(s) => {
            let pod = PodSecurityContext {
                run_as_non_root: s.run_as_non_root,
                run_as_user: s.run_as_user,
                run_as_group: s.run_as_group,
                fs_group: s.fs_group,
                fs_group_change_policy: s.fs_group_change_policy.clone(),
                supplemental_groups: s.supplemental_groups.clone(),
                seccomp_profile: s
                    .seccomp_profile
                    .as_ref()
                    .map(|p| seccomp(p, "security_context.seccomp_profile"))
                    .transpose()?,
                app_armor_profile: s
                    .app_armor_profile
                    .as_ref()
                    .map(|p| apparmor(p, "security_context.app_armor_profile"))
                    .transpose()?,
                sysctls: s.sysctls.clone().map(|v| {
                    v.into_iter()
                        .map(|s| Sysctl {
                            name: s.name,
                            value: s.value,
                        })
                        .collect()
                }),
                ..Default::default()
            };
            let defaults = SecurityContext {
                read_only_root_filesystem: s.read_only_root_filesystem,
                allow_privilege_escalation: s.allow_privilege_escalation,
                privileged: s.privileged,
                capabilities: s.capabilities.clone().map(|c| Capabilities {
                    add: c.add,
                    drop: c.drop,
                }),
                ..Default::default()
            };
            (Some(pod), defaults)
        }
        None => (None, SecurityContext::default()),
    };

    let volumes = opt::<Vec<VolumeArgs>>(args, "volumes")?
        .map(|vs| {
            vs.into_iter()
                .enumerate()
                .map(|(i, v)| volume(v, &format!("volumes[{i}]")))
                .collect::<Result<Vec<_>, String>>()
        })
        .transpose()?;
    let tolerations = opt::<Vec<TolerationArgs>>(args, "tolerations")?.map(|ts| {
        ts.into_iter()
            .map(|t| Toleration {
                key: t.key,
                operator: t.operator,
                value: t.value,
                effect: t.effect,
                toleration_seconds: t.toleration_seconds,
            })
            .collect()
    });
    let host_aliases = opt::<Vec<HostAliasArgs>>(args, "host_aliases")?.map(|hs| {
        hs.into_iter()
            .map(|h| HostAlias {
                ip: h.ip,
                hostnames: Some(h.hostnames),
            })
            .collect()
    });
    let image_pull_secrets = opt::<Vec<Value>>(args, "image_pull_secrets")?
        .map(|items| {
            items
                .iter()
                .enumerate()
                .map(|(i, v)| {
                    ref_name(v, &format!("image_pull_secrets[{i}]"))
                        .map(|(name, _)| LocalObjectReference { name })
                })
                .collect::<Result<Vec<_>, String>>()
        })
        .transpose()?;

    let spec = PodSpec {
        restart_policy,
        security_context: pod_security,
        volumes,
        service_account_name: args
            .get("service_account_name")
            .and_then(Value::as_str)
            .map(str::to_string),
        automount_service_account_token: args
            .get("automount_service_account_token")
            .and_then(Value::as_bool),
        image_pull_secrets,
        node_selector: string_map(args, "node_selector")?,
        tolerations,
        affinity: passthrough::<Affinity>(args, "affinity")?,
        topology_spread_constraints: passthrough::<Vec<TopologySpreadConstraint>>(
            args,
            "topology_spread_constraints",
        )?,
        priority_class_name: args
            .get("priority_class_name")
            .and_then(Value::as_str)
            .map(str::to_string),
        termination_grace_period_seconds: args
            .get("termination_grace_period_seconds")
            .and_then(Value::as_i64),
        dns_policy: args
            .get("dns_policy")
            .and_then(Value::as_str)
            .map(str::to_string),
        dns_config: passthrough::<PodDNSConfig>(args, "dns_config")?,
        host_aliases,
        runtime_class_name: args
            .get("runtime_class_name")
            .and_then(Value::as_str)
            .map(str::to_string),
        hostname: args
            .get("hostname")
            .and_then(Value::as_str)
            .map(str::to_string),
        subdomain: args
            .get("subdomain")
            .and_then(Value::as_str)
            .map(str::to_string),
        share_process_namespace: args.get("share_process_namespace").and_then(Value::as_bool),
        enable_service_links: args.get("enable_service_links").and_then(Value::as_bool),
        host_network: args.get("host_network").and_then(Value::as_bool),
        host_pid: args.get("host_pid").and_then(Value::as_bool),
        host_ipc: args.get("host_ipc").and_then(Value::as_bool),
        ..Default::default()
    };
    Ok(PodFields {
        spec,
        container_defaults,
        automount_set: args.get("automount_service_account_token").is_some(),
    })
}

/// The container arguments given at the top level (single-container form).
fn shorthand_container(args: &Value, name: &str) -> Result<Option<ContainerArgs>, String> {
    let mut map = Map::new();
    for key in CONTAINER_SHORTHAND_KEYS {
        if let Some(v) = args.get(*key) {
            if !v.is_null() {
                map.insert(key.to_string(), v.clone());
            }
        }
    }
    if map.is_empty() {
        return Ok(None);
    }
    map.insert("name".into(), Value::String(name.to_string()));
    parse(&Value::Object(map), "arguments").map(Some)
}

fn container_list(
    args: &Value,
    key: &str,
    require_image: bool,
) -> Result<Option<Vec<Container>>, String> {
    let Some(items) = opt::<Vec<Value>>(args, key)? else {
        return Ok(None);
    };
    items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let what = format!("{key}[{i}]");
            let c: ContainerArgs = parse(item, &what)?;
            if c.port.is_some() {
                return Err(format!("{what}: use ports instead of port"));
            }
            if key == "containers" && c.restart_policy.is_some() {
                return Err(format!(
                    "{what}.restart_policy is only valid for init_containers (restart_policy: Always makes a native sidecar)"
                ));
            }
            if let Some(p) = &c.restart_policy {
                check_enum(p, &["Always"], &format!("{what}.restart_policy"))?;
            }
            container(c, &what, require_image)
        })
        .collect::<Result<Vec<_>, String>>()
        .map(Some)
}

/// Apply the restricted defaults (when `secure_defaults` is on) and the
/// top-level container security defaults.
fn apply_container_defaults(
    c: &mut Container,
    defaults: &SecurityContext,
    pod: Option<&PodSecurityContext>,
    secure: bool,
    defaulted_non_root: bool,
) {
    let pod_non_root = pod.and_then(|p| p.run_as_non_root);
    let pod_user = pod.and_then(|p| p.run_as_user);
    let mut sc = c.security_context.take().unwrap_or_default();
    if sc.read_only_root_filesystem.is_none() {
        sc.read_only_root_filesystem = defaults.read_only_root_filesystem;
    }
    if sc.allow_privilege_escalation.is_none() {
        sc.allow_privilege_escalation = defaults.allow_privilege_escalation;
    }
    if sc.privileged.is_none() {
        sc.privileged = defaults.privileged;
    }
    if sc.capabilities.is_none() {
        sc.capabilities = defaults.capabilities.clone();
    }
    if secure && defaulted_non_root && sc.run_as_user == Some(0) && sc.run_as_non_root.is_none() {
        // The caller asked for UID 0 in this container: our pod-level
        // runAsNonRoot default must not make the kubelet refuse it.
        sc.run_as_non_root = Some(false);
    }
    if secure {
        let privileged = sc.privileged == Some(true);
        let adds_sys_admin = sc
            .capabilities
            .as_ref()
            .and_then(|c| c.add.as_ref())
            .is_some_and(|add| {
                add.iter()
                    .any(|a| crate::policy::normalize_capability(a) == "SYS_ADMIN")
            });
        // Kubernetes rejects allowPrivilegeEscalation: false together with
        // privileged or CAP_SYS_ADMIN.
        if sc.allow_privilege_escalation.is_none() && !privileged && !adds_sys_admin {
            sc.allow_privilege_escalation = Some(false);
        }
        // Drop every capability unless the caller asked to run as root (root
        // images usually need the runtime's default capabilities).
        let non_root = sc.run_as_non_root.or(pod_non_root) != Some(false)
            && sc.run_as_user.or(pod_user) != Some(0);
        if non_root && !privileged {
            let caps = sc.capabilities.get_or_insert_with(Capabilities::default);
            if caps.drop.is_none() {
                caps.drop = Some(vec!["ALL".to_string()]);
            }
        }
    }
    if !is_empty_security(&sc) {
        c.security_context = Some(sc);
    }
}

/// Build a pod spec from a workload tool's arguments.
///
/// `name` is the workload name: the default container name and `app` label.
pub fn build_pod_spec(
    args: &Value,
    name: &str,
    kind: WorkloadKind,
    policy: &WorkloadPolicy,
) -> Result<PodSpec, String> {
    let fields = pod_fields(args, kind)?;
    let mut spec = fields.spec;

    let container_name = args
        .get("container_name")
        .and_then(Value::as_str)
        .unwrap_or(name)
        .to_string();
    let shorthand = shorthand_container(args, &container_name)?;
    let listed = container_list(args, "containers", true)?;
    spec.containers = match (shorthand, listed) {
        (Some(_), Some(_)) => {
            return Err(
                "use either the single-container arguments (image, command, env, …) or containers, not both"
                    .to_string(),
            )
        }
        (Some(c), None) => vec![container(c, "arguments", true)?],
        (None, Some(list)) if !list.is_empty() => {
            if args.get("container_name").is_some() {
                return Err("container_name only applies to the single-container form; name each entry of containers instead".to_string());
            }
            list
        }
        _ => return Err("image (or a non-empty containers list) is required".to_string()),
    };
    spec.init_containers = container_list(args, "init_containers", true)?;

    let mut names = std::collections::HashSet::new();
    for c in spec
        .containers
        .iter()
        .chain(spec.init_containers.iter().flatten())
    {
        if !names.insert(c.name.clone()) {
            return Err(format!("container name {:?} is used twice", c.name));
        }
    }

    let secure = policy.secure_defaults;
    let mut defaulted_non_root = false;
    if secure {
        let sc = spec
            .security_context
            .get_or_insert_with(PodSecurityContext::default);
        if sc.run_as_non_root.is_none() && sc.run_as_user != Some(0) {
            sc.run_as_non_root = Some(true);
            defaulted_non_root = true;
        }
        if sc.seccomp_profile.is_none() {
            sc.seccomp_profile = Some(SeccompProfile {
                type_: "RuntimeDefault".to_string(),
                localhost_profile: None,
            });
        }
        if !fields.automount_set && spec.service_account_name.is_none() {
            spec.automount_service_account_token = Some(false);
        }
    }
    let pod_security = spec.security_context.clone();
    for c in spec
        .containers
        .iter_mut()
        .chain(spec.init_containers.iter_mut().flatten())
    {
        apply_container_defaults(
            c,
            &fields.container_defaults,
            pod_security.as_ref(),
            secure,
            defaulted_non_root,
        );
    }
    Ok(spec)
}

/// Labels and annotations for a workload object and its pod template.
pub struct TemplateMeta {
    /// Object labels: the caller's `labels`, `app: <name>`, and
    /// `app.kubernetes.io/managed-by: mcp-k8s`.
    pub labels: BTreeMap<String, String>,
    /// Object annotations (the caller's `annotations`).
    pub annotations: Option<BTreeMap<String, String>>,
    /// Pod template metadata: the same labels, plus `pod_annotations`.
    pub template: ObjectMeta,
    /// The selector, `app: <name>`.
    pub selector: BTreeMap<String, String>,
}

/// Build workload labels/annotations. For workloads that select pods by
/// `app`, the caller can't change that label.
pub fn build_meta(args: &Value, name: &str, kind: WorkloadKind) -> Result<TemplateMeta, String> {
    let mut labels = string_map(args, "labels")?.unwrap_or_default();
    let custom_selector = if kind.has_app_selector() {
        string_map(args, "selector")?.filter(|s| !s.is_empty())
    } else {
        None
    };
    let selector = match custom_selector {
        Some(selector) => {
            // The pods must carry every selector label.
            for (k, v) in &selector {
                match labels.get(k) {
                    Some(existing) if existing != v => {
                        return Err(format!(
                            "labels.{k} is {existing:?} but the selector needs {v:?}"
                        ))
                    }
                    _ => {
                        labels.insert(k.clone(), v.clone());
                    }
                }
            }
            labels
                .entry(APP_LABEL.to_string())
                .or_insert_with(|| name.to_string());
            selector
        }
        None => {
            match labels.get(APP_LABEL) {
                Some(v) if v != name && kind.has_app_selector() => {
                    return Err(format!(
                        "labels.{APP_LABEL} is the selector label and must equal the workload name ({name:?}); set selector to select pods by other labels"
                    ))
                }
                Some(_) => {}
                None => {
                    labels.insert(APP_LABEL.to_string(), name.to_string());
                }
            }
            BTreeMap::from([(APP_LABEL.to_string(), name.to_string())])
        }
    };
    if selector.contains_key(MANAGED_BY_LABEL) {
        return Err(format!("{MANAGED_BY_LABEL} can't be part of the selector"));
    }
    labels.insert(MANAGED_BY_LABEL.to_string(), MANAGED_BY_VALUE.to_string());
    let annotations = string_map(args, "annotations")?;
    let pod_annotations = string_map(args, "pod_annotations")?;
    if kind == WorkloadKind::Pod && pod_annotations.is_some() {
        return Err("create_pod takes annotations, not pod_annotations".to_string());
    }
    Ok(TemplateMeta {
        template: ObjectMeta {
            labels: Some(labels.clone()),
            annotations: pod_annotations,
            ..Default::default()
        },
        labels,
        annotations,
        selector,
    })
}

/// Build the pod template for a workload (metadata + spec).
pub fn build_template(
    args: &Value,
    name: &str,
    kind: WorkloadKind,
    policy: &WorkloadPolicy,
) -> Result<(TemplateMeta, PodTemplateSpec), String> {
    let meta = build_meta(args, name, kind)?;
    let spec = build_pod_spec(args, name, kind, policy)?;
    let template = PodTemplateSpec {
        metadata: Some(meta.template.clone()),
        spec: Some(spec),
    };
    Ok((meta, template))
}

// ---------------------------------------------------------------------------
// Update patches
// ---------------------------------------------------------------------------

/// Pick the container an update targets: `requested`, else the container
/// named after the workload, else the only container.
pub fn resolve_container(
    spec: Option<&PodSpec>,
    requested: Option<&str>,
    workload: &str,
) -> Result<String, String> {
    let names: Vec<&str> = spec
        .map(|s| s.containers.iter().map(|c| c.name.as_str()).collect())
        .unwrap_or_default();
    if let Some(r) = requested {
        if names.contains(&r) {
            return Ok(r.to_string());
        }
        return Err(format!(
            "container {r:?} not found (containers: {})",
            names.join(", ")
        ));
    }
    if names.contains(&workload) {
        return Ok(workload.to_string());
    }
    match names.as_slice() {
        [only] => Ok(only.to_string()),
        _ => Err(format!(
            "this workload has several containers ({}); say which one with container",
            names.join(", ")
        )),
    }
}

/// Keys an update may carry in addition to the creation-time pod fields.
pub const UPDATE_EXTRA_KEYS: &[&str] = &["container"];

/// Whether the update arguments touch any container field.
pub fn touches_container(args: &Value) -> bool {
    CONTAINER_SHORTHAND_KEYS
        .iter()
        .any(|k| args.get(*k).is_some())
        || [
            "privileged",
            "allow_privilege_escalation",
            "capabilities",
            "read_only_root_filesystem",
        ]
        .iter()
        .any(|k| args["security_context"].get(*k).is_some())
}

/// Build a strategic-merge patch for a pod template from update arguments.
///
/// Uses the same parsing as creation, so every field means the same thing;
/// only the fields the caller gave appear in the patch (no defaults are
/// added). Container fields go to `target` (see [`resolve_container`]);
/// `containers` / `init_containers` entries merge by name. Returns `None`
/// when nothing in the template changes.
pub fn build_template_patch(args: &Value, target: Option<&str>) -> Result<Option<Value>, String> {
    let mut spec = Map::new();

    let mut containers: Vec<Value> = Vec::new();
    if let Some(target) = target {
        let mut c = shorthand_container(args, target)?.unwrap_or_else(|| ContainerArgs {
            name: Some(target.to_string()),
            ..Default::default()
        });
        // Container-level fields of the top-level security_context apply to
        // the target container.
        if let Some(s) = opt::<SecurityArgs>(args, "security_context")? {
            let scoped = SecurityArgs {
                privileged: s.privileged,
                allow_privilege_escalation: s.allow_privilege_escalation,
                capabilities: s.capabilities.clone(),
                read_only_root_filesystem: s.read_only_root_filesystem,
                ..Default::default()
            };
            if scoped.privileged.is_some()
                || scoped.allow_privilege_escalation.is_some()
                || scoped.capabilities.is_some()
                || scoped.read_only_root_filesystem.is_some()
            {
                c.security_context = Some(scoped);
            }
        }
        let built = container(c, "arguments", false)?;
        let value = serde_json::to_value(&built).map_err(|e| e.to_string())?;
        if value.as_object().is_some_and(|o| o.len() > 1) {
            containers.push(value);
        }
    }
    if let Some(list) = container_list(args, "containers", false)? {
        for c in list {
            containers.push(serde_json::to_value(&c).map_err(|e| e.to_string())?);
        }
    }
    if !containers.is_empty() {
        spec.insert("containers".into(), Value::Array(containers));
    }
    if let Some(list) = container_list(args, "init_containers", false)? {
        let items = list
            .iter()
            .map(serde_json::to_value)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        spec.insert("initContainers".into(), Value::Array(items));
    }

    // Pod-level fields: build them exactly like creation, then copy the ones
    // the caller set.
    let mut pod_args = args.clone();
    if let Some(o) = pod_args.as_object_mut() {
        o.remove("restart_policy");
    }
    let fields = pod_fields(&pod_args, WorkloadKind::Pod)?;
    let mut pod = serde_json::to_value(&fields.spec).map_err(|e| e.to_string())?;
    if let Some(o) = pod.as_object_mut() {
        o.remove("restartPolicy");
        o.remove("containers");
        if let Some(sc) = o.get("securityContext") {
            if sc.as_object().is_some_and(Map::is_empty) {
                o.remove("securityContext");
            }
        }
        for (k, v) in std::mem::take(o) {
            spec.insert(k, v);
        }
    }

    let mut patch = Map::new();
    if let Some(annotations) = string_map(args, "pod_annotations")? {
        patch.insert("metadata".into(), json!({ "annotations": annotations }));
    }
    if !spec.is_empty() {
        patch.insert("spec".into(), Value::Object(spec));
    }
    Ok(if patch.is_empty() {
        None
    } else {
        Some(Value::Object(patch))
    })
}

// ---------------------------------------------------------------------------
// Schemas
// ---------------------------------------------------------------------------

/// Probe argument schema (`liveness_probe` etc.).
pub fn probe_schema(description: &str) -> Value {
    json!({
        "type": "object",
        "description": description,
        "properties": {
            "probe_type": { "type": "string", "enum": ["httpGet", "tcpSocket", "exec", "grpc"], "description": "Probe mechanism" },
            "path": { "type": "string", "description": "HTTP path for httpGet probes (e.g. \"/healthz\")" },
            "port": { "type": ["integer", "string"], "description": "Port number or named container port for httpGet/tcpSocket/grpc probes (default: 80; grpc needs a number)" },
            "scheme": { "type": "string", "enum": ["HTTP", "HTTPS"], "description": "Scheme for httpGet probes (default: HTTP)" },
            "http_headers": { "type": "array", "description": "Extra HTTP headers for httpGet probes", "items": { "type": "object", "properties": { "name": { "type": "string" }, "value": { "type": "string" } }, "required": ["name", "value"], "additionalProperties": false } },
            "command": { "type": "array", "items": { "type": "string" }, "description": "Command and args for exec probes" },
            "service": { "type": "string", "description": "gRPC health service name for grpc probes" },
            "initial_delay_seconds": { "type": "integer", "description": "Seconds to wait before the first probe (default: 0)" },
            "period_seconds": { "type": "integer", "description": "How often to probe in seconds (default: 10)" },
            "timeout_seconds": { "type": "integer", "description": "Seconds after which the probe times out (default: 1)" },
            "failure_threshold": { "type": "integer", "description": "Consecutive failures before the probe fails (default: 3)" },
            "success_threshold": { "type": "integer", "description": "Consecutive successes required after a failure (default: 1)" },
            "termination_grace_period_seconds": { "type": "integer", "description": "Grace period when a liveness/startup probe fails" }
        },
        "required": ["probe_type"],
        "additionalProperties": false
    })
}

fn handler_schema(description: &str) -> Value {
    json!({
        "type": "object",
        "description": description,
        "properties": {
            "handler_type": { "type": "string", "enum": ["exec", "httpGet", "sleep"], "description": "Hook mechanism" },
            "command": { "type": "array", "items": { "type": "string" }, "description": "Command for exec hooks" },
            "path": { "type": "string", "description": "HTTP path for httpGet hooks" },
            "port": { "type": ["integer", "string"], "description": "Port for httpGet hooks (default: 80)" },
            "scheme": { "type": "string", "enum": ["HTTP", "HTTPS"] },
            "seconds": { "type": "integer", "description": "Duration for sleep hooks" }
        },
        "required": ["handler_type"],
        "additionalProperties": false
    })
}

fn key_ref_schema(what: &str) -> Value {
    json!({
        "type": "object",
        "description": format!("Read the value from a key of a {what} in the same namespace"),
        "properties": {
            "name": { "type": "string" },
            "key": { "type": "string" },
            "optional": { "type": "boolean" }
        },
        "required": ["name", "key"],
        "additionalProperties": false
    })
}

fn env_source_properties() -> Value {
    json!({
        "secret_key_ref": key_ref_schema("Secret"),
        "config_map_key_ref": key_ref_schema("ConfigMap"),
        "field_ref": {
            "type": "object",
            "description": "Pod field, e.g. metadata.name, metadata.namespace, status.podIP, spec.nodeName",
            "properties": { "field_path": { "type": "string" }, "api_version": { "type": "string" } },
            "required": ["field_path"],
            "additionalProperties": false
        },
        "resource_field_ref": {
            "type": "object",
            "description": "Container resource, e.g. limits.cpu, requests.memory",
            "properties": { "resource": { "type": "string" }, "container_name": { "type": "string" }, "divisor": { "type": "string" } },
            "required": ["resource"],
            "additionalProperties": false
        }
    })
}

fn env_schema() -> Value {
    let source = json!({
        "type": "object",
        "properties": env_source_properties(),
        "additionalProperties": false
    });
    json!({
        "type": ["object", "array"],
        "description": "Environment variables. Either an object of NAME: value pairs, where a value may also be a source object such as {\"secret_key_ref\": {\"name\": \"db\", \"key\": \"password\"}}, or an array of {name, value} / {name, value_from: {secret_key_ref | config_map_key_ref | field_ref | resource_field_ref}}.",
        "additionalProperties": { "type": ["string", "object"], "properties": env_source_properties() },
        "items": {
            "type": "object",
            "properties": { "name": { "type": "string" }, "value": { "type": "string" }, "value_from": source },
            "required": ["name"],
            "additionalProperties": false
        }
    })
}

fn env_from_schema() -> Value {
    json!({
        "type": "array",
        "description": "Load every key of a Secret or ConfigMap as environment variables. Each item sets exactly one of secret_ref or config_map_ref (the resource name).",
        "items": {
            "type": "object",
            "properties": {
                "secret_ref": { "type": "string", "description": "Name of a Secret" },
                "config_map_ref": { "type": "string", "description": "Name of a ConfigMap" },
                "prefix": { "type": "string", "description": "Prefix added to every variable name" },
                "optional": { "type": "boolean" }
            },
            "additionalProperties": false
        }
    })
}

fn ports_schema() -> Value {
    json!({
        "type": "array",
        "description": "Container ports: numbers, or {container_port, name, protocol}. host_port is rejected unless the server allows host ports.",
        "items": {
            "type": ["object", "integer"],
            "properties": {
                "container_port": { "type": "integer" },
                "name": { "type": "string", "description": "Port name (referenced by Services and probes)" },
                "protocol": { "type": "string", "enum": ["TCP", "UDP", "SCTP"] },
                "host_port": { "type": "integer", "description": "Gated: needs --allow-host-ports" },
                "host_ip": { "type": "string" }
            },
            "required": ["container_port"],
            "additionalProperties": false
        }
    })
}

fn resources_schema() -> Value {
    let quantities = json!({
        "type": "object",
        "description": "Resource quantities, e.g. {\"cpu\": \"250m\", \"memory\": \"256Mi\", \"ephemeral-storage\": \"1Gi\", \"nvidia.com/gpu\": \"1\"}",
        "additionalProperties": { "type": ["string", "number"] }
    });
    json!({
        "type": "object",
        "description": "Compute resources: requests and limits",
        "properties": { "requests": quantities, "limits": quantities },
        "additionalProperties": false
    })
}

fn volume_mounts_schema() -> Value {
    json!({
        "type": "array",
        "description": "Volume mounts (each name must match an entry in volumes)",
        "items": {
            "type": "object",
            "properties": {
                "name": { "type": "string" },
                "mount_path": { "type": "string" },
                "sub_path": { "type": "string" },
                "sub_path_expr": { "type": "string" },
                "read_only": { "type": "boolean" },
                "mount_propagation": { "type": "string", "enum": ["None", "HostToContainer", "Bidirectional"] }
            },
            "required": ["name", "mount_path"],
            "additionalProperties": false
        }
    })
}

fn profile_schema(what: &str) -> Value {
    json!({
        "type": ["string", "object"],
        "description": format!("{what} profile: \"RuntimeDefault\", \"Localhost\" (with localhost_profile) or \"Unconfined\" (gated: needs --allow-unconfined)"),
        "properties": {
            "type": { "type": "string", "enum": ["RuntimeDefault", "Localhost", "Unconfined"] },
            "localhost_profile": { "type": "string" }
        },
        "additionalProperties": false
    })
}

fn container_security_properties() -> Map<String, Value> {
    let v = json!({
        "run_as_non_root": { "type": "boolean", "description": "Refuse to start the container as UID 0" },
        "run_as_user": { "type": "integer" },
        "run_as_group": { "type": "integer" },
        "read_only_root_filesystem": { "type": "boolean" },
        "allow_privilege_escalation": { "type": "boolean", "description": "Default false. true is gated: needs --allow-privilege-escalation" },
        "privileged": { "type": "boolean", "description": "Gated: needs --allow-privileged" },
        "capabilities": {
            "type": "object",
            "description": "Linux capabilities. drop defaults to [\"ALL\"] for non-root containers. add accepts the runtime's default set (AUDIT_WRITE, CHOWN, DAC_OVERRIDE, FOWNER, FSETID, KILL, MKNOD, NET_BIND_SERVICE, SETFCAP, SETGID, SETPCAP, SETUID, SYS_CHROOT); others need --allow-capabilities",
            "properties": {
                "add": { "type": "array", "items": { "type": "string" } },
                "drop": { "type": "array", "items": { "type": "string" } }
            },
            "additionalProperties": false
        },
        "seccomp_profile": profile_schema("Seccomp"),
        "app_armor_profile": profile_schema("AppArmor")
    });
    v.as_object().cloned().unwrap_or_default()
}

fn container_security_schema() -> Value {
    json!({
        "type": "object",
        "description": "Container securityContext (overrides the top-level security_context for this container)",
        "properties": container_security_properties(),
        "additionalProperties": false
    })
}

/// The top-level `security_context` schema: pod-level settings plus the
/// container defaults.
fn pod_security_schema() -> Value {
    let mut props = container_security_properties();
    props.insert("run_as_non_root".into(), json!({ "type": "boolean", "description": "Default true (with secure defaults on). Set false for images that must run as root; they then also keep the runtime's default capabilities" }));
    props.insert(
        "fs_group".into(),
        json!({ "type": "integer", "description": "Group that owns mounted volumes" }),
    );
    props.insert(
        "fs_group_change_policy".into(),
        json!({ "type": "string", "enum": ["OnRootMismatch", "Always"] }),
    );
    props.insert(
        "supplemental_groups".into(),
        json!({ "type": "array", "items": { "type": "integer" } }),
    );
    props.insert("sysctls".into(), json!({
        "type": "array",
        "description": "Namespaced sysctls. Only the safe set is allowed without --allow-unconfined",
        "items": { "type": "object", "properties": { "name": { "type": "string" }, "value": { "type": "string" } }, "required": ["name", "value"], "additionalProperties": false }
    }));
    json!({
        "type": "object",
        "description": "Security settings for the whole pod. run_as_*, fs_group, seccomp_profile, app_armor_profile, supplemental_groups and sysctls go to the pod securityContext; privileged, allow_privilege_escalation, capabilities and read_only_root_filesystem are defaults for every container. With secure defaults on (the default), unset fields become runAsNonRoot: true, seccompProfile RuntimeDefault, allowPrivilegeEscalation: false and capabilities.drop [ALL].",
        "properties": props,
        "additionalProperties": false
    })
}

/// Properties of one container (used for `containers[]` / `init_containers[]`
/// items and, without `name`, for the single-container shorthand).
fn container_properties() -> Map<String, Value> {
    let v = json!({
        "image": { "type": "string", "description": "Container image" },
        "command": { "type": "array", "items": { "type": "string" }, "description": "Entrypoint (replaces the image's ENTRYPOINT)" },
        "args": { "type": "array", "items": { "type": "string" }, "description": "Arguments (replace the image's CMD)" },
        "working_dir": { "type": "string", "description": "Working directory" },
        "image_pull_policy": { "type": "string", "enum": ["Always", "IfNotPresent", "Never"], "description": "When to pull the image (Kubernetes default: IfNotPresent, Always for :latest)" },
        "env": env_schema(),
        "env_from": env_from_schema(),
        "ports": ports_schema(),
        "resources": resources_schema(),
        "volume_mounts": volume_mounts_schema(),
        "liveness_probe": probe_schema("Liveness probe"),
        "readiness_probe": probe_schema("Readiness probe"),
        "startup_probe": probe_schema("Startup probe"),
        "lifecycle": {
            "type": "object",
            "description": "Lifecycle hooks",
            "properties": {
                "post_start": handler_schema("Runs right after the container starts"),
                "pre_stop": handler_schema("Runs before the container is stopped")
            },
            "additionalProperties": false
        }
    });
    v.as_object().cloned().unwrap_or_default()
}

fn container_item_schema(init: bool) -> Value {
    let mut props = container_properties();
    props.insert(
        "name".into(),
        json!({ "type": "string", "description": "Container name (unique in the pod)" }),
    );
    props.insert("security_context".into(), container_security_schema());
    if init {
        props.insert("restart_policy".into(), json!({ "type": "string", "enum": ["Always"], "description": "Always makes this init container a native sidecar that keeps running next to the main containers" }));
    }
    json!({
        "type": "object",
        "properties": props,
        "required": ["name", "image"],
        "additionalProperties": false
    })
}

fn volume_schema() -> Value {
    let key_items = json!({
        "type": "array",
        "items": { "type": "object", "properties": { "key": { "type": "string" }, "path": { "type": "string" }, "mode": { "type": "integer" } }, "required": ["key", "path"], "additionalProperties": false }
    });
    let downward_items = json!({
        "type": "array",
        "items": {
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                "field_ref": { "type": "object", "properties": { "field_path": { "type": "string" } }, "required": ["field_path"], "additionalProperties": false },
                "resource_field_ref": { "type": "object", "properties": { "resource": { "type": "string" }, "container_name": { "type": "string" }, "divisor": { "type": "string" } }, "required": ["resource"], "additionalProperties": false },
                "mode": { "type": "integer" }
            },
            "required": ["path"],
            "additionalProperties": false
        }
    });
    json!({
        "type": "object",
        "description": "A volume: name plus exactly one source. config_map, secret, empty_dir, persistent_volume_claim, projected and downward_api are always allowed; host_path needs --allow-host-path; nfs and csi need --allow-volume-types.",
        "properties": {
            "name": { "type": "string" },
            "config_map": { "type": "object", "properties": { "name": { "type": "string" }, "items": key_items, "default_mode": { "type": "integer" }, "optional": { "type": "boolean" } }, "required": ["name"], "additionalProperties": false },
            "secret": { "type": "object", "properties": { "secret_name": { "type": "string" }, "items": key_items, "default_mode": { "type": "integer" }, "optional": { "type": "boolean" } }, "required": ["secret_name"], "additionalProperties": false },
            "empty_dir": { "type": "object", "properties": { "medium": { "type": "string", "enum": ["", "Memory"] }, "size_limit": { "type": "string", "description": "e.g. \"1Gi\"" } }, "additionalProperties": false },
            "persistent_volume_claim": { "type": "object", "properties": { "claim_name": { "type": "string" }, "read_only": { "type": "boolean" } }, "required": ["claim_name"], "additionalProperties": false },
            "projected": {
                "type": "object",
                "properties": {
                    "sources": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "config_map": { "type": "object", "properties": { "name": { "type": "string" }, "items": key_items, "optional": { "type": "boolean" } }, "required": ["name"], "additionalProperties": false },
                                "secret": { "type": "object", "properties": { "name": { "type": "string" }, "items": key_items, "optional": { "type": "boolean" } }, "required": ["name"], "additionalProperties": false },
                                "service_account_token": { "type": "object", "properties": { "path": { "type": "string" }, "audience": { "type": "string" }, "expiration_seconds": { "type": "integer" } }, "required": ["path"], "additionalProperties": false },
                                "downward_api": { "type": "object", "properties": { "items": downward_items }, "required": ["items"], "additionalProperties": false }
                            },
                            "additionalProperties": false
                        }
                    },
                    "default_mode": { "type": "integer" }
                },
                "required": ["sources"],
                "additionalProperties": false
            },
            "downward_api": { "type": "object", "properties": { "items": downward_items, "default_mode": { "type": "integer" } }, "required": ["items"], "additionalProperties": false },
            "host_path": { "type": "object", "description": "Gated: needs --allow-host-path", "properties": { "path": { "type": "string" }, "type": { "type": "string" } }, "required": ["path"], "additionalProperties": false },
            "nfs": { "type": "object", "description": "Gated: needs --allow-volume-types nfs", "properties": { "server": { "type": "string" }, "path": { "type": "string" }, "read_only": { "type": "boolean" } }, "required": ["server", "path"], "additionalProperties": false },
            "csi": { "type": "object", "description": "Inline CSI volume. Gated: needs --allow-volume-types csi", "properties": { "driver": { "type": "string" }, "read_only": { "type": "boolean" }, "volume_attributes": { "type": "object", "additionalProperties": { "type": "string" } }, "fs_type": { "type": "string" }, "node_publish_secret_ref": { "type": "string" } }, "required": ["driver"], "additionalProperties": false }
        },
        "required": ["name"],
        "additionalProperties": false
    })
}

fn string_map_schema(description: &str) -> Value {
    json!({ "type": "object", "description": description, "additionalProperties": { "type": "string" } })
}

/// Pod-level properties shared by every workload tool.
fn pod_properties(kind: WorkloadKind) -> Map<String, Value> {
    let restart = match kind {
        WorkloadKind::Pod => {
            json!({ "type": "string", "enum": ["Never", "OnFailure", "Always"], "description": "Restart policy (default: Never)" })
        }
        WorkloadKind::Job => {
            json!({ "type": "string", "enum": ["Never", "OnFailure"], "description": "Pod restart policy (default: Never)" })
        }
        WorkloadKind::CronJob => {
            json!({ "type": "string", "enum": ["Never", "OnFailure"], "description": "Pod restart policy (default: OnFailure)" })
        }
        _ => Value::Null,
    };
    let v = json!({
        "containers": { "type": "array", "description": "Containers, for multi-container pods. Use either this or the single-container arguments (image, command, …).", "items": container_item_schema(false) },
        "init_containers": { "type": "array", "description": "Init containers, run to completion before the main containers. restart_policy: Always makes one a native sidecar.", "items": container_item_schema(true) },
        "container_name": { "type": "string", "description": "Name of the single container (default: the workload name)" },
        "volumes": { "type": "array", "description": "Pod volumes, mounted with volume_mounts", "items": volume_schema() },
        "service_account_name": { "type": "string", "description": "Service account the pods run as. When unset, the service account token is not mounted (automountServiceAccountToken: false)" },
        "automount_service_account_token": { "type": "boolean", "description": "Mount the service account token (default: false unless service_account_name is set)" },
        "image_pull_secrets": { "type": "array", "items": { "type": "string" }, "description": "Names of image pull Secrets" },
        "node_selector": string_map_schema("Node labels the pods must match"),
        "tolerations": {
            "type": "array",
            "description": "Taints the pods tolerate",
            "items": {
                "type": "object",
                "properties": {
                    "key": { "type": "string" },
                    "operator": { "type": "string", "enum": ["Equal", "Exists"] },
                    "value": { "type": "string" },
                    "effect": { "type": "string", "enum": ["NoSchedule", "PreferNoSchedule", "NoExecute"] },
                    "toleration_seconds": { "type": "integer" }
                },
                "additionalProperties": false
            }
        },
        "affinity": { "type": "object", "description": "Kubernetes Affinity object (nodeAffinity, podAffinity, podAntiAffinity) as in the API; snake_case keys are accepted too" },
        "topology_spread_constraints": { "type": "array", "description": "Kubernetes TopologySpreadConstraint objects (maxSkew, topologyKey, whenUnsatisfiable, labelSelector, …)", "items": { "type": "object" } },
        "priority_class_name": { "type": "string", "description": "PriorityClass for scheduling and preemption" },
        "termination_grace_period_seconds": { "type": "integer", "description": "Seconds to wait after SIGTERM before killing the pod (default: 30)" },
        "dns_policy": { "type": "string", "enum": ["ClusterFirst", "ClusterFirstWithHostNet", "Default", "None"], "description": "DNS policy (default: ClusterFirst; None requires dns_config)" },
        "dns_config": { "type": "object", "description": "{nameservers, searches, options: [{name, value}]}" },
        "host_aliases": { "type": "array", "description": "Extra /etc/hosts entries", "items": { "type": "object", "properties": { "ip": { "type": "string" }, "hostnames": { "type": "array", "items": { "type": "string" } } }, "required": ["ip", "hostnames"], "additionalProperties": false } },
        "runtime_class_name": { "type": "string", "description": "RuntimeClass, e.g. a sandboxed runtime such as gvisor or kata" },
        "hostname": { "type": "string", "description": "Pod hostname (default: the pod name)" },
        "subdomain": { "type": "string", "description": "Subdomain for <hostname>.<subdomain>.<namespace>.svc DNS records (a headless Service of that name)" },
        "share_process_namespace": { "type": "boolean", "description": "Containers share one process namespace (e.g. for debugging sidecars)" },
        "enable_service_links": { "type": "boolean", "description": "Inject environment variables for every Service in the namespace (Kubernetes default: true)" },
        "host_network": { "type": "boolean", "description": "Gated: needs --allow-host-namespaces" },
        "host_pid": { "type": "boolean", "description": "Gated: needs --allow-host-namespaces" },
        "host_ipc": { "type": "boolean", "description": "Gated: needs --allow-host-namespaces" },
        "security_context": pod_security_schema(),
        "labels": string_map_schema("Labels for the object and its pods (app and app.kubernetes.io/managed-by are set automatically)"),
        "annotations": string_map_schema("Annotations for the object")
    });
    let mut props = v.as_object().cloned().unwrap_or_default();
    if !restart.is_null() {
        props.insert("restart_policy".into(), restart);
    }
    if kind != WorkloadKind::Pod {
        props.insert(
            "pod_annotations".into(),
            string_map_schema("Annotations for the pod template (e.g. prometheus.io/scrape)"),
        );
    }
    if kind.has_app_selector() {
        props.insert(
            "selector".into(),
            string_map_schema("Pod selector labels (default: {app: <name>}); added to the pod labels. Immutable after creation"),
        );
    }
    props
}

/// Assemble a workload tool's input schema: `namespace`, `name`, the
/// single-container shorthand, pod fields, and the tool's own properties.
pub fn workload_schema(kind: WorkloadKind, extra: Value, required: &[&str]) -> Value {
    let mut props = Map::new();
    props.insert(
        "namespace".into(),
        json!({ "type": "string", "description": "Kubernetes namespace" }),
    );
    props.insert(
        "name".into(),
        json!({ "type": "string", "description": "Object name" }),
    );
    props.extend(container_properties());
    props.insert("port".into(), json!({ "type": "integer", "description": "Single container port to expose (shorthand for ports: [port])" }));
    props.extend(pod_properties(kind));
    if let Some(extra) = extra.as_object() {
        props.extend(extra.clone());
    }
    json!({
        "type": "object",
        "properties": props,
        "required": required,
        "additionalProperties": false
    })
}

/// Input schema for a workload update tool: the same fields, all optional,
/// plus `container` to pick the container the single-container fields change.
pub fn workload_update_schema(kind: WorkloadKind, extra: Value) -> Value {
    let mut schema = workload_schema(kind, extra, &["namespace", "name"]);
    if let Some(props) = schema["properties"].as_object_mut() {
        for key in [
            "labels",
            "annotations",
            "restart_policy",
            "container_name",
            "selector",
        ] {
            props.remove(key);
        }
        props.insert(
            "container".into(),
            json!({ "type": "string", "description": "Container the single-container fields (image, env, …) apply to. Default: the container named after the workload, or the only container" }),
        );
    }
    schema
}

#[cfg(test)]
mod tests;
