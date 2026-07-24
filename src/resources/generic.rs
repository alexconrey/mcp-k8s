use kube::api::{Api, DynamicObject, Patch, PatchParams};
use kube::core::discovery::Scope;
use kube::core::GroupVersionKind;
use kube::discovery;

use crate::client::K8sClient;

/// Parse an apiVersion string into (group, version).
/// "v1" -> ("", "v1"), "apps/v1" -> ("apps", "v1")
fn parse_api_version(api_version: &str) -> (&str, &str) {
    match api_version.split_once('/') {
        Some((group, version)) => (group, version),
        None => ("", api_version),
    }
}

pub fn tool_definitions() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "name": "apply_manifest",
            "description": "Apply an arbitrary Kubernetes YAML or JSON manifest using server-side apply. Accepts any resource type. Returns the applied object's metadata (name, namespace, kind, resource_version).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "manifest": {
                        "type": "string",
                        "description": "The Kubernetes manifest as a YAML or JSON string"
                    }
                },
                "required": ["manifest"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "get_resource_yaml",
            "description": "Get any Kubernetes resource as raw JSON output. Accepts apiVersion, kind, name, and optional namespace. Uses API discovery to resolve the resource type.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "api_version": {
                        "type": "string",
                        "description": "API version (e.g. 'v1', 'apps/v1', 'networking.k8s.io/v1')"
                    },
                    "kind": {
                        "type": "string",
                        "description": "Resource kind (e.g. 'Pod', 'Deployment', 'Service')"
                    },
                    "name": {
                        "type": "string",
                        "description": "Resource name"
                    },
                    "namespace": {
                        "type": "string",
                        "description": "Kubernetes namespace (omit for cluster-scoped resources)"
                    }
                },
                "required": ["api_version", "kind", "name"],
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
        "apply_manifest" => apply_manifest(client, args).await,
        "get_resource_yaml" => get_resource_json(client, args).await,
        _ => return None,
    };
    Some(result)
}

async fn apply_manifest(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let manifest_str = args["manifest"].as_str().ok_or("manifest is required")?;

    // Parse the manifest string as JSON. If the input is YAML (not JSON),
    // this will fail with a clear error message.
    let manifest: serde_json::Value = serde_json::from_str(manifest_str).map_err(|e| {
        format!(
            "Failed to parse manifest as JSON: {e}. \
                 If providing YAML, please convert to JSON first."
        )
    })?;

    // Extract required fields from the parsed manifest
    let api_version = manifest["apiVersion"]
        .as_str()
        .ok_or("manifest must contain 'apiVersion'")?;
    let kind = manifest["kind"]
        .as_str()
        .ok_or("manifest must contain 'kind'")?;
    let resource_name = manifest["metadata"]["name"]
        .as_str()
        .ok_or("manifest must contain 'metadata.name'")?;
    let namespace = manifest["metadata"]["namespace"].as_str();

    // Check namespace access if present
    if let Some(ns) = namespace {
        if !client.is_namespace_allowed(ns) {
            return Err(format!("Namespace '{ns}' is not in the allowed list"));
        }
    }

    // Resolve the GVK via API discovery
    let (group, version) = parse_api_version(api_version);
    let gvk = GroupVersionKind::gvk(group, version, kind);
    let (ar, caps) = discovery::pinned_kind(client.inner(), &gvk)
        .await
        .map_err(|e| format!("Failed to discover resource {kind} ({api_version}): {e}"))?;

    // Build the API handle based on scope
    let api: Api<DynamicObject> = match (caps.scope, namespace) {
        (Scope::Namespaced, Some(ns)) => Api::namespaced_with(client.inner().clone(), ns, &ar),
        (Scope::Namespaced, None) => {
            return Err(format!(
                "Resource {kind} is namespaced but no namespace was provided in the manifest"
            ));
        }
        (Scope::Cluster, _) => Api::all_with(client.inner().clone(), &ar),
    };

    // Server-side apply with force
    let patch_params = PatchParams::apply("mcp-k8s").force();
    let applied = api
        .patch(resource_name, &patch_params, &Patch::Apply(&manifest))
        .await
        .map_err(|e| format!("Failed to apply manifest: {e}"))?;

    let result = serde_json::json!({
        "name": applied.metadata.name,
        "namespace": applied.metadata.namespace,
        "kind": kind,
        "resource_version": applied.metadata.resource_version,
    });
    serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
}

async fn get_resource_json(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let api_version = args["api_version"]
        .as_str()
        .ok_or("api_version is required")?;
    let kind = args["kind"].as_str().ok_or("kind is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;
    let namespace = args["namespace"].as_str();

    // Check namespace access if present
    if let Some(ns) = namespace {
        if !client.is_namespace_allowed(ns) {
            return Err(format!("Namespace '{ns}' is not in the allowed list"));
        }
    }

    // Resolve the GVK via API discovery
    let (group, version) = parse_api_version(api_version);
    let gvk = GroupVersionKind::gvk(group, version, kind);
    let (ar, caps) = discovery::pinned_kind(client.inner(), &gvk)
        .await
        .map_err(|e| format!("Failed to discover resource {kind} ({api_version}): {e}"))?;

    // Build the API handle based on scope
    let api: Api<DynamicObject> = match (caps.scope, namespace) {
        (Scope::Namespaced, Some(ns)) => Api::namespaced_with(client.inner().clone(), ns, &ar),
        (Scope::Namespaced, None) => {
            return Err(format!(
                "Resource {kind} is namespaced but no namespace was provided"
            ));
        }
        (Scope::Cluster, _) => Api::all_with(client.inner().clone(), &ar),
    };

    // Get the resource and return as pretty-printed JSON
    let obj = api
        .get(name)
        .await
        .map_err(|e| format!("Failed to get {kind}/{name}: {e}"))?;

    serde_json::to_string_pretty(&obj).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_definitions_returns_two_tools() {
        let defs = tool_definitions();
        assert_eq!(defs.len(), 2);

        let names: Vec<&str> = defs.iter().map(|d| d["name"].as_str().unwrap()).collect();

        let mut unique = names.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(names.len(), unique.len(), "tool names must be unique");

        assert!(names.contains(&"apply_manifest"));
        assert!(names.contains(&"get_resource_yaml"));
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
    fn parse_api_version_core() {
        let (group, version) = parse_api_version("v1");
        assert_eq!(group, "");
        assert_eq!(version, "v1");
    }

    #[test]
    fn parse_api_version_apps() {
        let (group, version) = parse_api_version("apps/v1");
        assert_eq!(group, "apps");
        assert_eq!(version, "v1");
    }

    #[test]
    fn parse_api_version_networking() {
        let (group, version) = parse_api_version("networking.k8s.io/v1");
        assert_eq!(group, "networking.k8s.io");
        assert_eq!(version, "v1");
    }

    #[test]
    fn apply_manifest_tool_schema() {
        let defs = tool_definitions();
        let apply = defs
            .iter()
            .find(|d| d["name"] == "apply_manifest")
            .expect("apply_manifest tool must exist");

        let schema = &apply["inputSchema"];
        let required = schema["required"].as_array().unwrap();
        assert_eq!(required.len(), 1);
        assert_eq!(required[0], "manifest");

        let props = schema["properties"].as_object().unwrap();
        assert!(props.contains_key("manifest"));
    }

    #[test]
    fn get_resource_yaml_tool_schema() {
        let defs = tool_definitions();
        let get = defs
            .iter()
            .find(|d| d["name"] == "get_resource_yaml")
            .expect("get_resource_yaml tool must exist");

        let schema = &get["inputSchema"];
        let required = schema["required"].as_array().unwrap();
        assert_eq!(required.len(), 3);
        assert!(required.contains(&serde_json::json!("api_version")));
        assert!(required.contains(&serde_json::json!("kind")));
        assert!(required.contains(&serde_json::json!("name")));

        let props = schema["properties"].as_object().unwrap();
        assert!(props.contains_key("api_version"));
        assert!(props.contains_key("kind"));
        assert!(props.contains_key("name"));
        assert!(props.contains_key("namespace"));
    }
}
