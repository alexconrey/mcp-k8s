//! Object-level dispatch: which rules apply to which kind, plus the extra
//! kind and API-group gates for raw manifests.

use serde_json::Value;

use super::network::{check_ingress, check_service};
use super::pod::{check_container, check_pod_spec};
use super::rbac::{
    check_binding, check_cluster_role_extras, check_namespace_labels, check_rules, check_secret,
};
use super::{flags, Remedy, Violation, WorkloadPolicy};

/// Who built the object being checked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// A typed tool built it from validated arguments. Content rules apply.
    Typed,
    /// A caller supplied it as a raw manifest (the custom resource tools, or a
    /// library user checking a manifest). Content rules apply, plus the kind
    /// and custom resource group gates.
    Manifest,
}

/// API groups served by the Kubernetes API server itself. Anything else is a
/// custom (or aggregated) resource.
const BUILTIN_GROUPS: &[&str] = &[
    "",
    "admissionregistration.k8s.io",
    "apiextensions.k8s.io",
    "apiregistration.k8s.io",
    "apps",
    "authentication.k8s.io",
    "authorization.k8s.io",
    "autoscaling",
    "batch",
    "certificates.k8s.io",
    "coordination.k8s.io",
    "discovery.k8s.io",
    "events.k8s.io",
    "extensions",
    "flowcontrol.apiserver.k8s.io",
    "internal.apiserver.k8s.io",
    "metrics.k8s.io",
    "networking.k8s.io",
    "node.k8s.io",
    "policy",
    "rbac.authorization.k8s.io",
    "resource.k8s.io",
    "scheduling.k8s.io",
    "storage.k8s.io",
    "storagemigration.k8s.io",
];

/// Whether `group` is a built-in Kubernetes API group.
pub fn is_builtin_group(group: &str) -> bool {
    BUILTIN_GROUPS.contains(&group)
}

const RBAC: &str = "rbac.authorization.k8s.io";
const ADMISSION: &str = "admissionregistration.k8s.io";

/// Namespaced built-in kinds raw manifests may not write by default, with the
/// reason shown to the caller.
const SENSITIVE_NAMESPACED_KINDS: &[(&str, &str, &str)] = &[
    (RBAC, "Role", "RBAC objects grant permissions"),
    (RBAC, "RoleBinding", "RBAC objects grant permissions"),
    (
        "",
        "Endpoints",
        "hand-written endpoints can redirect a Service's traffic anywhere (CVE-2021-25740)",
    ),
    (
        "discovery.k8s.io",
        "EndpointSlice",
        "hand-written endpoints can redirect a Service's traffic anywhere (CVE-2021-25740)",
    ),
    (
        "apps",
        "ControllerRevision",
        "controller revisions feed pod templates into rollbacks",
    ),
];

/// Built-in cluster-scoped kinds. Raw manifests may not write any
/// cluster-scoped kind by default (custom ones are caught after discovery).
const CLUSTER_SCOPED_KINDS: &[(&str, &str)] = &[
    ("", "Namespace"),
    ("", "Node"),
    ("", "PersistentVolume"),
    ("", "ComponentStatus"),
    (RBAC, "ClusterRole"),
    (RBAC, "ClusterRoleBinding"),
    (ADMISSION, "MutatingWebhookConfiguration"),
    (ADMISSION, "ValidatingWebhookConfiguration"),
    (ADMISSION, "ValidatingAdmissionPolicy"),
    (ADMISSION, "ValidatingAdmissionPolicyBinding"),
    (ADMISSION, "MutatingAdmissionPolicy"),
    (ADMISSION, "MutatingAdmissionPolicyBinding"),
    ("apiextensions.k8s.io", "CustomResourceDefinition"),
    ("apiregistration.k8s.io", "APIService"),
    ("certificates.k8s.io", "CertificateSigningRequest"),
    ("certificates.k8s.io", "ClusterTrustBundle"),
    ("flowcontrol.apiserver.k8s.io", "FlowSchema"),
    ("flowcontrol.apiserver.k8s.io", "PriorityLevelConfiguration"),
    ("internal.apiserver.k8s.io", "StorageVersion"),
    ("networking.k8s.io", "IngressClass"),
    ("networking.k8s.io", "IPAddress"),
    ("networking.k8s.io", "ServiceCIDR"),
    ("node.k8s.io", "RuntimeClass"),
    ("resource.k8s.io", "DeviceClass"),
    ("resource.k8s.io", "ResourceSlice"),
    ("scheduling.k8s.io", "PriorityClass"),
    ("storage.k8s.io", "CSIDriver"),
    ("storage.k8s.io", "CSINode"),
    ("storage.k8s.io", "StorageClass"),
    ("storage.k8s.io", "VolumeAttachment"),
    ("storage.k8s.io", "VolumeAttributesClass"),
    ("storagemigration.k8s.io", "StorageVersionMigration"),
];

/// Typed tools to suggest instead of a rejected raw manifest.
fn typed_tool_for(kind: &str) -> Option<&'static str> {
    Some(match kind {
        "Role" => "create_role",
        "RoleBinding" => "create_rolebinding",
        "ClusterRole" => "create_clusterrole",
        "ClusterRoleBinding" => "create_clusterrolebinding",
        "Namespace" => "create_namespace",
        "StorageClass" => "create_storageclass",
        "IngressClass" => "create_ingressclass",
        _ => return None,
    })
}

/// `"apps/v1"` → `"apps"`, `"v1"` → `""`.
pub(crate) fn group_of(api_version: &str) -> &str {
    api_version.split_once('/').map(|(g, _)| g).unwrap_or("")
}

/// Whether `(group, kind)` is a built-in cluster-scoped kind.
pub(crate) fn is_builtin_cluster_scoped(group: &str, kind: &str) -> bool {
    CLUSTER_SCOPED_KINDS.contains(&(group, kind))
}

/// The violation for a kind raw manifests may not write.
pub fn kind_not_allowed(group: &str, kind: &str, why: &str) -> Violation {
    let shown = if group.is_empty() {
        kind.to_string()
    } else {
        format!("{group}/{kind}")
    };
    let hint = typed_tool_for(kind)
        .map(|t| format!(" Use the typed {t} tool instead."))
        .unwrap_or_default();
    Violation::new(
        "kind",
        format!("{shown} is not accepted from raw manifests: {why}.{hint}"),
        Remedy::AddTo(&flags::ALLOW_MANIFEST_KINDS, kind.to_string()),
    )
}

/// Check a Kubernetes object (its JSON form) against the policy.
///
/// Typed tools call this on the object they built (or, for updates, on the
/// server's dry-run result); the custom resource tools call it on the
/// caller's manifest. Returns every violation found; an empty
/// vector means the object is allowed.
pub fn check_object(policy: &WorkloadPolicy, object: &Value, source: Source) -> Vec<Violation> {
    let mut out = Vec::new();
    let api_version = object
        .get("apiVersion")
        .and_then(Value::as_str)
        .unwrap_or("");
    let kind = object.get("kind").and_then(Value::as_str).unwrap_or("");
    let group = group_of(api_version);

    if source == Source::Manifest && !policy.manifest_kind_allowed(group, kind) {
        if let Some((_, _, why)) = SENSITIVE_NAMESPACED_KINDS
            .iter()
            .find(|(g, k, _)| *g == group && *k == kind)
        {
            out.push(kind_not_allowed(group, kind, why));
        } else if is_builtin_cluster_scoped(group, kind) {
            out.push(kind_not_allowed(
                group,
                kind,
                "cluster-scoped objects affect every namespace",
            ));
        }
    }

    if !is_builtin_group(group) {
        if source == Source::Manifest && !policy.custom_resource_group_allowed(group) {
            out.push(Violation::new(
                "apiVersion",
                format!(
                    "custom resources in API group {group:?} are not allowlisted (operators can turn custom resources into pods, RBAC objects or anything else)"
                ),
                Remedy::AddTo(&flags::ALLOW_CUSTOM_RESOURCE_GROUPS, group.to_string()),
            ));
        }
        // Whatever the allowlist says, pod specs, containers and manifests
        // embedded in a custom resource get the same rules as top-level ones.
        if let Some(spec) = object.get("spec") {
            scan_embedded(policy, spec, "spec", &mut out);
        }
        return out;
    }

    check_builtin(policy, group, kind, object, &mut out);
    out
}

/// Content rules for built-in kinds.
fn check_builtin(
    policy: &WorkloadPolicy,
    group: &str,
    kind: &str,
    object: &Value,
    out: &mut Vec<Violation>,
) {
    let spec = object.get("spec");
    match (group, kind) {
        ("", "Pod") => {
            if let Some(spec) = spec {
                check_pod_spec(policy, spec, object.get("metadata"), "spec", out);
            }
        }
        ("", "PodTemplate") => check_template(policy, object.get("template"), "template", out),
        ("", "ReplicationController")
        | ("apps", "ReplicaSet")
        | ("apps", "Deployment")
        | ("apps", "StatefulSet")
        | ("apps", "DaemonSet")
        | ("batch", "Job") => check_template(
            policy,
            spec.and_then(|s| s.get("template")),
            "spec.template",
            out,
        ),
        ("batch", "CronJob") => check_template(
            policy,
            spec.and_then(|s| s.get("jobTemplate"))
                .and_then(|j| j.get("spec"))
                .and_then(|s| s.get("template")),
            "spec.jobTemplate.spec.template",
            out,
        ),
        ("", "Service") => check_service(policy, object, out),
        ("networking.k8s.io", "Ingress") | ("extensions", "Ingress") => {
            check_ingress(policy, object, out)
        }
        ("", "Secret") => check_secret(policy, object, out),
        ("", "Namespace") => {
            if let Some(labels) = object.get("metadata").and_then(|m| m.get("labels")) {
                check_namespace_labels(policy, labels, out);
            }
        }
        (RBAC, "Role") | (RBAC, "ClusterRole") => {
            let rules = object
                .get("rules")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            check_rules(policy, &rules, "rules", out);
            if kind == "ClusterRole" {
                check_cluster_role_extras(policy, object, out);
            }
        }
        (RBAC, "RoleBinding") | (RBAC, "ClusterRoleBinding") => check_binding(policy, object, out),
        _ => {}
    }
}

fn check_template(
    policy: &WorkloadPolicy,
    template: Option<&Value>,
    path: &str,
    out: &mut Vec<Violation>,
) {
    if let Some(spec) = template.and_then(|t| t.get("spec")) {
        check_pod_spec(
            policy,
            spec,
            template.and_then(|t| t.get("metadata")),
            &format!("{path}.spec"),
            out,
        );
    }
}

/// Walk a custom resource looking for things that become pods or objects:
/// pod specs (`containers`), lone containers (`image`), volumes, and embedded
/// manifests (`apiVersion` + `kind`, e.g. Crossplane `Object`).
fn scan_embedded(policy: &WorkloadPolicy, value: &Value, path: &str, out: &mut Vec<Violation>) {
    match value {
        Value::Object(map) => {
            // References ({apiVersion, kind, name}) are not manifests; real
            // embedded manifests carry metadata.
            let embedded_manifest = map.get("apiVersion").is_some_and(Value::is_string)
                && map.get("kind").is_some_and(Value::is_string)
                && map.get("metadata").is_some_and(Value::is_object);
            if embedded_manifest {
                for v in check_object(policy, value, Source::Manifest) {
                    out.push(Violation::new(
                        format!(
                            "{path} (embedded {}): {}",
                            map["kind"].as_str().unwrap_or("?"),
                            v.location
                        ),
                        v.reason,
                        v.remedy,
                    ));
                }
                return;
            }
            let pod_spec = map.get("containers").is_some_and(Value::is_array);
            if pod_spec {
                check_pod_spec(policy, value, None, path, out);
            } else if map.get("image").is_some_and(Value::is_string) {
                check_container(policy, value, path, out);
            } else if let Some(volumes) = map.get("volumes").and_then(Value::as_array) {
                // Volumes outside a pod spec (e.g. Argo workflow-level volumes).
                let wrapper = serde_json::json!({ "volumes": volumes });
                check_pod_spec(policy, &wrapper, None, path, out);
            }
            for (key, child) in map {
                // Already covered by check_pod_spec above.
                if pod_spec
                    && matches!(
                        key.as_str(),
                        "containers" | "initContainers" | "ephemeralContainers" | "volumes"
                    )
                {
                    continue;
                }
                scan_embedded(policy, child, &format!("{path}.{key}"), out);
            }
        }
        Value::Array(list) => {
            for (i, child) in list.iter().enumerate() {
                scan_embedded(policy, child, &format!("{path}[{i}]"), out);
            }
        }
        _ => {}
    }
}
