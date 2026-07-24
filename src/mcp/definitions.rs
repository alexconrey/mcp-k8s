use crate::permissions::ActionPermissions;

pub fn tool_definitions(permissions: &ActionPermissions) -> Vec<serde_json::Value> {
    let mut tools = vec![
        serde_json::json!({
            "name": "get_namespaces",
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
        serde_json::json!({
            "name": "create_service",
            "description": "Create a Kubernetes ClusterIP Service pointing to pods with a matching app label.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "Service name (also used as the app label selector)" },
                    "port": { "type": "integer", "description": "Service port (default: 80)" },
                    "target_port": { "type": "integer", "description": "Container port to forward to (defaults to same as port)" }
                },
                "required": ["namespace", "name"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "create_ingress",
            "description": "Create a Kubernetes Ingress resource. Automatically creates a backing ClusterIP Service if one doesn't exist.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "Ingress name" },
                    "host": { "type": "string", "description": "Hostname for the ingress rule (e.g. myapp.example.com)" },
                    "service_name": { "type": "string", "description": "Backend service name" },
                    "service_port": { "type": "integer", "description": "Backend service port (default: 80)" },
                    "path": { "type": "string", "description": "URL path (default: /)" },
                    "path_type": { "type": "string", "description": "Path matching type (default: Prefix)", "enum": ["Prefix", "Exact", "ImplementationSpecific"] },
                    "ingress_class": { "type": "string", "description": "IngressClass name (e.g. alb, nginx)" },
                    "annotations": { "type": "object", "description": "Ingress annotations as key-value pairs", "additionalProperties": { "type": "string" } }
                },
                "required": ["namespace", "name", "service_name"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "update_ingress",
            "description": "Update an existing Kubernetes Ingress resource (host, paths, annotations, TLS).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "Ingress name" },
                    "host": { "type": "string", "description": "Hostname for the ingress rule" },
                    "service_name": { "type": "string", "description": "Backend service name" },
                    "service_port": { "type": "integer", "description": "Backend service port (default: 80)" },
                    "path": { "type": "string", "description": "URL path (default: /)" },
                    "path_type": { "type": "string", "description": "Path matching type (default: Prefix)" },
                    "ingress_class": { "type": "string", "description": "IngressClass name" },
                    "annotations": { "type": "object", "description": "Ingress annotations as key-value pairs", "additionalProperties": { "type": "string" } }
                },
                "required": ["namespace", "name", "service_name"],
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
