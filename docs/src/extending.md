# Extending mcp-k8s

This guide walks through adding a new resource module to mcp-k8s. Each Kubernetes resource type gets its own module in `src/resources/` following a consistent pattern.

## Resource Module Pattern

Every resource module exports two public functions:

- `tool_definitions() -> Vec<serde_json::Value>` -- returns JSON schemas for each tool the module provides.
- `handle_tool(client, name, args) -> Option<Result<String, String>>` -- dispatches a tool call by name, returning `None` if the tool name is not recognized.

A typical module also contains:

- A namespace-checked API factory function (`fn api(...)`)
- One or more summary structs for token-efficient output
- Extraction functions that convert Kubernetes API objects into summaries
- Private handler functions for each tool (list, get, create, delete, etc.)
- Unit tests

## File Structure

```
src/resources/
├── mod.rs            # Aggregates all modules
├── deployment.rs     # Example: Deployment tools
├── pod.rs            # Example: Pod tools
├── limitrange.rs     # Example: LimitRange tools (good reference for CRUD)
└── widget.rs         # Your new module goes here
```

## Step-by-Step Walkthrough

The following example adds a hypothetical "Widget" custom resource. Adapt this pattern for any Kubernetes resource type.

### 1. Create the module file

Create `src/resources/widget.rs`:

```rust
use std::collections::BTreeMap;

use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::api::{DeleteParams, ListParams, PostParams};
use serde::Serialize;

use crate::client::K8sClient;
```

### 2. Define the API factory

For **namespaced** resources, check the namespace allowlist before returning the API handle:

```rust
fn api(client: &K8sClient, ns: &str) -> Result<kube::Api<Widget>, String> {
    if !client.is_namespace_allowed(ns) {
        return Err(format!("Namespace '{ns}' is not in the allowed list"));
    }
    Ok(kube::Api::namespaced(client.inner().clone(), ns))
}
```

For **cluster-scoped** resources (like Nodes, ClusterRoles, StorageClasses), use `kube::Api::all` instead and skip the namespace check:

```rust
fn api(client: &K8sClient) -> kube::Api<Widget> {
    kube::Api::all(client.inner().clone())
}
```

### 3. Define the summary struct

Summary structs provide token-efficient responses. Only include fields that are useful for the MCP client -- strip out managed fields, last-applied-configuration annotations, and other noise.

```rust
#[derive(Serialize, Debug)]
pub struct WidgetSummary {
    pub name: String,
    pub namespace: String,
    pub color: String,
    pub size: i32,
    pub created_at: Option<String>,
}
```

### 4. Write the extraction function

Convert the full Kubernetes API object into the summary:

```rust
fn extract_summary(widget: &Widget) -> WidgetSummary {
    let meta = &widget.metadata;
    WidgetSummary {
        name: meta.name.clone().unwrap_or_default(),
        namespace: meta.namespace.clone().unwrap_or_default(),
        color: widget.spec.as_ref().map(|s| s.color.clone()).unwrap_or_default(),
        size: widget.spec.as_ref().map(|s| s.size).unwrap_or(0),
        created_at: meta.creation_timestamp.as_ref().map(|t| t.0.to_string()),
    }
}
```

### 5. Define tool definitions

Each tool definition is a JSON object with `name`, `description`, and `inputSchema`. The schema must set `"additionalProperties": false`.

```rust
pub fn tool_definitions() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "name": "list_widgets",
            "description": "List Widgets in a namespace. Returns name, namespace, color, size, and created_at.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "label_selector": { "type": "string", "description": "Label selector to filter results (e.g. app=nginx)" },
                    "field_selector": { "type": "string", "description": "Field selector to filter results (e.g. metadata.name=my-widget)" }
                },
                "required": ["namespace"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "get_widget",
            "description": "Get a Widget by name. Returns full spec, labels, and annotations.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "Widget name" }
                },
                "required": ["namespace", "name"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "create_widget",
            "description": "Create a Widget in a namespace.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "Widget name" },
                    "color": { "type": "string", "description": "Widget color" },
                    "size": { "type": "integer", "description": "Widget size" }
                },
                "required": ["namespace", "name", "color", "size"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "delete_widget",
            "description": "Delete a Widget by name from a namespace.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "Widget name" }
                },
                "required": ["namespace", "name"],
                "additionalProperties": false
            }
        }),
    ]
}
```

### 6. Implement the handler dispatch

The `handle_tool` function matches the tool name and delegates to private handler functions. It returns `None` for unrecognized tools so the dispatch chain can continue.

```rust
pub async fn handle_tool(
    client: &K8sClient,
    name: &str,
    args: &serde_json::Value,
) -> Option<Result<String, String>> {
    let result = match name {
        "list_widgets" => list_widgets(client, args).await,
        "get_widget" => get_widget(client, args).await,
        "create_widget" => create_widget(client, args).await,
        "delete_widget" => delete_widget(client, args).await,
        _ => return None,
    };
    Some(result)
}
```

### 7. Implement the handler functions

Each handler extracts arguments from the JSON `args`, calls the Kubernetes API, and returns pretty-printed JSON.

**List handler** -- supports `label_selector` and `field_selector`:

```rust
async fn list_widgets(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let widget_api = api(client, ns)?;

    let mut lp = ListParams::default();
    if let Some(sel) = args["label_selector"].as_str() {
        lp = lp.labels(sel);
    }
    if let Some(sel) = args["field_selector"].as_str() {
        lp = lp.fields(sel);
    }

    let list = widget_api.list(&lp).await.map_err(|e| e.to_string())?;
    let summaries: Vec<WidgetSummary> = list.iter().map(extract_summary).collect();
    serde_json::to_string_pretty(&summaries).map_err(|e| e.to_string())
}
```

**Get handler**:

```rust
async fn get_widget(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;
    let widget_api = api(client, ns)?;
    let widget = widget_api.get(name).await.map_err(|e| e.to_string())?;

    let meta = &widget.metadata;
    let result = serde_json::json!({
        "name": meta.name.clone().unwrap_or_default(),
        "namespace": meta.namespace.clone().unwrap_or_default(),
        "spec": widget.spec,
        "labels": meta.labels.clone().unwrap_or_default(),
        "annotations": meta.annotations.clone().unwrap_or_default(),
    });
    serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
}
```

**Create handler** -- adds the `managed-by: mcp-k8s` label:

```rust
async fn create_widget(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;
    let color = args["color"].as_str().ok_or("color is required")?;
    let size = args["size"].as_i64().ok_or("size is required")? as i32;

    let mut labels = BTreeMap::new();
    labels.insert(
        "app.kubernetes.io/managed-by".to_string(),
        "mcp-k8s".to_string(),
    );

    let widget = Widget {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            namespace: Some(ns.to_string()),
            labels: Some(labels),
            ..Default::default()
        },
        spec: Some(WidgetSpec { color: color.to_string(), size }),
    };

    let widget_api = api(client, ns)?;
    let created = widget_api
        .create(&PostParams::default(), &widget)
        .await
        .map_err(|e| e.to_string())?;

    let summary = extract_summary(&created);
    serde_json::to_string_pretty(&summary).map_err(|e| e.to_string())
}
```

**Delete handler**:

```rust
async fn delete_widget(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;

    let widget_api = api(client, ns)?;
    widget_api
        .delete(name, &DeleteParams::default())
        .await
        .map_err(|e| e.to_string())?;

    let result = serde_json::json!({
        "deleted": name,
        "namespace": ns,
    });
    serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
}
```

### 8. Add tests

Every module should have unit tests that verify:

- Tool definitions return the expected number of unique tools
- All definitions have `name`, `description`, and `inputSchema`
- Summary structs serialize correctly
- Extraction functions handle both populated and empty inputs

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_definitions_returns_unique_tools() {
        let defs = tool_definitions();
        assert_eq!(defs.len(), 4);

        let names: Vec<&str> = defs.iter().map(|d| d["name"].as_str().unwrap()).collect();
        let mut unique = names.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(names.len(), unique.len(), "tool names must be unique");
    }

    #[test]
    fn tool_definitions_have_input_schema() {
        let defs = tool_definitions();
        for def in &defs {
            assert!(def.get("name").is_some(), "tool must have a name");
            assert!(def.get("description").is_some(), "tool must have a description");
            let schema = def.get("inputSchema").expect("tool must have inputSchema");
            assert_eq!(schema["type"], "object");
            assert_eq!(schema["additionalProperties"], false);
        }
    }

    #[test]
    fn widget_summary_serialization() {
        let summary = WidgetSummary {
            name: "my-widget".to_string(),
            namespace: "default".to_string(),
            color: "blue".to_string(),
            size: 42,
            created_at: Some("2024-01-01T00:00:00Z".to_string()),
        };
        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["name"], "my-widget");
        assert_eq!(json["color"], "blue");
        assert_eq!(json["size"], 42);
    }
}
```

## Registering in mod.rs

After creating your module file, register it in `src/resources/mod.rs`:

### 1. Add the module declaration

Add a `pub mod widget;` line to the module declarations at the top of the file, in alphabetical order:

```rust
pub mod volume_attributes;
pub mod widget;  // <-- add this
```

### 2. Add to `all_tool_definitions()`

Add `tools.extend(widget::tool_definitions());` to the `all_tool_definitions()` function:

```rust
pub fn all_tool_definitions() -> Vec<serde_json::Value> {
    let mut tools = Vec::new();
    // ... existing modules ...
    tools.extend(volume_attributes::tool_definitions());
    tools.extend(widget::tool_definitions());  // <-- add this
    tools
}
```

### 3. Add to `handle_tool()`

Add the dispatch call to the `handle_tool()` function:

```rust
pub async fn handle_tool(
    client: &K8sClient,
    name: &str,
    args: &serde_json::Value,
) -> Option<Result<String, String>> {
    // ... existing modules ...
    if let result @ Some(_) = volume_attributes::handle_tool(client, name, args).await {
        return result;
    }
    if let result @ Some(_) = widget::handle_tool(client, name, args).await {  // <-- add this
        return result;
    }
    None
}
```

That is all that is needed. No other registration steps are required.

## How Permissions Integrate Automatically

You do **not** need to add any permission-checking code to your module. The permission system works automatically based on tool name prefixes:

| Tool Name Prefix | Action | Example |
|-----------------|--------|---------|
| `list_`, `get_` | Read | `list_widgets`, `get_widget` |
| `create_` | Create | `create_widget` |
| `update_`, `scale_`, `restart_`, `rollback_` | Update | `update_widget` |
| `delete_`, `evict_` | Delete | `delete_widget` |

The `ActionPermissions::action_for_tool()` function in `src/permissions.rs` maps tool names to actions using these prefixes. The MCP handlers layer checks permissions **before** dispatching to your module:

1. **`tools/list`** -- disabled tools are filtered out of the response, so the client never sees them.
2. **`tools/call`** -- if a tool is disabled, the call is rejected with an error before your `handle_tool` function is reached.

Per-resource overrides use the `"<resource>-<action>"` format (e.g., `--disable widget-delete`), where `<resource>` is the part of the tool name after the prefix.

## Conventions

Follow these conventions for consistency across modules:

1. **`label_selector`** -- all list tools should accept an optional `label_selector` parameter for filtering by labels.
2. **`field_selector`** -- list tools should accept `field_selector` where the Kubernetes API supports it.
3. **`managed-by` label** -- all create operations must add `app.kubernetes.io/managed-by: mcp-k8s` to the resource labels.
4. **`serde_json::to_string_pretty`** -- all handler output must use pretty-printed JSON for readability.
5. **Error handling** -- use `.map_err(|e| e.to_string())` to convert kube errors into strings. Return early with descriptive error messages for missing required arguments (e.g., `"namespace is required"`).
6. **Namespace checking** -- always call `client.is_namespace_allowed(ns)` in the API factory for namespaced resources. The error message should be: `"Namespace '{ns}' is not in the allowed list"`.
7. **`additionalProperties: false`** -- every `inputSchema` must include this to prevent extra arguments from being passed.
8. **Tool naming** -- use singular nouns for get/create/delete (e.g., `get_widget`, `create_widget`) and plural for list (e.g., `list_widgets`).
