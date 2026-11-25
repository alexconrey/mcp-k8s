pub mod definitions;
pub mod handlers;
pub mod protocol;

pub use definitions::tool_definitions;

/// Guidance for MCP clients, sent as `instructions` in the `initialize`
/// response. Library users can include it in their own instructions.
pub const SERVER_INSTRUCTIONS: &str = "Use the typed create_*/update_* tools (create_pod, create_job, \
create_cronjob, create_deployment, create_statefulset, create_daemonset, create_service, create_ingress, \
create_configmap, create_secret, ...) to change the cluster. They cover containers, init containers and \
sidecars, env from secrets/configmaps, probes, resources, volumes, scheduling, security settings, rollout \
strategies, Service and Ingress options. Writes are checked against a workload policy (no privileged \
containers, host namespaces, hostPath, host ports, extra capabilities, NodePort/LoadBalancer Services or \
ingress snippets unless the operator allows them). A rejection names the operator setting that would allow \
it: don't try to work around it with another tool, report it to the user. apply_manifest, if the operator \
has enabled it, is a raw server-side apply that skips the workload policy entirely. Only use it when the \
user explicitly asks for a raw manifest to be applied: show them the full manifest and get their \
confirmation first. Never pick it on your own, and never use it to get around a rejection or a missing \
typed tool field.";
pub use handlers::handle_tool;
pub use protocol::{
    error_response, method_not_found, success_response, JsonRpcError, JsonRpcRequest,
    JsonRpcResponse,
};

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
