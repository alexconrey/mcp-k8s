//! Tool-call classification, target extraction and allowlist redaction.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use serde_json::{Map, Value};

use crate::permissions::{Action, ActionPermissions};

/// The longest string value telemetry records; longer allowlisted values are
/// cut. Kubernetes object names are at most 253 bytes.
pub(crate) const MAX_VALUE_LEN: usize = 256;
/// At most this many argument keys are recorded per call.
const MAX_KEYS: usize = 32;
/// Manifests passed as a JSON string are parsed for their target only up to
/// this size.
const MAX_MANIFEST_LEN: usize = 4 * 1024 * 1024;

/// Top-level argument keys whose *scalar* values (string, number or bool)
/// telemetry may record, with the reason each is safe. Every other value is
/// dropped; only its key is recorded.
///
/// This is the redaction guarantee: Secret `data`/`stringData`, env values,
/// exec commands, manifest and patch bodies, label and annotation values,
/// selectors and plugin config never reach a span, metric or log. The canary
/// test in `telemetry/tests.rs` enforces it for every tool in
/// `tool_definitions()`.
pub const ALLOWED_ARGUMENTS: &[(&str, &str)] = &[
    ("namespace", "the namespace the call targets"),
    (
        "name",
        "the object the call targets (or the cluster context of switch_cluster)",
    ),
    ("kind", "the kind the call targets"),
    ("api_version", "the API version the call targets"),
    (
        "group",
        "API group of a custom-resource call; with `version` it is the target's api_version",
    ),
    ("version", "API version of a custom-resource call"),
    ("replicas", "a desired replica count"),
    ("image", "a container image reference"),
    ("container", "a container name"),
    ("pod_name", "the target object name of get_pod_logs, like `name`"),
    ("job_name", "the target object name of get_build_logs, like `name`"),
    (
        "cluster",
        "the kubeconfig context a call targets (embedders' multi-cluster tools)",
    ),
    (
        "decode",
        "whether get_secret was asked for decoded values: audit-relevant, and a boolean carries no data",
    ),
];

/// What a tool does, as recorded in `mcp_k8s.action`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ActionClass {
    /// Reads cluster state.
    Read,
    /// Creates or changes cluster state (create, update, scale, restart, …).
    Write,
    /// Deletes or evicts.
    Delete,
    /// Runs a command inside a workload (`exec_pod`).
    Exec,
}

impl ActionClass {
    /// Classify an mcp-k8s tool from [`ActionPermissions::action_for_tool`],
    /// the same classification the permission check uses: `Read` is `read`,
    /// `Create` and `Update` are `write`, `Delete` is `delete`. `exec_*` tools
    /// are labelled `exec` instead, a refinement of the permission action
    /// they are gated by (`update` once `action_for_tool` classifies
    /// `exec_pod` as one), so the decision and the label never disagree on
    /// whether a call mutates the cluster.
    ///
    /// Embedders whose tool names don't follow mcp-k8s's prefixes
    /// (`trigger_build`, `enable_plugin`, …) should pick the class themselves
    /// with [`super::ToolCall::start_with_action`].
    pub fn for_tool(tool: &str) -> Self {
        match ActionPermissions::action_for_tool(tool) {
            _ if tool.starts_with("exec_") => Self::Exec,
            Action::Read => Self::Read,
            Action::Create | Action::Update => Self::Write,
            Action::Delete => Self::Delete,
        }
    }

    /// The attribute value: `read`, `write`, `delete` or `exec`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
            Self::Delete => "delete",
            Self::Exec => "exec",
        }
    }
}

impl std::fmt::Display for ActionClass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The Kubernetes object a tool call targets, where its arguments say.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Target {
    /// From a `cluster` argument. The request handler records the active
    /// cluster separately.
    pub cluster: Option<String>,
    /// From `namespace`, else a manifest's `metadata.namespace`.
    pub namespace: Option<String>,
    /// From `kind`, else a manifest's `kind`.
    pub kind: Option<String>,
    /// From `name`, `pod_name` or `job_name`, else a manifest's `metadata.name`.
    pub name: Option<String>,
    /// From `api_version`, else `group`/`version`, else a manifest's
    /// `apiVersion`.
    pub api_version: Option<String>,
}

/// What telemetry records about a tool call's arguments. Build it with
/// [`summarize`].
#[derive(Clone, Debug, PartialEq)]
pub struct ToolCallSummary {
    /// The action class.
    pub action: ActionClass,
    /// The object the call targets.
    pub target: Target,
    /// The top-level argument keys, sorted. For mcp-k8s tools only keys the
    /// tool's input schema declares are kept; others are counted as `?`.
    pub argument_keys: Vec<String>,
    /// The allowlisted scalar argument values ([`ALLOWED_ARGUMENTS`]).
    pub arguments: Map<String, Value>,
}

impl ToolCallSummary {
    /// [`Self::argument_keys`] as one comma-separated string.
    pub fn argument_keys_string(&self) -> String {
        self.argument_keys.join(",")
    }

    /// [`Self::arguments`] as a compact JSON object, or `None` if empty.
    pub fn arguments_json(&self) -> Option<String> {
        if self.arguments.is_empty() {
            None
        } else {
            serde_json::to_string(&self.arguments).ok()
        }
    }
}

/// Classify a tool call and redact its arguments for telemetry.
///
/// Returns the action class, the target (cluster, namespace, kind, name and
/// api_version, where the arguments have them) and an argument summary: the
/// top-level argument **keys**, plus the values of the safe scalars in
/// [`ALLOWED_ARGUMENTS`]. No other argument value is ever returned: not
/// Secret data, env values, commands, manifest or patch bodies, label or
/// annotation values, selectors or plugin config.
///
/// For `apply_manifest` and `create_custom_resource` the target's kind,
/// apiVersion, name and namespace come from the manifest (a JSON object, or a
/// string holding one). Nothing else is read from it.
pub fn summarize(tool: &str, args: &Value) -> ToolCallSummary {
    let action = ActionClass::for_tool(tool);
    let Some(obj) = args.as_object() else {
        return ToolCallSummary {
            action,
            target: Target::default(),
            argument_keys: Vec::new(),
            arguments: Map::new(),
        };
    };

    let mut arguments = Map::new();
    for (key, _why) in ALLOWED_ARGUMENTS {
        if let Some(value) = obj.get(*key).and_then(safe_scalar) {
            arguments.insert((*key).to_string(), value);
        }
    }

    let declared = declared_keys(tool);
    let mut argument_keys: Vec<String> = Vec::new();
    let mut undeclared = 0usize;
    for key in obj.keys() {
        let ok = match declared {
            Some(set) => set.contains(key.as_str()),
            None => is_plain_key(key),
        };
        if ok {
            argument_keys.push(key.clone());
        } else {
            undeclared += 1;
        }
    }
    argument_keys.sort();
    argument_keys.truncate(MAX_KEYS);
    if undeclared > 0 {
        argument_keys.push("?".to_string());
    }

    let string_arg = |key: &str| obj.get(key).and_then(Value::as_str).map(clean);
    let mut target = Target {
        cluster: string_arg("cluster"),
        namespace: string_arg("namespace"),
        kind: string_arg("kind"),
        name: string_arg("name")
            .or_else(|| string_arg("pod_name"))
            .or_else(|| string_arg("job_name")),
        api_version: string_arg("api_version").or_else(|| {
            let version = string_arg("version")?;
            match string_arg("group") {
                Some(group) if !group.is_empty() => Some(clean(&format!("{group}/{version}"))),
                _ => Some(version),
            }
        }),
    };
    if let Some(manifest) = obj.get("manifest") {
        fill_from_manifest(&mut target, manifest);
    }

    ToolCallSummary {
        action,
        target,
        argument_keys,
        arguments,
    }
}

/// Whether `tool` is one of the tools mcp-k8s serves (listed by
/// [`crate::mcp::tool_definitions`] with default permissions).
///
/// Use it to decide whether a client-supplied tool name is safe to put in a
/// span name: an unknown name is unbounded client input.
pub fn is_known_tool(tool: &str) -> bool {
    tool_schemas().contains_key(tool)
}

/// Tool name → the top-level argument keys its input schema declares.
fn tool_schemas() -> &'static HashMap<String, HashSet<String>> {
    static SCHEMAS: OnceLock<HashMap<String, HashSet<String>>> = OnceLock::new();
    SCHEMAS.get_or_init(|| {
        // Every tool, including apply_manifest, which is off by default but
        // still a known tool when an operator enables it.
        crate::mcp::tool_definitions(&ActionPermissions::default().with_apply_manifest(true))
            .iter()
            .filter_map(|tool| {
                let name = tool["name"].as_str()?.to_string();
                let keys = tool["inputSchema"]["properties"]
                    .as_object()
                    .map(|props| props.keys().cloned().collect())
                    .unwrap_or_default();
                Some((name, keys))
            })
            .collect()
    })
}

fn declared_keys(tool: &str) -> Option<&'static HashSet<String>> {
    tool_schemas().get(tool)
}

/// Keys of tools mcp-k8s doesn't know are kept only if they look like
/// identifiers, never free text.
fn is_plain_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 64
        && key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
}

/// A recordable copy of a scalar, or `None` for objects, arrays and null.
fn safe_scalar(value: &Value) -> Option<Value> {
    match value {
        Value::String(s) => Some(Value::String(clean(s))),
        Value::Number(_) | Value::Bool(_) => Some(value.clone()),
        _ => None,
    }
}

/// Drop control characters and cut to [`MAX_VALUE_LEN`] bytes.
pub(crate) fn clean(s: &str) -> String {
    let mut out = String::with_capacity(s.len().min(MAX_VALUE_LEN));
    for c in s.chars().filter(|c| !c.is_control()) {
        if out.len() + c.len_utf8() > MAX_VALUE_LEN {
            break;
        }
        out.push(c);
    }
    out
}

/// Fill the target from a manifest's `kind`, `apiVersion`, `metadata.name`
/// and `metadata.namespace`, without overriding explicit arguments. Nothing
/// else in the manifest is read.
fn fill_from_manifest(target: &mut Target, manifest: &Value) {
    let parsed;
    let manifest = match manifest {
        Value::Object(_) => manifest,
        Value::String(s) if s.len() <= MAX_MANIFEST_LEN && s.trim_start().starts_with('{') => {
            match serde_json::from_str::<Value>(s) {
                Ok(v) => {
                    parsed = v;
                    &parsed
                }
                Err(_) => return,
            }
        }
        _ => return,
    };
    let field = |v: &Value| v.as_str().map(clean);
    if target.kind.is_none() {
        target.kind = field(&manifest["kind"]);
    }
    if target.api_version.is_none() {
        target.api_version = field(&manifest["apiVersion"]);
    }
    if target.name.is_none() {
        target.name = field(&manifest["metadata"]["name"]);
    }
    if target.namespace.is_none() {
        target.namespace = field(&manifest["metadata"]["namespace"]);
    }
}
