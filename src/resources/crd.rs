use k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition;
use kube::api::{Api, DeleteParams, DynamicObject, ListParams, PostParams};
use kube::core::discovery::Scope;
use kube::core::GroupVersionKind;
use kube::discovery;
use serde::Serialize;

use crate::client::K8sClient;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn crd_api(client: &K8sClient) -> Api<CustomResourceDefinition> {
    Api::all(client.inner().clone())
}

async fn get_dynamic_api(
    client: &K8sClient,
    group: &str,
    version: &str,
    kind: &str,
    namespace: Option<&str>,
) -> Result<Api<DynamicObject>, String> {
    get_dynamic_api_with_scope(client, group, version, kind, namespace)
        .await
        .map(|(api, _)| api)
}

async fn get_dynamic_api_with_scope(
    client: &K8sClient,
    group: &str,
    version: &str,
    kind: &str,
    namespace: Option<&str>,
) -> Result<(Api<DynamicObject>, Scope), String> {
    let gvk = GroupVersionKind::gvk(group, version, kind);
    let (ar, caps) = discovery::pinned_kind(client.inner(), &gvk)
        .await
        .map_err(|e| format!("Failed to discover {kind}: {e}"))?;
    let scope = caps.scope;

    if let Some(ns) = namespace {
        if !client.is_namespace_allowed(ns) {
            return Err(format!("Namespace '{ns}' is not in the allowed list"));
        }
        Ok((Api::namespaced_with(client.inner().clone(), ns, &ar), scope))
    } else {
        Ok((Api::all_with(client.inner().clone(), &ar), scope))
    }
}

/// The custom resource tools only handle custom resources: built-in kinds go
/// through their typed tools or apply_manifest, which apply the full policy.
fn reject_builtin(group: &str, kind: &str, tool: &str) -> Result<(), String> {
    if crate::policy::is_builtin_group(group) {
        let shown = if group.is_empty() {
            "the core API group".to_string()
        } else {
            format!("API group {group:?}")
        };
        return Err(format!(
            "{tool} only handles custom resources; {kind} in {shown} is a built-in kind. \
             Use its typed create_*/update_* tool, or apply_manifest."
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Summary types
// ---------------------------------------------------------------------------

#[derive(Serialize, Debug, Clone)]
pub struct CrdSummary {
    pub name: String,
    pub group: String,
    pub kind: String,
    pub scope: String,
    pub versions: Vec<String>,
    pub created_at: Option<String>,
}

// ---------------------------------------------------------------------------
// Tool definitions
// ---------------------------------------------------------------------------

pub fn tool_definitions() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "name": "list_crds",
            "description": "List all CustomResourceDefinitions installed in the cluster. Returns name, group, kind, scope, versions, and creation timestamp for each CRD.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "required": [],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "get_crd",
            "description": "Get a CustomResourceDefinition by name. Returns the full spec including group, names, scope, and versions with schema information.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "CRD name (e.g. 'crontabs.stable.example.com')"
                    }
                },
                "required": ["name"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "list_custom_resources",
            "description": "List instances of a custom resource. Uses dynamic API discovery to resolve the resource type from group/version/kind.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "group": {
                        "type": "string",
                        "description": "API group of the custom resource (e.g. 'stable.example.com')"
                    },
                    "version": {
                        "type": "string",
                        "description": "API version (e.g. 'v1', 'v1alpha1')"
                    },
                    "kind": {
                        "type": "string",
                        "description": "Resource kind (e.g. 'CronTab')"
                    },
                    "namespace": {
                        "type": "string",
                        "description": "Kubernetes namespace (omit for cluster-scoped custom resources)"
                    }
                },
                "required": ["group", "version", "kind"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "get_custom_resource",
            "description": "Get a single custom resource by name. Uses dynamic API discovery to resolve the resource type from group/version/kind.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "group": {
                        "type": "string",
                        "description": "API group of the custom resource (e.g. 'stable.example.com')"
                    },
                    "version": {
                        "type": "string",
                        "description": "API version (e.g. 'v1', 'v1alpha1')"
                    },
                    "kind": {
                        "type": "string",
                        "description": "Resource kind (e.g. 'CronTab')"
                    },
                    "name": {
                        "type": "string",
                        "description": "Resource name"
                    },
                    "namespace": {
                        "type": "string",
                        "description": "Kubernetes namespace (omit for cluster-scoped custom resources)"
                    }
                },
                "required": ["group", "version", "kind", "name"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "create_custom_resource",
            "description": "Create a custom resource from a JSON manifest. Uses dynamic API discovery to resolve the resource type from group/version/kind. Custom resources can make operators create pods or RBAC objects, so writes are only accepted for API groups the server operator allowlists (--allow-custom-resource-groups); cluster-scoped kinds also need --allow-manifest-kinds. Pod specs, containers and manifests embedded in the resource get the same guardrails as the typed tools. Built-in kinds are rejected: use their typed tools or apply_manifest.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "group": {
                        "type": "string",
                        "description": "API group of the custom resource (e.g. 'stable.example.com')"
                    },
                    "version": {
                        "type": "string",
                        "description": "API version (e.g. 'v1', 'v1alpha1')"
                    },
                    "kind": {
                        "type": "string",
                        "description": "Resource kind (e.g. 'CronTab')"
                    },
                    "namespace": {
                        "type": "string",
                        "description": "Kubernetes namespace (omit for cluster-scoped custom resources)"
                    },
                    "manifest": {
                        "type": "object",
                        "description": "JSON object for the full resource manifest (must include apiVersion, kind, metadata, and spec)"
                    }
                },
                "required": ["group", "version", "kind", "manifest"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "update_custom_resource",
            "description": "Merge-patch a custom resource. Uses dynamic API discovery to resolve the resource type from group/version/kind. Same rules as create_custom_resource: only allowlisted API groups, and the patched result is checked against the guardrails with a server-side dry run before it is written. Built-in kinds are rejected: use their typed update_* tools or apply_manifest.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "group": {
                        "type": "string",
                        "description": "API group of the custom resource (e.g. 'stable.example.com')"
                    },
                    "version": {
                        "type": "string",
                        "description": "API version (e.g. 'v1', 'v1alpha1')"
                    },
                    "kind": {
                        "type": "string",
                        "description": "Resource kind (e.g. 'CronTab')"
                    },
                    "name": {
                        "type": "string",
                        "description": "Resource name"
                    },
                    "namespace": {
                        "type": "string",
                        "description": "Kubernetes namespace (omit for cluster-scoped custom resources)"
                    },
                    "patch": {
                        "type": "object",
                        "description": "JSON merge-patch object to apply to the resource"
                    }
                },
                "required": ["group", "version", "kind", "name", "patch"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "delete_custom_resource",
            "description": "Delete a custom resource by name. Uses dynamic API discovery to resolve the resource type from group/version/kind.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "group": {
                        "type": "string",
                        "description": "API group of the custom resource (e.g. 'stable.example.com')"
                    },
                    "version": {
                        "type": "string",
                        "description": "API version (e.g. 'v1', 'v1alpha1')"
                    },
                    "kind": {
                        "type": "string",
                        "description": "Resource kind (e.g. 'CronTab')"
                    },
                    "name": {
                        "type": "string",
                        "description": "Resource name"
                    },
                    "namespace": {
                        "type": "string",
                        "description": "Kubernetes namespace (omit for cluster-scoped custom resources)"
                    }
                },
                "required": ["group", "version", "kind", "name"],
                "additionalProperties": false
            }
        }),
    ]
}

// ---------------------------------------------------------------------------
// Tool dispatch
// ---------------------------------------------------------------------------

pub async fn handle_tool(
    client: &K8sClient,
    name: &str,
    args: &serde_json::Value,
) -> Option<Result<String, String>> {
    let result = match name {
        "list_crds" => list_crds(client).await,
        "get_crd" => get_crd(client, args).await,
        "list_custom_resources" => list_custom_resources(client, args).await,
        "get_custom_resource" => get_custom_resource(client, args).await,
        "create_custom_resource" => create_custom_resource(client, args).await,
        "update_custom_resource" => update_custom_resource(client, args).await,
        "delete_custom_resource" => delete_custom_resource(client, args).await,
        _ => return None,
    };
    Some(result)
}

// ---------------------------------------------------------------------------
// Tool implementations
// ---------------------------------------------------------------------------

async fn list_crds(client: &K8sClient) -> Result<String, String> {
    let api = crd_api(client);
    let list = api
        .list(&ListParams::default())
        .await
        .map_err(|e| format!("Failed to list CRDs: {e}"))?;

    let summaries: Vec<CrdSummary> = list
        .items
        .iter()
        .map(|crd| {
            let spec = &crd.spec;
            CrdSummary {
                name: crd.metadata.name.clone().unwrap_or_default(),
                group: spec.group.clone(),
                kind: spec.names.kind.clone(),
                scope: spec.scope.clone(),
                versions: spec.versions.iter().map(|v| v.name.clone()).collect(),
                created_at: crd
                    .metadata
                    .creation_timestamp
                    .as_ref()
                    .map(|t| t.0.to_string()),
            }
        })
        .collect();

    serde_json::to_string_pretty(&summaries).map_err(|e| e.to_string())
}

async fn get_crd(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let name = args["name"].as_str().ok_or("name is required")?;
    let api = crd_api(client);
    let crd = api
        .get(name)
        .await
        .map_err(|e| format!("Failed to get CRD {name}: {e}"))?;

    serde_json::to_string_pretty(&crd).map_err(|e| e.to_string())
}

async fn list_custom_resources(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let group = args["group"].as_str().ok_or("group is required")?;
    let version = args["version"].as_str().ok_or("version is required")?;
    let kind = args["kind"].as_str().ok_or("kind is required")?;
    let namespace = args["namespace"].as_str();

    let api = get_dynamic_api(client, group, version, kind, namespace).await?;
    let list = api
        .list(&ListParams::default())
        .await
        .map_err(|e| format!("Failed to list {kind}: {e}"))?;

    serde_json::to_string_pretty(&list).map_err(|e| e.to_string())
}

async fn get_custom_resource(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let group = args["group"].as_str().ok_or("group is required")?;
    let version = args["version"].as_str().ok_or("version is required")?;
    let kind = args["kind"].as_str().ok_or("kind is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;
    let namespace = args["namespace"].as_str();

    let api = get_dynamic_api(client, group, version, kind, namespace).await?;
    let obj = api
        .get(name)
        .await
        .map_err(|e| format!("Failed to get {kind}/{name}: {e}"))?;

    serde_json::to_string_pretty(&obj).map_err(|e| e.to_string())
}

async fn create_custom_resource(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let group = args["group"].as_str().ok_or("group is required")?;
    let version = args["version"].as_str().ok_or("version is required")?;
    let kind = args["kind"].as_str().ok_or("kind is required")?;
    let namespace = args["namespace"].as_str();
    reject_builtin(group, kind, "create_custom_resource")?;

    let mut manifest = args.get("manifest").ok_or("manifest is required")?.clone();
    if !manifest.is_object() {
        return Err("manifest must be a JSON object".to_string());
    }
    // The manifest must describe the resource the arguments name.
    let api_version = format!("{group}/{version}");
    for (field, expected) in [("apiVersion", api_version.as_str()), ("kind", kind)] {
        match manifest[field].as_str() {
            None => manifest[field] = serde_json::Value::String(expected.to_string()),
            Some(found) if found == expected => {}
            Some(found) => {
                return Err(format!(
                    "manifest {field} {found:?} does not match the arguments ({expected:?})"
                ))
            }
        }
    }
    if let (Some(ns), Some(found)) = (namespace, manifest["metadata"]["namespace"].as_str()) {
        if ns != found {
            return Err(format!(
                "manifest metadata.namespace {found:?} does not match the namespace argument {ns:?}"
            ));
        }
    }

    let target = crate::policy::admit_manifest(
        client,
        &mut manifest,
        crate::policy::AdmissionOrigin::CreateCustomResource,
        namespace,
    )
    .await?;

    let obj: DynamicObject =
        serde_json::from_value(manifest).map_err(|e| format!("Invalid manifest: {e}"))?;
    let created = target
        .api
        .create(&PostParams::default(), &obj)
        .await
        .map_err(|e| format!("Failed to create {kind}: {e}"))?;

    let result = serde_json::json!({
        "name": created.metadata.name,
        "namespace": created.metadata.namespace,
        "kind": kind,
        "resource_version": created.metadata.resource_version,
    });
    serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
}

async fn update_custom_resource(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let group = args["group"].as_str().ok_or("group is required")?;
    let version = args["version"].as_str().ok_or("version is required")?;
    let kind = args["kind"].as_str().ok_or("kind is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;
    let namespace = args["namespace"].as_str();
    let patch = args.get("patch").ok_or("patch is required")?.clone();
    reject_builtin(group, kind, "update_custom_resource")?;

    let (api, scope) = get_dynamic_api_with_scope(client, group, version, kind, namespace).await?;
    if matches!(scope, Scope::Cluster) && !client.policy().manifest_kind_allowed(group, kind) {
        return Err(crate::policy::PolicyError {
            violations: vec![crate::policy::kind_not_allowed(
                group,
                kind,
                "cluster-scoped objects affect every namespace",
            )],
        }
        .to_string());
    }
    // The dry-run result is checked like a raw manifest: the group must be
    // allowlisted and embedded pod specs must pass the pod rules.
    let patched = crate::policy::patch_checked(
        client,
        &api,
        name,
        patch,
        crate::policy::PatchKind::Merge,
        crate::policy::Source::Manifest,
    )
    .await
    .map_err(|e| format!("Failed to update {kind}/{name}: {e}"))?;

    let result = serde_json::json!({
        "name": patched.metadata.name,
        "namespace": patched.metadata.namespace,
        "kind": kind,
        "resource_version": patched.metadata.resource_version,
    });
    serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
}

async fn delete_custom_resource(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let group = args["group"].as_str().ok_or("group is required")?;
    let version = args["version"].as_str().ok_or("version is required")?;
    let kind = args["kind"].as_str().ok_or("kind is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;
    let namespace = args["namespace"].as_str();

    let api = get_dynamic_api(client, group, version, kind, namespace).await?;
    api.delete(name, &DeleteParams::default())
        .await
        .map_err(|e| format!("Failed to delete {kind}/{name}: {e}"))?;

    let result = serde_json::json!({
        "deleted": name,
        "kind": kind,
    });
    serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_definitions_returns_seven_tools() {
        let defs = tool_definitions();
        assert_eq!(defs.len(), 7);

        let names: Vec<&str> = defs.iter().map(|d| d["name"].as_str().unwrap()).collect();

        let mut unique = names.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(names.len(), unique.len(), "tool names must be unique");

        assert!(names.contains(&"list_crds"));
        assert!(names.contains(&"get_crd"));
        assert!(names.contains(&"list_custom_resources"));
        assert!(names.contains(&"get_custom_resource"));
        assert!(names.contains(&"create_custom_resource"));
        assert!(names.contains(&"update_custom_resource"));
        assert!(names.contains(&"delete_custom_resource"));
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
    fn list_crds_tool_schema() {
        let defs = tool_definitions();
        let tool = defs
            .iter()
            .find(|d| d["name"] == "list_crds")
            .expect("list_crds tool must exist");

        let schema = &tool["inputSchema"];
        let required = schema["required"].as_array().unwrap();
        assert_eq!(required.len(), 0);
    }

    #[test]
    fn get_crd_tool_schema() {
        let defs = tool_definitions();
        let tool = defs
            .iter()
            .find(|d| d["name"] == "get_crd")
            .expect("get_crd tool must exist");

        let schema = &tool["inputSchema"];
        let required = schema["required"].as_array().unwrap();
        assert_eq!(required.len(), 1);
        assert!(required.contains(&serde_json::json!("name")));
    }

    #[test]
    fn list_custom_resources_tool_schema() {
        let defs = tool_definitions();
        let tool = defs
            .iter()
            .find(|d| d["name"] == "list_custom_resources")
            .expect("list_custom_resources tool must exist");

        let schema = &tool["inputSchema"];
        let required = schema["required"].as_array().unwrap();
        assert_eq!(required.len(), 3);
        assert!(required.contains(&serde_json::json!("group")));
        assert!(required.contains(&serde_json::json!("version")));
        assert!(required.contains(&serde_json::json!("kind")));

        let props = schema["properties"].as_object().unwrap();
        assert!(props.contains_key("group"));
        assert!(props.contains_key("version"));
        assert!(props.contains_key("kind"));
        assert!(props.contains_key("namespace"));
    }

    #[test]
    fn get_custom_resource_tool_schema() {
        let defs = tool_definitions();
        let tool = defs
            .iter()
            .find(|d| d["name"] == "get_custom_resource")
            .expect("get_custom_resource tool must exist");

        let schema = &tool["inputSchema"];
        let required = schema["required"].as_array().unwrap();
        assert_eq!(required.len(), 4);
        assert!(required.contains(&serde_json::json!("group")));
        assert!(required.contains(&serde_json::json!("version")));
        assert!(required.contains(&serde_json::json!("kind")));
        assert!(required.contains(&serde_json::json!("name")));

        let props = schema["properties"].as_object().unwrap();
        assert!(props.contains_key("namespace"));
    }

    #[test]
    fn create_custom_resource_tool_schema() {
        let defs = tool_definitions();
        let tool = defs
            .iter()
            .find(|d| d["name"] == "create_custom_resource")
            .expect("create_custom_resource tool must exist");

        let schema = &tool["inputSchema"];
        let required = schema["required"].as_array().unwrap();
        assert_eq!(required.len(), 4);
        assert!(required.contains(&serde_json::json!("group")));
        assert!(required.contains(&serde_json::json!("version")));
        assert!(required.contains(&serde_json::json!("kind")));
        assert!(required.contains(&serde_json::json!("manifest")));

        let props = schema["properties"].as_object().unwrap();
        assert!(props.contains_key("namespace"));
    }

    #[test]
    fn update_custom_resource_tool_schema() {
        let defs = tool_definitions();
        let tool = defs
            .iter()
            .find(|d| d["name"] == "update_custom_resource")
            .expect("update_custom_resource tool must exist");

        let schema = &tool["inputSchema"];
        let required = schema["required"].as_array().unwrap();
        assert_eq!(required.len(), 5);
        assert!(required.contains(&serde_json::json!("group")));
        assert!(required.contains(&serde_json::json!("version")));
        assert!(required.contains(&serde_json::json!("kind")));
        assert!(required.contains(&serde_json::json!("name")));
        assert!(required.contains(&serde_json::json!("patch")));

        let props = schema["properties"].as_object().unwrap();
        assert!(props.contains_key("namespace"));
    }

    #[test]
    fn delete_custom_resource_tool_schema() {
        let defs = tool_definitions();
        let tool = defs
            .iter()
            .find(|d| d["name"] == "delete_custom_resource")
            .expect("delete_custom_resource tool must exist");

        let schema = &tool["inputSchema"];
        let required = schema["required"].as_array().unwrap();
        assert_eq!(required.len(), 4);
        assert!(required.contains(&serde_json::json!("group")));
        assert!(required.contains(&serde_json::json!("version")));
        assert!(required.contains(&serde_json::json!("kind")));
        assert!(required.contains(&serde_json::json!("name")));

        let props = schema["properties"].as_object().unwrap();
        assert!(props.contains_key("namespace"));
    }
}
