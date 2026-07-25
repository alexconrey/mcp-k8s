use futures::{StreamExt, TryStreamExt};
use kube::api::{Api, DynamicObject, WatchEvent, WatchParams};
use kube::core::discovery::Scope;
use kube::core::GroupVersionKind;
use kube::discovery;
use tokio::time::{timeout, Duration};

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
    vec![serde_json::json!({
        "name": "watch_resource",
        "description": "Watch a Kubernetes resource type for changes over a specified duration. Returns all ADDED, MODIFIED, and DELETED events that occurred during the watch window. Since MCP tools are request/response, this collects events for the given duration and returns them as a batch.",
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
                "namespace": {
                    "type": "string",
                    "description": "Kubernetes namespace (omit for cluster-scoped resources or to watch across all namespaces)"
                },
                "label_selector": {
                    "type": "string",
                    "description": "Label selector to filter watched resources (e.g. 'app=nginx')"
                },
                "duration_seconds": {
                    "type": "integer",
                    "description": "How long to watch for events in seconds (default: 10, max: 30)"
                }
            },
            "required": ["api_version", "kind"],
            "additionalProperties": false
        }
    })]
}

pub async fn handle_tool(
    client: &K8sClient,
    name: &str,
    args: &serde_json::Value,
) -> Option<Result<String, String>> {
    let result = match name {
        "watch_resource" => watch_resource(client, args).await,
        _ => return None,
    };
    Some(result)
}

async fn watch_resource(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let api_version = args["api_version"]
        .as_str()
        .ok_or("api_version is required")?;
    let kind = args["kind"].as_str().ok_or("kind is required")?;
    let namespace = args["namespace"].as_str();
    let label_selector = args["label_selector"].as_str();
    let requested = args["duration_seconds"].as_u64().unwrap_or(10);
    let duration_secs = requested.min(30);
    if requested > 30 {
        tracing::warn!(
            requested = requested,
            capped = duration_secs,
            "watch duration capped at 30s"
        );
    }

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
        (Scope::Namespaced, None) => Api::all_with(client.inner().clone(), &ar),
        (Scope::Cluster, _) => Api::all_with(client.inner().clone(), &ar),
    };

    // Build watch params
    let mut wp = WatchParams::default();
    if let Some(sel) = label_selector {
        wp = wp.labels(sel);
    }

    // Watch for events, collecting them until the duration expires
    let mut events = Vec::new();
    let watch_stream = api
        .watch(&wp, "0")
        .await
        .map_err(|e| format!("Failed to start watch: {e}"))?;
    let mut stream = watch_stream.boxed();

    let _ = timeout(Duration::from_secs(duration_secs), async {
        while let Ok(Some(event)) = stream.try_next().await {
            let entry = match &event {
                WatchEvent::Added(obj) => {
                    serde_json::json!({
                        "type": "ADDED",
                        "name": obj.metadata.name,
                        "namespace": obj.metadata.namespace,
                    })
                }
                WatchEvent::Modified(obj) => {
                    serde_json::json!({
                        "type": "MODIFIED",
                        "name": obj.metadata.name,
                        "namespace": obj.metadata.namespace,
                    })
                }
                WatchEvent::Deleted(obj) => {
                    serde_json::json!({
                        "type": "DELETED",
                        "name": obj.metadata.name,
                        "namespace": obj.metadata.namespace,
                    })
                }
                WatchEvent::Bookmark(_) | WatchEvent::Error(_) => continue,
            };
            events.push(entry);
        }
    })
    .await;

    let result = serde_json::json!({
        "api_version": api_version,
        "kind": kind,
        "namespace": namespace,
        "duration_seconds": duration_secs,
        "events_count": events.len(),
        "events": events,
    });
    serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_definitions_returns_one_tool() {
        let defs = tool_definitions();
        assert_eq!(defs.len(), 1);

        let names: Vec<&str> = defs.iter().map(|d| d["name"].as_str().unwrap()).collect();

        let mut unique = names.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(names.len(), unique.len(), "tool names must be unique");

        assert!(names.contains(&"watch_resource"));
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
    fn watch_resource_tool_schema() {
        let defs = tool_definitions();
        let watch = defs
            .iter()
            .find(|d| d["name"] == "watch_resource")
            .expect("watch_resource tool must exist");

        let schema = &watch["inputSchema"];
        let required = schema["required"].as_array().unwrap();
        assert_eq!(required.len(), 2);
        assert!(required.contains(&serde_json::json!("api_version")));
        assert!(required.contains(&serde_json::json!("kind")));

        let props = schema["properties"].as_object().unwrap();
        assert!(props.contains_key("api_version"));
        assert!(props.contains_key("kind"));
        assert!(props.contains_key("namespace"));
        assert!(props.contains_key("label_selector"));
        assert!(props.contains_key("duration_seconds"));
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
}
