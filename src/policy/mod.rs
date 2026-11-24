//! Workload guardrail policy.
//!
//! Every write path in mcp-k8s runs the same checks: the typed `create_*` and
//! `update_*` tools (on the object they built, or on the server's dry-run
//! result for updates), `apply_manifest`, and `create_custom_resource` /
//! `update_custom_resource`. The checks work on the Kubernetes JSON form of an
//! object, so a typed tool and a raw manifest that describe the same object get
//! the same verdict. `apply_manifest` can't be used to get around a rule the
//! typed tools enforce.
//!
//! The defaults are restrictive. Each relaxation is an explicit opt-in on
//! [`WorkloadPolicy`] (library users) or a CLI flag / environment variable
//! (the `mcp-k8s` server), and every rejection names the flag that would allow
//! it.

mod admission;
mod manifest;
mod network;
mod pod;
mod rbac;

use std::fmt;

pub(crate) use admission::{
    admit_manifest, bound_role_violations, check_typed, patch_checked, AdmissionOrigin, PatchKind,
};
pub use manifest::{check_object, is_builtin_group, kind_not_allowed, Source};
pub use network::{check_ingress, check_service};
pub use pod::{check_pod_spec, normalize_image, BASELINE_CAPABILITIES, SAFE_VOLUME_TYPES};
pub use rbac::{check_binding, check_namespace_labels, check_rules, check_secret};

/// Label every object written by mcp-k8s carries.
pub const MANAGED_BY_LABEL: &str = "app.kubernetes.io/managed-by";
/// Value of [`MANAGED_BY_LABEL`].
pub const MANAGED_BY_VALUE: &str = "mcp-k8s";

/// Guardrails for objects written through mcp-k8s.
///
/// [`WorkloadPolicy::default()`] is restrictive: it rejects privileged
/// containers, privilege escalation, added capabilities beyond the container
/// runtime's default set, host namespaces, `hostPath` volumes and host ports,
/// risky volume types, unconfined seccomp/AppArmor profiles, NodePort and
/// LoadBalancer Services, ingress-nginx snippet annotations, RBAC escalation
/// primitives, and (for `apply_manifest` and the custom resource tools)
/// RBAC/admission/CRD/cluster-scoped kinds and custom resources.
///
/// Relax individual rules by setting fields (the struct is `#[non_exhaustive]`,
/// so start from `default()`):
///
/// ```
/// let mut policy = mcp_k8s::policy::WorkloadPolicy::default();
/// policy.allow_load_balancer = true;
/// policy.allowed_custom_resource_groups = vec!["cert-manager.io".into()];
/// ```
///
/// [`WorkloadPolicy::permissive()`] turns every guardrail off (mcp-k8s 0.4
/// behaviour).
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct WorkloadPolicy {
    /// Allow `securityContext.privileged: true` and Windows `hostProcess`
    /// containers, and namespaces labelled `pod-security.kubernetes.io/*=privileged`.
    pub allow_privileged: bool,
    /// Allow `securityContext.allowPrivilegeEscalation: true`.
    pub allow_privilege_escalation: bool,
    /// Allow `hostNetwork`, `hostPID` and `hostIPC`.
    pub allow_host_namespaces: bool,
    /// Allow `hostPath` volumes.
    pub allow_host_path: bool,
    /// Allow `hostPort` on container ports.
    pub allow_host_ports: bool,
    /// Allow seccomp/AppArmor `Unconfined`, custom SELinux options,
    /// `procMount: Unmasked`, sysctls outside the safe set, and probe or
    /// lifecycle-hook `host` values other than localhost.
    pub allow_unconfined: bool,
    /// Capabilities that may be added on top of [`BASELINE_CAPABILITIES`]
    /// (the container runtime's defaults, which are always allowed).
    /// `"ALL"` allows any capability.
    pub allowed_capabilities: Vec<String>,
    /// Volume types allowed on top of [`SAFE_VOLUME_TYPES`], by their
    /// Kubernetes field name (e.g. `"nfs"`, `"csi"`). `"*"` allows any type
    /// except `hostPath`, which needs [`Self::allow_host_path`].
    pub allowed_volume_types: Vec<String>,
    /// Image registry allowlist (e.g. `"ghcr.io/myorg"`, `"docker.io/library"`).
    /// Empty allows any registry.
    pub allowed_registries: Vec<String>,
    /// Allow `NodePort` Services (and explicit `nodePort` values).
    pub allow_node_port: bool,
    /// Allow `LoadBalancer` Services, `loadBalancer*` fields, and cloud
    /// load-balancer annotations.
    pub allow_load_balancer: bool,
    /// Allow `ExternalName` Services and `spec.externalIPs` (CVE-2020-8554).
    pub allow_external_services: bool,
    /// Allow `*-snippet` ingress annotations and nginx configuration syntax in
    /// ingress-nginx annotation values (the IngressNightmare class of attacks).
    pub allow_ingress_snippets: bool,
    /// If non-empty, only these Ingress annotation keys are accepted. A
    /// trailing `*` matches a prefix (e.g. `"nginx.ingress.kubernetes.io/*"`).
    pub ingress_annotation_allowlist: Vec<String>,
    /// Ingress annotation keys that are always rejected (same pattern syntax).
    pub ingress_annotation_denylist: Vec<String>,
    /// Kinds `apply_manifest` and the custom resource tools may write although
    /// they are rejected by default (RBAC, admission webhooks and policies,
    /// CRDs, APIServices, Endpoints/EndpointSlices, ControllerRevisions, and
    /// every cluster-scoped kind). Entries are `Kind` or `group/Kind`
    /// (`core/Kind` for the core group); `"*"` allows every kind.
    pub allowed_manifest_kinds: Vec<String>,
    /// API groups of custom resources that may be written (custom resources
    /// can create pods indirectly through operators). `"*"` allows any group.
    /// Empty (the default) rejects custom resource writes.
    pub allowed_custom_resource_groups: Vec<String>,
    /// Allow RBAC escalation primitives: `escalate`/`bind`/`impersonate` verbs,
    /// wildcard verbs/resources/apiGroups/nonResourceURLs, aggregation, binding
    /// `cluster-admin` (or a role with those rules), binding anonymous
    /// subjects, and service-account token Secrets.
    pub allow_rbac_escalation: bool,
    /// Typed create tools fill in a restricted securityContext (runAsNonRoot,
    /// seccomp RuntimeDefault, no privilege escalation, drop ALL capabilities)
    /// and `automountServiceAccountToken: false` when the caller doesn't set
    /// them. These are defaults, not rules.
    pub secure_defaults: bool,
}

impl Default for WorkloadPolicy {
    fn default() -> Self {
        Self {
            allow_privileged: false,
            allow_privilege_escalation: false,
            allow_host_namespaces: false,
            allow_host_path: false,
            allow_host_ports: false,
            allow_unconfined: false,
            allowed_capabilities: Vec::new(),
            allowed_volume_types: Vec::new(),
            allowed_registries: Vec::new(),
            allow_node_port: false,
            allow_load_balancer: false,
            allow_external_services: false,
            allow_ingress_snippets: false,
            ingress_annotation_allowlist: Vec::new(),
            ingress_annotation_denylist: Vec::new(),
            allowed_manifest_kinds: Vec::new(),
            allowed_custom_resource_groups: Vec::new(),
            allow_rbac_escalation: false,
            secure_defaults: true,
        }
    }
}

impl WorkloadPolicy {
    /// Every guardrail off and no secure defaults: the behaviour of
    /// mcp-k8s 0.4 and earlier. The namespace allowlist and
    /// [`crate::permissions::ActionPermissions`] still apply.
    pub fn permissive() -> Self {
        Self {
            allow_privileged: true,
            allow_privilege_escalation: true,
            allow_host_namespaces: true,
            allow_host_path: true,
            allow_host_ports: true,
            allow_unconfined: true,
            allowed_capabilities: vec!["ALL".to_string()],
            allowed_volume_types: vec!["*".to_string()],
            allowed_registries: Vec::new(),
            allow_node_port: true,
            allow_load_balancer: true,
            allow_external_services: true,
            allow_ingress_snippets: true,
            ingress_annotation_allowlist: Vec::new(),
            ingress_annotation_denylist: Vec::new(),
            allowed_manifest_kinds: vec!["*".to_string()],
            allowed_custom_resource_groups: vec!["*".to_string()],
            allow_rbac_escalation: true,
            secure_defaults: false,
        }
    }

    /// Whether `cap` (with or without a `CAP_` prefix, any case) may be added.
    pub fn capability_allowed(&self, cap: &str) -> bool {
        let cap = normalize_capability(cap);
        if cap != "ALL" && BASELINE_CAPABILITIES.contains(&cap.as_str()) {
            return true;
        }
        self.allowed_capabilities.iter().any(|allowed| {
            let allowed = normalize_capability(allowed);
            allowed == "ALL" || allowed == cap
        })
    }

    /// Whether a volume of type `field` (its Kubernetes field name, e.g.
    /// `"nfs"`) is allowed. `hostPath` is governed by [`Self::allow_host_path`].
    pub fn volume_type_allowed(&self, field: &str) -> bool {
        if field == "hostPath" {
            return self.allow_host_path;
        }
        SAFE_VOLUME_TYPES.contains(&field)
            || self
                .allowed_volume_types
                .iter()
                .any(|t| t == "*" || t.eq_ignore_ascii_case(field))
    }

    /// Whether `image` comes from an allowed registry.
    pub fn image_allowed(&self, image: &str) -> bool {
        if self.allowed_registries.is_empty() {
            return true;
        }
        let image = normalize_image(image);
        self.allowed_registries.iter().any(|entry| {
            let entry = entry.trim().trim_end_matches('/');
            !entry.is_empty()
                && (image == entry
                    || image
                        .strip_prefix(entry)
                        .is_some_and(|rest| rest.starts_with(['/', ':', '@'])))
        })
    }

    /// Whether `apply_manifest` / the custom resource tools may write `kind`
    /// in API group `group` despite the kind being rejected by default.
    pub fn manifest_kind_allowed(&self, group: &str, kind: &str) -> bool {
        self.allowed_manifest_kinds.iter().any(|entry| {
            let entry = entry.trim();
            if entry == "*" {
                return true;
            }
            match entry.rsplit_once('/') {
                Some((g, k)) => {
                    let g = if g == "core" { "" } else { g };
                    g.eq_ignore_ascii_case(group) && k.eq_ignore_ascii_case(kind)
                }
                None => entry.eq_ignore_ascii_case(kind),
            }
        })
    }

    /// Whether custom resources in API group `group` may be written.
    pub fn custom_resource_group_allowed(&self, group: &str) -> bool {
        self.allowed_custom_resource_groups
            .iter()
            .any(|g| g == "*" || g.eq_ignore_ascii_case(group))
    }
}

/// `"cap_net_admin"` → `"NET_ADMIN"`.
pub(crate) fn normalize_capability(cap: &str) -> String {
    let upper = cap.trim().to_ascii_uppercase();
    upper
        .strip_prefix("CAP_")
        .map(str::to_string)
        .unwrap_or(upper)
}

/// Glob match for annotation patterns: a trailing `*` matches a prefix.
pub(crate) fn pattern_matches(pattern: &str, key: &str) -> bool {
    let pattern = pattern.trim();
    match pattern.strip_suffix('*') {
        Some(prefix) => key.starts_with(prefix),
        None => pattern == key,
    }
}

// ---------------------------------------------------------------------------
// Flags and violations
// ---------------------------------------------------------------------------

/// A policy setting that relaxes (or, for allow/deny lists, configures) a
/// rule: its server flag, environment variable and [`WorkloadPolicy`] field.
#[derive(Debug, PartialEq, Eq)]
pub struct PolicyFlag {
    pub cli: &'static str,
    pub env: &'static str,
    pub field: &'static str,
}

macro_rules! flag {
    ($name:ident, $cli:literal, $env:literal, $field:literal) => {
        pub const $name: PolicyFlag = PolicyFlag {
            cli: $cli,
            env: $env,
            field: $field,
        };
    };
}

/// Every policy setting, for error messages and documentation.
pub mod flags {
    use super::PolicyFlag;
    flag!(
        ALLOW_PRIVILEGED,
        "--allow-privileged",
        "MCP_K8S_ALLOW_PRIVILEGED",
        "allow_privileged"
    );
    flag!(
        ALLOW_PRIVILEGE_ESCALATION,
        "--allow-privilege-escalation",
        "MCP_K8S_ALLOW_PRIVILEGE_ESCALATION",
        "allow_privilege_escalation"
    );
    flag!(
        ALLOW_HOST_NAMESPACES,
        "--allow-host-namespaces",
        "MCP_K8S_ALLOW_HOST_NAMESPACES",
        "allow_host_namespaces"
    );
    flag!(
        ALLOW_HOST_PATH,
        "--allow-host-path",
        "MCP_K8S_ALLOW_HOST_PATH",
        "allow_host_path"
    );
    flag!(
        ALLOW_HOST_PORTS,
        "--allow-host-ports",
        "MCP_K8S_ALLOW_HOST_PORTS",
        "allow_host_ports"
    );
    flag!(
        ALLOW_UNCONFINED,
        "--allow-unconfined",
        "MCP_K8S_ALLOW_UNCONFINED",
        "allow_unconfined"
    );
    flag!(
        ALLOW_CAPABILITIES,
        "--allow-capabilities",
        "MCP_K8S_ALLOW_CAPABILITIES",
        "allowed_capabilities"
    );
    flag!(
        ALLOW_VOLUME_TYPES,
        "--allow-volume-types",
        "MCP_K8S_ALLOW_VOLUME_TYPES",
        "allowed_volume_types"
    );
    flag!(
        ALLOWED_REGISTRIES,
        "--allowed-registries",
        "MCP_K8S_ALLOWED_REGISTRIES",
        "allowed_registries"
    );
    flag!(
        ALLOW_NODEPORT,
        "--allow-nodeport",
        "MCP_K8S_ALLOW_NODEPORT",
        "allow_node_port"
    );
    flag!(
        ALLOW_LOADBALANCER,
        "--allow-loadbalancer",
        "MCP_K8S_ALLOW_LOADBALANCER",
        "allow_load_balancer"
    );
    flag!(
        ALLOW_EXTERNAL_SERVICES,
        "--allow-external-services",
        "MCP_K8S_ALLOW_EXTERNAL_SERVICES",
        "allow_external_services"
    );
    flag!(
        ALLOW_INGRESS_SNIPPETS,
        "--allow-ingress-snippets",
        "MCP_K8S_ALLOW_INGRESS_SNIPPETS",
        "allow_ingress_snippets"
    );
    flag!(
        INGRESS_ANNOTATION_ALLOWLIST,
        "--ingress-annotation-allowlist",
        "MCP_K8S_INGRESS_ANNOTATION_ALLOWLIST",
        "ingress_annotation_allowlist"
    );
    flag!(
        INGRESS_ANNOTATION_DENYLIST,
        "--ingress-annotation-denylist",
        "MCP_K8S_INGRESS_ANNOTATION_DENYLIST",
        "ingress_annotation_denylist"
    );
    flag!(
        ALLOW_MANIFEST_KINDS,
        "--allow-manifest-kinds",
        "MCP_K8S_ALLOW_MANIFEST_KINDS",
        "allowed_manifest_kinds"
    );
    flag!(
        ALLOW_CUSTOM_RESOURCE_GROUPS,
        "--allow-custom-resource-groups",
        "MCP_K8S_ALLOW_CUSTOM_RESOURCE_GROUPS",
        "allowed_custom_resource_groups"
    );
    flag!(
        ALLOW_RBAC_ESCALATION,
        "--allow-rbac-escalation",
        "MCP_K8S_ALLOW_RBAC_ESCALATION",
        "allow_rbac_escalation"
    );
    flag!(
        NO_SECURE_DEFAULTS,
        "--no-secure-defaults",
        "MCP_K8S_NO_SECURE_DEFAULTS",
        "secure_defaults"
    );

    /// All settings, in documentation order.
    pub const ALL: &[&PolicyFlag] = &[
        &ALLOW_PRIVILEGED,
        &ALLOW_PRIVILEGE_ESCALATION,
        &ALLOW_HOST_NAMESPACES,
        &ALLOW_HOST_PATH,
        &ALLOW_HOST_PORTS,
        &ALLOW_UNCONFINED,
        &ALLOW_CAPABILITIES,
        &ALLOW_VOLUME_TYPES,
        &ALLOWED_REGISTRIES,
        &ALLOW_NODEPORT,
        &ALLOW_LOADBALANCER,
        &ALLOW_EXTERNAL_SERVICES,
        &ALLOW_INGRESS_SNIPPETS,
        &INGRESS_ANNOTATION_ALLOWLIST,
        &INGRESS_ANNOTATION_DENYLIST,
        &ALLOW_MANIFEST_KINDS,
        &ALLOW_CUSTOM_RESOURCE_GROUPS,
        &ALLOW_RBAC_ESCALATION,
        &NO_SECURE_DEFAULTS,
    ];
}

/// How an operator can lift a violation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Remedy {
    /// Turn on a boolean setting.
    Enable(&'static PolicyFlag),
    /// Add `value` to a list setting.
    AddTo(&'static PolicyFlag, String),
    /// Remove `value` from a deny-list setting.
    RemoveFrom(&'static PolicyFlag, String),
}

impl fmt::Display for Remedy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Remedy::Enable(flag) => write!(
                f,
                "an operator can allow this with {} (env {}=true; library: WorkloadPolicy.{})",
                flag.cli, flag.env, flag.field
            ),
            Remedy::AddTo(flag, value) => write!(
                f,
                "an operator can allow this with {} {value} (env {}; library: WorkloadPolicy.{})",
                flag.cli, flag.env, flag.field
            ),
            Remedy::RemoveFrom(flag, value) => write!(
                f,
                "{value} is listed in {} (env {}; library: WorkloadPolicy.{})",
                flag.cli, flag.env, flag.field
            ),
        }
    }
}

/// One broken rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    /// Where in the object, e.g. `spec.template.spec.containers[0] (app)`.
    pub location: String,
    /// What is wrong.
    pub reason: String,
    /// How an operator can allow it.
    pub remedy: Remedy,
}

impl Violation {
    pub(crate) fn new(
        location: impl Into<String>,
        reason: impl Into<String>,
        remedy: Remedy,
    ) -> Self {
        Self {
            location: location.into(),
            reason: reason.into(),
            remedy,
        }
    }

    /// The flag that would lift this violation.
    pub fn flag(&self) -> &'static PolicyFlag {
        match &self.remedy {
            Remedy::Enable(f) | Remedy::AddTo(f, _) | Remedy::RemoveFrom(f, _) => f,
        }
    }
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}; {}", self.location, self.reason, self.remedy)
    }
}

/// The request broke one or more guardrails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyError {
    pub violations: Vec<Violation>,
}

impl fmt::Display for PolicyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let n = self.violations.len();
        writeln!(
            f,
            "rejected by the mcp-k8s workload policy ({n} violation{}):",
            if n == 1 { "" } else { "s" }
        )?;
        for v in &self.violations {
            writeln!(f, "- {v}")?;
        }
        write!(
            f,
            "These guardrails apply to every tool, including apply_manifest and the custom resource tools."
        )
    }
}

impl std::error::Error for PolicyError {}

/// `Ok(())` when there are no violations, otherwise a [`PolicyError`] with
/// the violations de-duplicated in a stable order.
pub fn into_result(mut violations: Vec<Violation>) -> Result<(), PolicyError> {
    if violations.is_empty() {
        return Ok(());
    }
    let mut seen = std::collections::HashSet::new();
    violations.retain(|v| seen.insert(v.to_string()));
    let error = PolicyError { violations };
    error.record_denial();
    Err(error)
}

impl PolicyError {
    /// Report this rejection to the running tool call's telemetry:
    /// `mcp_k8s.decision=denied`, `mcp_k8s.denied_rule=policy:<setting>,…`
    /// (the settings that would allow it). A no-op outside a tool call.
    pub fn record_denial(&self) {
        let mut fields: Vec<&str> = self.violations.iter().map(|v| v.flag().field).collect();
        fields.sort_unstable();
        fields.dedup();
        crate::telemetry::record_denial(&format!("policy:{}", fields.join(",")));
    }
}

#[cfg(test)]
mod tests;
