use crate::cluster::ClusterManager;

/// MCP tool definitions for cluster management operations.
pub fn tool_definitions() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "name": "list_clusters",
            "description": "List all configured Kubernetes cluster contexts and show which is active.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "switch_cluster",
            "description": "Switch the active Kubernetes cluster context.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Cluster context name to switch to" }
                },
                "required": ["name"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "get_active_cluster",
            "description": "Get the name of the currently active Kubernetes cluster context.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "additionalProperties": false
            }
        }),
    ]
}

/// Handle a cluster management tool call. Returns `Some(result)` if the
/// tool name is recognized, `None` otherwise.
pub async fn handle_tool(
    manager: &ClusterManager,
    name: &str,
    args: &serde_json::Value,
) -> Option<Result<String, String>> {
    let result = match name {
        "list_clusters" => list_clusters(manager).await,
        "switch_cluster" => switch_cluster(manager, args).await,
        "get_active_cluster" => get_active_cluster(manager).await,
        _ => return None,
    };
    Some(result)
}

async fn list_clusters(manager: &ClusterManager) -> Result<String, String> {
    let clusters = manager.list_clusters().await;
    let active = manager.active_name().await;

    let lines: Vec<String> = clusters
        .iter()
        .map(|name| {
            if name == &active {
                format!("* {name} (active)")
            } else {
                format!("  {name}")
            }
        })
        .collect();

    Ok(lines.join("\n"))
}

async fn switch_cluster(
    manager: &ClusterManager,
    args: &serde_json::Value,
) -> Result<String, String> {
    let name = args["name"]
        .as_str()
        .ok_or_else(|| "name is required".to_string())?;
    manager.switch(name).await?;
    Ok(format!("Switched active cluster to '{name}'"))
}

async fn get_active_cluster(manager: &ClusterManager) -> Result<String, String> {
    let active = manager.active_name().await;
    Ok(active)
}
