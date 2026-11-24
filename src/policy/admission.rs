//! The parts of the policy that need the API server: discovery (to learn a
//! kind's scope), existence checks (to map server-side apply onto create or
//! update permissions), dry-run patches (to check what an update produces),
//! and fetching the role a binding refers to.

use std::fmt::Debug;

use kube::api::{Api, DynamicObject, Patch, PatchParams};
use kube::core::discovery::Scope;
use kube::core::GroupVersionKind;
use kube::{discovery, Resource, ResourceExt};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;

use super::manifest::{group_of, kind_not_allowed};
use super::rbac::check_bound_role;
use super::{check_object, into_result, Source, Violation, MANAGED_BY_LABEL, MANAGED_BY_VALUE};
use crate::client::K8sClient;
use crate::permissions::Action;

/// Which tool is admitting a raw manifest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AdmissionOrigin {
    /// `apply_manifest`: server-side apply, which creates or updates.
    ApplyManifest,
    /// `create_custom_resource`: always a create.
    CreateCustomResource,
}

/// Where an admitted manifest goes.
pub(crate) struct AdmittedTarget {
    pub api: Api<DynamicObject>,
    pub name: String,
    pub kind: String,
}

/// The `--disable` key typed tools use for a kind, so `apply_manifest` honours
/// the same per-resource switches (e.g. `deployment-create`).
pub(crate) fn resource_key(group: &str, kind: &str) -> String {
    let key = match kind {
        "PersistentVolumeClaim" => "pvc",
        "PersistentVolume" => "pv",
        "HorizontalPodAutoscaler" => "hpa",
        "PodDisruptionBudget" => "pdb",
        "CustomResourceDefinition" => "crd",
        _ if !super::is_builtin_group(group) => "custom_resource",
        _ => return kind.to_ascii_lowercase(),
    };
    key.to_string()
}

fn policy_error(violations: Vec<Violation>) -> Result<(), String> {
    into_result(violations).map_err(|e| e.to_string())
}

/// Check a raw manifest before `apply_manifest` or `create_custom_resource`
/// writes it, and resolve where it goes.
///
/// Runs the static policy ([`check_object`] with [`Source::Manifest`]), the
/// namespace allowlist, the cluster-scope gate (custom cluster-scoped kinds
/// are only known after discovery), the per-resource create/update
/// permissions, and, for bindings, the rules of the bound role. Adds the
/// `app.kubernetes.io/managed-by` label when the manifest has none.
pub(crate) async fn admit_manifest(
    client: &K8sClient,
    manifest: &mut Value,
    origin: AdmissionOrigin,
    namespace: Option<&str>,
) -> Result<AdmittedTarget, String> {
    let api_version = manifest["apiVersion"]
        .as_str()
        .ok_or("manifest must contain 'apiVersion'")?
        .to_string();
    let kind = manifest["kind"]
        .as_str()
        .ok_or("manifest must contain 'kind'")?
        .to_string();
    let name = manifest["metadata"]["name"]
        .as_str()
        .ok_or("manifest must contain 'metadata.name'")?
        .to_string();
    let namespace = namespace
        .map(str::to_string)
        .or_else(|| manifest["metadata"]["namespace"].as_str().map(str::to_string));
    let group = group_of(&api_version).to_string();
    let policy = client.policy();

    policy_error(check_object(policy, manifest, Source::Manifest))?;

    if let Some(ns) = &namespace {
        if !client.is_namespace_allowed(ns) {
            return Err(format!("Namespace '{ns}' is not in the allowed list"));
        }
    }

    let (group_name, version) = match api_version.split_once('/') {
        Some((g, v)) => (g, v),
        None => ("", api_version.as_str()),
    };
    let gvk = GroupVersionKind::gvk(group_name, version, &kind);
    let (ar, caps) = discovery::pinned_kind(client.inner(), &gvk)
        .await
        .map_err(|e| format!("Failed to discover resource {kind} ({api_version}): {e}"))?;

    let api: Api<DynamicObject> = match (caps.scope, namespace.as_deref()) {
        (Scope::Namespaced, Some(ns)) => Api::namespaced_with(client.inner().clone(), ns, &ar),
        (Scope::Namespaced, None) => {
            return Err(format!(
                "Resource {kind} is namespaced but no namespace was provided in the manifest"
            ));
        }
        (Scope::Cluster, _) => {
            if !policy.manifest_kind_allowed(&group, &kind) {
                policy_error(vec![kind_not_allowed(
                    &group,
                    &kind,
                    "cluster-scoped objects affect every namespace",
                )])?;
            }
            Api::all_with(client.inner().clone(), &ar)
        }
    };

    if origin == AdmissionOrigin::ApplyManifest {
        // Server-side apply creates or updates; hold it to the same
        // per-resource switches as the typed create_*/update_* tools.
        let exists = api
            .get_opt(&name)
            .await
            .map_err(|e| format!("Failed to look up {kind} '{name}': {e}"))?
            .is_some();
        let action = if exists { Action::Update } else { Action::Create };
        let resource = resource_key(&group, &kind);
        if !client.permissions().is_action_allowed(&resource, &action) {
            let verb = if exists { "update the existing" } else { "create" };
            return Err(format!(
                "apply_manifest would {verb} {kind} '{name}', but action '{action}' is not allowed on resource '{resource}' \
                 (see --disable-{action} and --disable {resource}-{action})"
            ));
        }
    }

    if group == "rbac.authorization.k8s.io" && (kind == "RoleBinding" || kind == "ClusterRoleBinding") {
        let violations =
            bound_role_violations(client, namespace.as_deref(), &manifest["roleRef"]).await?;
        policy_error(violations)?;
    }

    add_managed_by_label(manifest);
    Ok(AdmittedTarget { api, name, kind })
}

/// Add `app.kubernetes.io/managed-by: mcp-k8s` unless the object already has
/// a managed-by label (e.g. Helm's).
pub(crate) fn add_managed_by_label(object: &mut Value) {
    if !object["metadata"].is_object() {
        return;
    }
    let labels = &mut object["metadata"]["labels"];
    if !labels.is_object() {
        *labels = Value::Object(Default::default());
    }
    if let Some(map) = labels.as_object_mut() {
        map.entry(MANAGED_BY_LABEL)
            .or_insert_with(|| Value::String(MANAGED_BY_VALUE.to_string()));
    }
}

/// Violations from the role a binding refers to. A role that doesn't exist
/// yet grants nothing, so it passes (creating it later is checked then).
pub(crate) async fn bound_role_violations(
    client: &K8sClient,
    binding_namespace: Option<&str>,
    role_ref: &Value,
) -> Result<Vec<Violation>, String> {
    let policy = client.policy();
    if policy.allow_rbac_escalation {
        return Ok(Vec::new());
    }
    let kind = role_ref["kind"].as_str().unwrap_or("");
    let name = role_ref["name"].as_str().unwrap_or("");
    let role = match kind {
        "ClusterRole" => {
            let api: Api<k8s_openapi::api::rbac::v1::ClusterRole> = Api::all(client.inner().clone());
            api.get_opt(name)
                .await
                .map_err(|e| format!("Failed to look up ClusterRole '{name}': {e}"))?
                .map(|r| serde_json::to_value(r).unwrap_or_default())
        }
        "Role" => {
            let ns = binding_namespace.ok_or("a binding to a Role needs a namespace")?;
            let api: Api<k8s_openapi::api::rbac::v1::Role> =
                Api::namespaced(client.inner().clone(), ns);
            api.get_opt(name)
                .await
                .map_err(|e| format!("Failed to look up Role '{name}': {e}"))?
                .map(|r| serde_json::to_value(r).unwrap_or_default())
        }
        _ => None,
    };
    let mut out = Vec::new();
    if let Some(role) = role {
        check_bound_role(policy, kind, name, &role, &mut out);
    }
    Ok(out)
}

/// Check a typed object a tool built, before it is created.
pub(crate) fn check_typed<T: Serialize>(client: &K8sClient, object: &T) -> Result<(), String> {
    let value = serde_json::to_value(object).map_err(|e| e.to_string())?;
    policy_error(check_object(client.policy(), &value, Source::Typed))
}

/// How [`patch_checked`] sends its patch.
#[derive(Clone, Copy, Debug)]
pub(crate) enum PatchKind {
    Strategic,
    Merge,
}

/// Apply `patch` only if the object it produces passes the policy.
///
/// The patch is sent as a server-side dry run first, the result is checked
/// with [`check_object`], and then the patch is sent for real with the dry
/// run's `resourceVersion` as a precondition, so a concurrent change can't
/// slip in between check and write. Because the whole resulting object is
/// checked, this also refuses to modify an existing object that already
/// breaks the policy (e.g. swapping the image of a privileged DaemonSet).
pub(crate) async fn patch_checked<K>(
    client: &K8sClient,
    api: &Api<K>,
    name: &str,
    patch: Value,
    kind: PatchKind,
    source: Source,
) -> Result<K, String>
where
    K: Resource + Clone + DeserializeOwned + Serialize + Debug,
{
    let to_patch = |p: &Value| match kind {
        PatchKind::Strategic => Patch::Strategic(p.clone()),
        PatchKind::Merge => Patch::Merge(p.clone()),
    };
    let dry_run = PatchParams {
        dry_run: true,
        ..PatchParams::default()
    };
    let preview = api
        .patch(name, &dry_run, &to_patch(&patch))
        .await
        .map_err(|e| e.to_string())?;
    let value = serde_json::to_value(&preview).map_err(|e| e.to_string())?;
    policy_error(check_object(client.policy(), &value, source))?;

    let mut patch = patch;
    if let Some(rv) = preview.resource_version() {
        patch["metadata"]["resourceVersion"] = Value::String(rv);
    }
    api.patch(name, &PatchParams::default(), &to_patch(&patch))
        .await
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_keys_match_typed_tool_names() {
        assert_eq!(resource_key("apps", "Deployment"), "deployment");
        assert_eq!(resource_key("", "PersistentVolumeClaim"), "pvc");
        assert_eq!(resource_key("autoscaling", "HorizontalPodAutoscaler"), "hpa");
        assert_eq!(resource_key("policy", "PodDisruptionBudget"), "pdb");
        assert_eq!(resource_key("networking.k8s.io", "NetworkPolicy"), "networkpolicy");
        assert_eq!(resource_key("cert-manager.io", "Certificate"), "custom_resource");
    }

    #[test]
    fn managed_by_label_added_but_not_overridden() {
        let mut obj = serde_json::json!({ "metadata": { "name": "x" } });
        add_managed_by_label(&mut obj);
        assert_eq!(obj["metadata"]["labels"][MANAGED_BY_LABEL], MANAGED_BY_VALUE);

        let mut helm = serde_json::json!({ "metadata": { "name": "x", "labels": { MANAGED_BY_LABEL: "Helm" } } });
        add_managed_by_label(&mut helm);
        assert_eq!(helm["metadata"]["labels"][MANAGED_BY_LABEL], "Helm");
    }
}
