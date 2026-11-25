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

/// Input schema shared by `get_resource_json` and its deprecated alias.
fn resource_ref_schema() -> serde_json::Value {
    serde_json::json!({
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
    })
}

pub fn tool_definitions() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "name": "apply_manifest",
            "description": "Raw server-side apply of an arbitrary Kubernetes manifest, given as JSON. UNRESTRICTED: unlike every other write tool it skips the server's workload policy, so it can create privileged pods, hostPath mounts, RBAC bindings, webhooks, CRDs and any other kind. Only the namespace allowlist and the per-resource create/update switches still apply. Only call it when the user has explicitly asked for this manifest to be applied: show them the complete manifest and get their confirmation first. Never choose it yourself in place of a typed create_*/update_* tool, and never use it to get around a rejection from another tool. YAML is not accepted: convert it to JSON first. Returns the applied object's metadata (name, namespace, kind, resource_version).",
            // MCP tool annotations: clients such as Claude Code use these to
            // ask the user before running a destructive tool.
            "annotations": {
                "title": "Apply raw manifest (unrestricted)",
                "readOnlyHint": false,
                "destructiveHint": true,
                "idempotentHint": true,
                "openWorldHint": false
            },
            "inputSchema": {
                "type": "object",
                "properties": {
                    "manifest": {
                        "type": "string",
                        "description": "The Kubernetes manifest as a JSON string (YAML is not accepted)"
                    }
                },
                "required": ["manifest"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "get_resource_json",
            "description": "Get any Kubernetes resource as JSON, the object as the API server returns it. Accepts apiVersion, kind, name, and optional namespace. Uses API discovery to resolve the resource type. Use it to inspect an object before changing it with a typed update_* tool; convert it to YAML yourself if a person needs YAML.",
            "inputSchema": resource_ref_schema()
        }),
        serde_json::json!({
            "name": "get_resource_yaml",
            "description": "Deprecated alias for get_resource_json, kept for compatibility. It returns JSON, not YAML. Use get_resource_json instead; this alias will be removed in the next minor release.",
            "inputSchema": resource_ref_schema()
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
        // get_resource_yaml is a deprecated alias: the output has always been JSON.
        "get_resource_json" | "get_resource_yaml" => get_resource_json(client, args).await,
        _ => return None,
    };
    Some(result)
}

async fn apply_manifest(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let manifest_str = args["manifest"].as_str().ok_or("manifest is required")?;

    // Parse the manifest string as JSON. If the input is YAML (not JSON),
    // this will fail with a clear error message.
    let mut manifest: serde_json::Value = serde_json::from_str(manifest_str).map_err(|e| {
        format!(
            "Failed to parse manifest as JSON: {e}. \
                 If providing YAML, please convert to JSON first."
        )
    })?;
    if !manifest.is_object() {
        return Err("manifest must be a JSON object".to_string());
    }

    // No workload policy (on purpose; see admit_manifest): only the namespace
    // allowlist and the create/update switches. Resolves the target API.
    let target = crate::policy::admit_manifest(
        client,
        &mut manifest,
        crate::policy::AdmissionOrigin::ApplyManifest,
        None,
    )
    .await?;

    // Server-side apply with force
    let patch_params = PatchParams::apply("mcp-k8s").force();
    let applied = target
        .api
        .patch(&target.name, &patch_params, &Patch::Apply(&manifest))
        .await
        .map_err(|e| format!("Failed to apply manifest: {e}"))?;

    let result = serde_json::json!({
        "name": applied.metadata.name,
        "namespace": applied.metadata.namespace,
        "kind": target.kind,
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

    let mut value = serde_json::to_value(&obj).map_err(|e| e.to_string())?;
    if !client.permissions().secret_decode_enabled {
        // Base64 isn't protection: without --enable-secret-decode a Secret's
        // values never leave the server, whichever tool asks.
        crate::resources::secret::redact_secret_json(&mut value);
    }
    serde_json::to_string_pretty(&value).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_definitions_returns_three_tools() {
        let defs = tool_definitions();
        assert_eq!(defs.len(), 3);

        let names: Vec<&str> = defs.iter().map(|d| d["name"].as_str().unwrap()).collect();

        let mut unique = names.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(names.len(), unique.len(), "tool names must be unique");

        assert!(names.contains(&"apply_manifest"));
        assert!(names.contains(&"get_resource_json"));
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

    #[test]
    fn get_resource_yaml_is_a_documented_deprecated_alias() {
        let defs = tool_definitions();
        let def = |name: &str| defs.iter().find(|d| d["name"] == name).cloned().unwrap();
        let json = def("get_resource_json");
        let yaml = def("get_resource_yaml");
        // Same arguments, so callers can switch names without other changes.
        assert_eq!(json["inputSchema"], yaml["inputSchema"]);
        let desc = yaml["description"].as_str().unwrap();
        assert!(desc.starts_with("Deprecated"), "{desc}");
        assert!(desc.contains("get_resource_json"), "{desc}");
        assert!(desc.contains("returns JSON"), "{desc}");
    }

    #[test]
    fn apply_manifest_does_not_claim_yaml_support() {
        // The parser only accepts JSON (serde_json); the description must say so.
        let defs = tool_definitions();
        let def = defs.iter().find(|d| d["name"] == "apply_manifest").unwrap();
        let desc = def["description"].as_str().unwrap();
        let param = def["inputSchema"]["properties"]["manifest"]["description"]
            .as_str()
            .unwrap();
        assert!(!desc.contains("YAML or JSON"), "{desc}");
        assert!(desc.contains("YAML is not accepted"), "{desc}");
        // It bypasses the workload policy: the description must say so, and
        // put the user, not the agent, in charge of using it.
        assert!(desc.contains("UNRESTRICTED"), "{desc}");
        assert!(desc.contains("explicitly asked"), "{desc}");
        assert!(desc.contains("get their confirmation first"), "{desc}");
        assert!(
            desc.contains("never use it to get around a rejection"),
            "{desc}"
        );
        // MCP clients use the annotation to ask the user before each call.
        assert_eq!(def["annotations"]["destructiveHint"], true);
        assert_eq!(def["annotations"]["readOnlyHint"], false);
        assert!(
            param.contains("JSON") && param.contains("not accepted"),
            "{param}"
        );
    }
}
