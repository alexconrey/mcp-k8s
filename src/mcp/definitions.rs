use crate::permissions::ActionPermissions;

pub fn tool_definitions(permissions: &ActionPermissions) -> Vec<serde_json::Value> {
    let mut tools = vec![
        serde_json::json!({
            "name": "list_namespaces",
            "description": "List all Kubernetes namespaces visible to the server.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "list_deployments",
            "description": "List deployments in a namespace with replica counts and status.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "label_selector": { "type": "string", "description": "Label selector to filter results (e.g. app=nginx)" },
                    "field_selector": { "type": "string", "description": "Field selector to filter results (e.g. metadata.name=foo)" }
                },
                "required": ["namespace"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "get_deployment",
            "description": "Get detailed info for a single deployment including pods and ingresses.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "Deployment name" }
                },
                "required": ["namespace", "name"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "get_pod_logs",
            "description": "Fetch logs from a pod. Optionally scope to a container and limit line count.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "pod_name": { "type": "string", "description": "Pod name" },
                    "tail_lines": { "type": "integer", "description": "Number of recent lines to return" },
                    "container": { "type": "string", "description": "Container name (optional, defaults to first)" }
                },
                "required": ["namespace", "pod_name"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "get_events",
            "description": "List Kubernetes events in a namespace, optionally filtered by resource name and/or label selector.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "resource_name": { "type": "string", "description": "Filter to events involving this resource name" },
                    "label_selector": { "type": "string", "description": "Label selector to filter results (e.g. app=nginx)" },
                    "field_selector": { "type": "string", "description": "Field selector to filter results (e.g. involvedObject.kind=Pod, reason=Killing)" }
                },
                "required": ["namespace"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "get_deployment_history",
            "description": "List revision history (ReplicaSets) for a deployment.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "Deployment name" }
                },
                "required": ["namespace", "name"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "get_build_logs",
            "description": "Fetch logs from a Kubernetes Job's pod (e.g. a build job).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "job_name": { "type": "string", "description": "Job name" }
                },
                "required": ["namespace", "job_name"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "list_ingresses",
            "description": "List ingresses in a namespace with hosts, classes, and addresses.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "label_selector": { "type": "string", "description": "Label selector to filter results (e.g. app=nginx)" }
                },
                "required": ["namespace"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "get_metrics",
            "description": "Get pod resource usage metrics (CPU/memory) from metrics-server.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "label_selector": { "type": "string", "description": "Label selector to scope pods (e.g. app=foo)" }
                },
                "required": ["namespace"],
                "additionalProperties": false
            }
        }),
    ];
    tools.extend(crate::resources::all_tool_definitions());
    tools.retain(|tool| {
        let name = tool["name"].as_str().unwrap_or("");
        permissions.is_tool_allowed(name)
    });
    tools
}
