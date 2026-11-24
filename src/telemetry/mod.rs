//! Telemetry for MCP requests and tool calls, as plain [`tracing`].
//!
//! The library emits `tracing` spans and events named after the
//! OpenTelemetry MCP conventions ([`semconv`]). It never depends on an
//! `opentelemetry` crate: OpenTelemetry's global providers are statics inside
//! the `opentelemetry` crate, so a library and an application on different
//! 0.x versions would silently lose telemetry. `tracing` 0.1 has no such
//! skew. The application turns the `tracing` data into OTLP with
//! `tracing-opentelemetry` (spans, plus its `MetricsLayer` for metrics) and
//! `opentelemetry-appender-tracing` (logs). The `mcp-k8s` binary does exactly
//! that (see `src/otel.rs`).
//!
//! # What gets emitted
//!
//! For every MCP request:
//!
//! - **One SERVER span**, created by the request handler with
//!   [`server_span`]: `tracing` name `mcp_request`, OTLP name
//!   `{mcp.method.name} {target}` (e.g. `tools/call apply_manifest`). For a
//!   tool call it carries the tool, action, decision, target, argument keys
//!   and allowlisted argument values.
//! - **One metric observation**: an event on target
//!   [`semconv::METRICS_TARGET`] with the field
//!   `histogram.mcp.server.operation.duration = <seconds>` and only
//!   low-cardinality fields ([`semconv::METRIC_ATTRIBUTES`]).
//!   `tracing-opentelemetry`'s `MetricsLayer` turns it into the
//!   `mcp.server.operation.duration` histogram, with every other field as an
//!   attribute. `MetricsLayer` creates instruments with no unit or buckets, so
//!   give the meter provider a view with unit `s`, the conventions' buckets
//!   ([`semconv::MCP_SERVER_OPERATION_DURATION_BUCKETS`]) and an attribute
//!   allowlist (see the binary's `src/otel.rs`).
//! - **One log event** at the end, on target [`semconv::TARGET`]:
//!   `tool call finished` (tool, outcome, `duration_ms`, decision, action,
//!   `error.type`, target) or `request finished` for other methods.
//!
//! Arguments are redacted by [`summarize`]: only top-level argument keys and
//! the safe scalars in [`ALLOWED_ARGUMENTS`] are ever recorded.
//!
//! # Embedding (deckwatch)
//!
//! [`crate::mcp::handle_tool`] instruments itself, so an embedder that only
//! forwards tools to it gets tool telemetry for free. To get identical
//! telemetry for its own tools and a proper request span:
//!
//! ```no_run
//! use mcp_k8s::telemetry;
//! use tracing::Instrument;
//!
//! # fn is_own_tool(name: &str) -> bool { name == "create_application" }
//! # async fn own_tool(_name: &str, _args: &serde_json::Value) -> Result<String, String> { Ok(String::new()) }
//! # async fn example(client: mcp_k8s::K8sClient, params: serde_json::Value) -> Option<Result<String, String>> {
//! let tool = params["name"].as_str().unwrap_or("");
//! let args = &params["arguments"];
//! let own = is_own_tool(tool);
//! // 1. One SERVER span per request, named after the tool only if it is
//! //    served here (a client-supplied name is unbounded).
//! let target = (own || telemetry::is_known_tool(tool)).then_some(tool);
//! let span = telemetry::server_span("tools/call", target);
//! // 2. With your OpenTelemetry crates, before the span is first entered:
//! //    span.set_parent(cx), with cx from telemetry::W3cTraceContext::from_meta(&params)
//! //    or the HTTP `traceparent` header.
//! async {
//!     if own {
//!         // 3. Your tools get the instrumentation handle_tool uses. A nested
//!         //    handle_tool call (an interceptor) reports into this call.
//!         Some(telemetry::instrument_tool_call(tool, args, own_tool(tool, args)).await)
//!     } else {
//!         // 4. handle_tool instruments itself, into the request span.
//!         mcp_k8s::mcp::handle_tool(&client, tool, args).await
//!     }
//! }
//! .instrument(span)
//! .await
//! # }
//! ```
//!
//! - Create the request span **before** entering it with the final target:
//!   `tracing-opentelemetry` 0.34 ignores `otel.name` changes once a span has
//!   been entered. [`ToolCall`] and `handle_tool` must run with the request
//!   span current (don't wrap them in other spans), or they create their own.
//! - A tool of yours that calls `handle_tool` inside [`ToolCall::run`] is
//!   still one call: the inner `handle_tool` reports into the outer call.
//!   Tools named outside mcp-k8s's prefixes (`trigger_build`) need
//!   [`ToolCall::start_with_action`] for the right `mcp_k8s.action`, and
//!   guardrails of yours can call [`record_denial`].
//! - For other methods, call [`finish_operation`] at the end of the request
//!   so every request yields one metric point and one log line, and
//!   [`record_rpc_status`] for JSON-RPC error responses.
//! - Keep `mcp_k8s=info` in your log filter, filter the target
//!   [`semconv::METRICS_TARGET`] out of console and OTLP logs (it carries
//!   metric events), and give the `MetricsLayer` an unfiltered view of it.

mod call;
pub mod semconv;
mod summary;

#[cfg(test)]
mod tests;

use std::time::Duration;

use tracing::field::Empty;
use tracing::Span;

pub use call::{instrument_tool_call, record_denial, tool_call_active, CallOutcome, ToolCall};
pub use summary::{
    is_known_tool, summarize, ActionClass, Target, ToolCallSummary, ALLOWED_ARGUMENTS,
};

/// The longest method, tool or prompt name copied into a span attribute.
const MAX_NAME_LEN: usize = 128;

/// Create the SERVER span for one MCP request.
///
/// Every attribute in [`semconv`] is declared on it as
/// [`tracing::field::Empty`], so code running inside it (such as
/// [`ToolCall`] and [`crate::mcp::handle_tool`]) can `record` into it later.
/// It sets `otel.kind = "server"`, `mcp.method.name`, and `otel.name` =
/// `{method} {target}`, or `{method}` without a target. Methods outside the
/// conventions' well-known list appear as `_OTHER` in the name. For
/// `prompts/get` it records `gen_ai.prompt.name`; tool attributes are
/// recorded by [`ToolCall`] when the call finishes.
///
/// Record each attribute at most once: the OpenTelemetry SDK keeps every
/// value recorded for a key, so a second `record` exports a duplicate.
///
/// - `target`: for `tools/call` the tool name, for `prompts/get` the prompt
///   name, otherwise `None`. Pass only names you serve: anything else is
///   unbounded client input. It must be known now: `tracing-opentelemetry`
///   0.34 ignores later `otel.name` changes once the span has been entered.
///
/// The span is at `INFO` on target [`semconv::TARGET`]. The caller enters it
/// (e.g. with [`tracing::Instrument::instrument`]) and, if it exports OTLP,
/// sets its remote parent before entering it.
pub fn server_span(method: &str, target: Option<&str>) -> Span {
    let known = semconv::KNOWN_METHODS.contains(&method);
    let name_method = if known { method } else { semconv::OTHER };
    let target = target.filter(|t| !t.is_empty()).map(clip_name);
    let otel_name = match &target {
        Some(t) => format!("{name_method} {t}"),
        None => name_method.to_string(),
    };
    let method_attr = clip_name(method);
    let span = tracing::info_span!(
        target: semconv::TARGET,
        semconv::SERVER_SPAN_NAME,
        { semconv::OTEL_NAME } = otel_name.as_str(),
        { semconv::OTEL_KIND } = "server",
        { semconv::OTEL_STATUS_CODE } = Empty,
        { semconv::MCP_METHOD_NAME } = method_attr.as_str(),
        { semconv::MCP_K8S_REQUEST_ID } = Empty,
        { semconv::TRACE_ID } = Empty,
        { semconv::JSONRPC_REQUEST_ID } = Empty,
        { semconv::MCP_PROTOCOL_VERSION } = Empty,
        { semconv::MCP_SESSION_ID } = Empty,
        { semconv::MCP_RESOURCE_URI } = Empty,
        { semconv::NETWORK_TRANSPORT } = Empty,
        { semconv::NETWORK_PROTOCOL_NAME } = Empty,
        { semconv::CLIENT_ADDRESS } = Empty,
        { semconv::CLIENT_PORT } = Empty,
        { semconv::MCP_K8S_CLIENT_NAME } = Empty,
        { semconv::MCP_K8S_CLIENT_VERSION } = Empty,
        { semconv::MCP_K8S_ACTOR } = Empty,
        { semconv::GEN_AI_TOOL_NAME } = Empty,
        { semconv::GEN_AI_OPERATION_NAME } = Empty,
        { semconv::GEN_AI_PROMPT_NAME } = Empty,
        { semconv::MCP_K8S_ACTION } = Empty,
        { semconv::MCP_K8S_DECISION } = Empty,
        { semconv::MCP_K8S_DENIED_RULE } = Empty,
        { semconv::MCP_K8S_OUTCOME } = Empty,
        { semconv::ERROR_TYPE } = Empty,
        { semconv::RPC_STATUS_CODE } = Empty,
        { semconv::K8S_CLUSTER_NAME } = Empty,
        { semconv::K8S_NAMESPACE_NAME } = Empty,
        { semconv::MCP_K8S_RESOURCE_KIND } = Empty,
        { semconv::MCP_K8S_RESOURCE_NAME } = Empty,
        { semconv::MCP_K8S_RESOURCE_API_VERSION } = Empty,
        { semconv::MCP_K8S_TOOL_ARGUMENT_KEYS } = Empty,
        { semconv::MCP_K8S_TOOL_ARGUMENTS } = Empty,
    );
    // The tool attributes are left to ToolCall, which records each once: the
    // OTel SDK keeps duplicate attribute keys rather than overwriting them.
    if let (Some(t), "prompts/get") = (&target, method) {
        span.record(semconv::GEN_AI_PROMPT_NAME, t.as_str());
    }
    span
}

/// Whether `span` was created by [`server_span`] (and is enabled).
pub fn is_server_span(span: &Span) -> bool {
    span.metadata().is_some_and(|meta| {
        meta.name() == semconv::SERVER_SPAN_NAME
            && meta.target() == semconv::TARGET
            && meta.fields().field(semconv::GEN_AI_TOOL_NAME).is_some()
    })
}

/// The current span, if it is a request span from [`server_span`].
pub(crate) fn current_server_span() -> Option<Span> {
    let span = Span::current();
    is_server_span(&span).then_some(span)
}

/// Record a JSON-RPC error response on a request span: `rpc.status_code`,
/// and span status `ERROR` unless the code is one the conventions count as a
/// caller mistake (`-32700`, `-32600`, `-32601`, `-32602`, `-32002`).
///
/// It doesn't set `error.type`: [`ToolCall`] and [`finish_operation`] do.
pub fn record_rpc_status(span: &Span, code: i64) {
    span.record(semconv::RPC_STATUS_CODE, code.to_string().as_str());
    if !matches!(code, -32700 | -32600 | -32601 | -32602 | -32002) {
        span.record(semconv::OTEL_STATUS_CODE, "error");
    }
}

/// Finish an MCP request that isn't a [`ToolCall`] (any other method, or a
/// `tools/call` that never reached a tool): record the outcome on the current
/// request span, and emit the duration metric event and the `request
/// finished` log event.
///
/// `error_type` is `None` on success, otherwise a low-cardinality class: the
/// JSON-RPC error code (`"-32000"`), or [`semconv::error_type::UNKNOWN_TOOL`].
/// Methods outside the well-known list are reported as `_OTHER` on the
/// metric.
pub fn finish_operation(method: &str, elapsed: Duration, error_type: Option<&str>) {
    let method_attr = if semconv::KNOWN_METHODS.contains(&method) {
        method
    } else {
        semconv::OTHER
    };
    let outcome = if error_type.is_some() { "error" } else { "ok" };
    if let Some(span) = current_server_span() {
        span.record(semconv::MCP_K8S_OUTCOME, outcome);
        if let Some(et) = error_type {
            span.record(semconv::ERROR_TYPE, et);
        }
    }
    tracing::event!(
        target: semconv::METRICS_TARGET,
        tracing::Level::INFO,
        {
            { semconv::HISTOGRAM_MCP_SERVER_OPERATION_DURATION } = elapsed.as_secs_f64(),
            { semconv::MCP_METHOD_NAME } = method_attr,
            { semconv::ERROR_TYPE } = error_type,
        }
    );
    tracing::event!(
        target: semconv::TARGET,
        tracing::Level::INFO,
        {
            { semconv::MCP_METHOD_NAME } = method_attr,
            { semconv::MCP_K8S_OUTCOME } = outcome,
            { semconv::DURATION_MS } = millis(elapsed),
            { semconv::ERROR_TYPE } = error_type,
        },
        "request finished"
    );
}

/// The client's self-description from `initialize`. Self-reported, so use it
/// as a label, never as an identity.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClientInfo {
    /// `clientInfo.name`, cleaned and cut to 128 bytes.
    pub name: Option<String>,
    /// `clientInfo.version`, cleaned and cut to 128 bytes.
    pub version: Option<String>,
    /// The `protocolVersion` the client asked for, if it is a valid
    /// `YYYY-MM-DD` version.
    pub protocol_version: Option<String>,
}

impl ClientInfo {
    /// Read the client info from `initialize` request params.
    pub fn from_initialize(params: &serde_json::Value) -> Self {
        let text = |v: &serde_json::Value| v.as_str().map(clip_name).filter(|s| !s.is_empty());
        Self {
            name: text(&params["clientInfo"]["name"]),
            version: text(&params["clientInfo"]["version"]),
            protocol_version: params["protocolVersion"]
                .as_str()
                .filter(|v| valid_protocol_version(v))
                .map(str::to_string),
        }
    }

    /// Record `mcp_k8s.client.name` and `mcp_k8s.client.version` on a request
    /// span.
    pub fn record(&self, span: &Span) {
        if let Some(name) = &self.name {
            span.record(semconv::MCP_K8S_CLIENT_NAME, name.as_str());
        }
        if let Some(version) = &self.version {
            span.record(semconv::MCP_K8S_CLIENT_VERSION, version.as_str());
        }
    }
}

/// Whether `v` looks like an MCP protocol version (`YYYY-MM-DD`). Use it
/// before recording a client-supplied `MCP-Protocol-Version` header.
pub fn valid_protocol_version(v: &str) -> bool {
    let b = v.as_bytes();
    b.len() == 10
        && b.iter().enumerate().all(|(i, c)| match i {
            4 | 7 => *c == b'-',
            _ => c.is_ascii_digit(),
        })
}

/// W3C trace context carried in a request's `params._meta`, as the MCP
/// conventions propagate it. Feed it to your OpenTelemetry propagator to get
/// the remote parent for [`server_span`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct W3cTraceContext {
    /// The `traceparent` value.
    pub traceparent: String,
    /// The `tracestate` value, if any.
    pub tracestate: Option<String>,
}

impl W3cTraceContext {
    /// Read `params._meta.traceparent` and `tracestate`. Returns `None`
    /// without a plausible `traceparent` (a string of at most 128 bytes);
    /// a `tracestate` over 512 bytes (the W3C limit) is dropped.
    pub fn from_meta(params: &serde_json::Value) -> Option<Self> {
        let meta = params.get("_meta")?;
        Self::from_parts(
            meta.get("traceparent")?.as_str()?,
            meta.get("tracestate").and_then(serde_json::Value::as_str),
        )
    }

    /// Build from raw `traceparent`/`tracestate` values (e.g. HTTP headers),
    /// with the same checks as [`Self::from_meta`].
    pub fn from_parts(traceparent: &str, tracestate: Option<&str>) -> Option<Self> {
        let traceparent = traceparent.trim();
        if traceparent.is_empty() || traceparent.len() > 128 {
            return None;
        }
        let tracestate = tracestate
            .map(str::trim)
            .filter(|s| !s.is_empty() && s.len() <= 512)
            .map(str::to_string);
        Some(Self {
            traceparent: traceparent.to_string(),
            tracestate,
        })
    }
}

/// Milliseconds with microsecond precision, for `duration_ms`.
pub(crate) fn millis(d: Duration) -> f64 {
    d.as_micros() as f64 / 1000.0
}

fn clip_name(s: &str) -> String {
    let mut out = String::with_capacity(s.len().min(MAX_NAME_LEN));
    for c in s.chars().filter(|c| !c.is_control()) {
        if out.len() + c.len_utf8() > MAX_NAME_LEN {
            break;
        }
        out.push(c);
    }
    out
}
