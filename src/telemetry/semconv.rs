//! Attribute, span and metric names used by mcp-k8s telemetry.
//!
//! Every name lives here so a rename touches one file. Standard names follow
//! the OpenTelemetry GenAI conventions for the Model Context Protocol
//! ([`open-telemetry/semantic-conventions-genai`], `docs/gen-ai/mcp.md`,
//! status *Development*), implemented against commit [`IMPLEMENTED_AGAINST`].
//! Names that are ours use the `mcp_k8s.` prefix.
//!
//! The conventions are still in development, so expect renames. When one
//! happens upstream, change the constant here and note it in the changelog.
//!
//! [`open-telemetry/semantic-conventions-genai`]: https://github.com/open-telemetry/semantic-conventions-genai/blob/main/docs/gen-ai/mcp.md

/// The `semantic-conventions-genai` commit (2026-11-20, "Use rpc.status_code
/// instead of deprecated rpc.response.status_code in MCP") these names were
/// checked against.
pub const IMPLEMENTED_AGAINST: &str = "7cb940f24deafb12198ed6566c2272cdc77093c2";

// ---------------------------------------------------------------------------
// tracing-opentelemetry special fields
// ---------------------------------------------------------------------------

/// Span name override read by `tracing-opentelemetry`. It must be set when the
/// span is created: `tracing-opentelemetry` 0.34 ignores later changes once
/// the span has been entered.
pub const OTEL_NAME: &str = "otel.name";
/// Span kind read by `tracing-opentelemetry` (`server` for the MCP span).
pub const OTEL_KIND: &str = "otel.kind";
/// Span status read by `tracing-opentelemetry` (`error` on failure).
pub const OTEL_STATUS_CODE: &str = "otel.status_code";

// ---------------------------------------------------------------------------
// MCP / JSON-RPC (standard)
// ---------------------------------------------------------------------------

/// The JSON-RPC method, e.g. `tools/call`.
pub const MCP_METHOD_NAME: &str = "mcp.method.name";
/// The MCP protocol version in use, e.g. `2025-11-25`.
pub const MCP_PROTOCOL_VERSION: &str = "mcp.protocol.version";
/// The MCP session id. mcp-k8s doesn't issue `Mcp-Session-Id`, so it never
/// records this; embedders with sessions can.
pub const MCP_SESSION_ID: &str = "mcp.session.id";
/// The resource URI of `resources/read`.
pub const MCP_RESOURCE_URI: &str = "mcp.resource.uri";
/// The JSON-RPC request `id`, as a string. Not recorded for notifications.
pub const JSONRPC_REQUEST_ID: &str = "jsonrpc.request.id";
/// The JSON-RPC error code of an error response, as a string (`-32000`).
pub const RPC_STATUS_CODE: &str = "rpc.status_code";
/// The class of error the operation ended with. See [`error_type`].
pub const ERROR_TYPE: &str = "error.type";
/// The peer address (the TCP peer for HTTP, not a forwarded address).
pub const CLIENT_ADDRESS: &str = "client.address";
/// The peer port.
pub const CLIENT_PORT: &str = "client.port";
/// `pipe` for stdio, `tcp` for HTTP.
pub const NETWORK_TRANSPORT: &str = "network.transport";
/// `http` for the HTTP transport.
pub const NETWORK_PROTOCOL_NAME: &str = "network.protocol.name";

// ---------------------------------------------------------------------------
// GenAI (standard)
// ---------------------------------------------------------------------------

/// The tool of a `tools/call`.
pub const GEN_AI_TOOL_NAME: &str = "gen_ai.tool.name";
/// `execute_tool` for `tools/call`, unset otherwise.
pub const GEN_AI_OPERATION_NAME: &str = "gen_ai.operation.name";
/// The prompt of a `prompts/get`.
pub const GEN_AI_PROMPT_NAME: &str = "gen_ai.prompt.name";

// ---------------------------------------------------------------------------
// Kubernetes (standard where a standard name exists)
// ---------------------------------------------------------------------------

/// The cluster (kubeconfig context) the request ran against.
pub const K8S_CLUSTER_NAME: &str = "k8s.cluster.name";
/// The namespace a tool targets.
pub const K8S_NAMESPACE_NAME: &str = "k8s.namespace.name";
/// The kind a tool targets (`Deployment`). There is no generic standard name.
pub const MCP_K8S_RESOURCE_KIND: &str = "mcp_k8s.resource.kind";
/// The object name a tool targets.
pub const MCP_K8S_RESOURCE_NAME: &str = "mcp_k8s.resource.name";
/// The API version of the kind a tool targets (`apps/v1`).
pub const MCP_K8S_RESOURCE_API_VERSION: &str = "mcp_k8s.resource.api_version";

// ---------------------------------------------------------------------------
// mcp-k8s
// ---------------------------------------------------------------------------

/// `read`, `write`, `delete` or `exec`. See [`super::ActionClass`].
pub const MCP_K8S_ACTION: &str = "mcp_k8s.action";
/// `allowed` or `denied`.
pub const MCP_K8S_DECISION: &str = "mcp_k8s.decision";
/// The setting or policy rule that denied the call:
/// - `global:<action>`, `override:<resource>-<action>` or
///   `apply_manifest:disabled` for [`crate::permissions::ActionPermissions`]
///   (see `ActionPermissions::denied_by`);
/// - `policy:<field>[,<field>…]` for workload guardrails, naming the policy
///   settings that would allow the call (sorted, de-duplicated);
/// - embedders' own rules otherwise.
///
/// Guardrails report it with [`super::record_denial`]. Span only, never on
/// the metric.
pub const MCP_K8S_DENIED_RULE: &str = "mcp_k8s.denied_rule";
/// How the call ended: `ok`, `error`, `denied` or `cancelled`.
pub const MCP_K8S_OUTCOME: &str = "mcp_k8s.outcome";
/// `clientInfo.name` from `initialize`. Self-reported by the client: a label,
/// never an identity.
pub const MCP_K8S_CLIENT_NAME: &str = "mcp_k8s.client.name";
/// `clientInfo.version` from `initialize`. Self-reported, like the name.
pub const MCP_K8S_CLIENT_VERSION: &str = "mcp_k8s.client.version";
/// The authenticated caller. Reserved: never recorded until SSO lands.
pub const MCP_K8S_ACTOR: &str = "mcp_k8s.actor";
/// A per-request id for correlating log lines, recorded with or without OTLP.
pub const MCP_K8S_REQUEST_ID: &str = "mcp_k8s.request_id";
/// The top-level argument keys of a tool call, sorted and comma-separated.
pub const MCP_K8S_TOOL_ARGUMENT_KEYS: &str = "mcp_k8s.tool.argument_keys";
/// The allowlisted argument values of a tool call, as a JSON object. See
/// [`super::summarize`]. Deliberately not `gen_ai.tool.call.arguments`.
pub const MCP_K8S_TOOL_ARGUMENTS: &str = "mcp_k8s.tool.arguments";
/// The W3C trace id, recorded by applications that export OTLP so log lines
/// can be joined with traces.
pub const TRACE_ID: &str = "trace_id";
/// Wall time of the call in milliseconds, on the end-of-call log event.
pub const DURATION_MS: &str = "duration_ms";

// ---------------------------------------------------------------------------
// Metric
// ---------------------------------------------------------------------------

/// Histogram of request duration in seconds, with only low-cardinality
/// attributes ([`METRIC_ATTRIBUTES`]).
pub const MCP_SERVER_OPERATION_DURATION: &str = "mcp.server.operation.duration";
/// The unit of [`MCP_SERVER_OPERATION_DURATION`] (UCUM seconds).
pub const MCP_SERVER_OPERATION_DURATION_UNIT: &str = "s";
/// The bucket boundaries the conventions ask for, in seconds.
pub const MCP_SERVER_OPERATION_DURATION_BUCKETS: &[f64] = &[
    0.01, 0.02, 0.05, 0.1, 0.2, 0.5, 1.0, 2.0, 5.0, 10.0, 30.0, 60.0, 120.0, 300.0,
];
/// The event field that `tracing-opentelemetry`'s `MetricsLayer` turns into a
/// histogram observation of [`MCP_SERVER_OPERATION_DURATION`].
pub const HISTOGRAM_MCP_SERVER_OPERATION_DURATION: &str = "histogram.mcp.server.operation.duration";
/// Every attribute the duration metric can carry. Applications should give
/// their meter provider a view that keeps only these (the mcp-k8s binary
/// does), so a mistake can't add a high-cardinality attribute.
pub const METRIC_ATTRIBUTES: &[&str] = &[
    MCP_METHOD_NAME,
    GEN_AI_TOOL_NAME,
    GEN_AI_OPERATION_NAME,
    ERROR_TYPE,
    MCP_K8S_ACTION,
    MCP_K8S_DECISION,
];

// ---------------------------------------------------------------------------
// Event targets and names
// ---------------------------------------------------------------------------

/// `tracing` target of the request span and the end-of-call log events.
pub const TARGET: &str = "mcp_k8s::telemetry";
/// `tracing` target of the metric events. They carry no message; filter this
/// target out of console logs and OTLP log export (the binary does).
pub const METRICS_TARGET: &str = "mcp_k8s::telemetry::metrics";
/// The `tracing` span name of the MCP request span (shown in console logs;
/// OTLP uses [`OTEL_NAME`]).
pub const SERVER_SPAN_NAME: &str = "mcp_request";

/// Values of [`ERROR_TYPE`] that mcp-k8s sets itself. JSON-RPC errors that
/// don't come from a tool use the error code instead (`-32000`).
pub mod error_type {
    /// The tool ran and returned an error.
    pub const TOOL_ERROR: &str = "tool_error";
    /// A permission setting or policy rule refused the call.
    pub const PERMISSION_DENIED: &str = "permission_denied";
    /// The call was dropped before it finished (client went away).
    pub const CANCELLED: &str = "cancelled";
    /// `tools/call` named a tool the server doesn't have.
    pub const UNKNOWN_TOOL: &str = "unknown_tool";
}

/// `gen_ai.operation.name` for tool calls.
pub const EXECUTE_TOOL: &str = "execute_tool";
/// The `mcp.method.name` of tool calls.
pub const TOOLS_CALL: &str = "tools/call";
/// Used in span names and on the metric for methods outside the well-known
/// set, so a client can't create unbounded names.
pub const OTHER: &str = "_OTHER";

/// The well-known `mcp.method.name` values from the conventions. Others are
/// reported as [`OTHER`] in span names and on the metric.
pub const KNOWN_METHODS: &[&str] = &[
    "completion/complete",
    "elicitation/create",
    "initialize",
    "logging/setLevel",
    "notifications/cancelled",
    "notifications/initialized",
    "notifications/message",
    "notifications/progress",
    "notifications/prompts/list_changed",
    "notifications/resources/list_changed",
    "notifications/resources/updated",
    "notifications/roots/list_changed",
    "notifications/tools/list_changed",
    "ping",
    "prompts/get",
    "prompts/list",
    "resources/list",
    "resources/read",
    "resources/subscribe",
    "resources/templates/list",
    "resources/unsubscribe",
    "roots/list",
    "sampling/createMessage",
    "tools/call",
    "tools/list",
];
