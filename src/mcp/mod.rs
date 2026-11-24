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
strategies, Service and Ingress options. apply_manifest is a last resort for kinds or fields the typed \
tools don't cover; it enforces the same guardrails, so it can't do anything the typed tools refuse. \
Writes are checked against a workload policy (no privileged containers, host namespaces, hostPath, host \
ports, extra capabilities, NodePort/LoadBalancer Services or ingress snippets unless the operator allows \
them). A rejection names the operator setting that would allow it: don't try to work around it with \
another tool, report it to the user.";
pub use handlers::handle_tool;
pub use protocol::{
    error_response, method_not_found, success_response, JsonRpcError, JsonRpcRequest,
    JsonRpcResponse,
};

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
