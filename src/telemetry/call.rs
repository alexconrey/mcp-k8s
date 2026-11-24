//! Tool-call instrumentation shared by [`crate::mcp::handle_tool`] and
//! embedders' own tools.

use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::Value;
use tracing::{Instrument, Span};

use super::semconv::{self, error_type};
use super::summary::{summarize, ActionClass, ToolCallSummary};
use super::{current_server_span, millis, server_span};

tokio::task_local! {
    /// The tool call whose body is running on this task, set by
    /// [`ToolCall::run`]. Nested instrumentation (an embedder's wrapper
    /// around [`crate::mcp::handle_tool`]) reports into it instead of starting
    /// a second call.
    static ACTIVE_CALL: Arc<CallState>;
}

struct CallState {
    tool: String,
    summary: ToolCallSummary,
    span: Span,
    started: Instant,
    denied_by: Mutex<Option<String>>,
}

impl CallState {
    fn deny(&self, rule: &str) {
        let mut denied = self.denied_by.lock().unwrap_or_else(|e| e.into_inner());
        if denied.is_none() {
            *denied = Some(super::summary::clean(rule));
        }
    }
}

/// Whether a [`ToolCall`] body is running on this task (see [`ToolCall::run`]).
pub fn tool_call_active() -> bool {
    ACTIVE_CALL.try_with(|_| ()).is_ok()
}

/// Mark the tool call running on this task as denied by `rule` (a permission
/// setting or policy rule, e.g. `global:create` or `hostPath`).
///
/// Call it where a guardrail refuses a call, inside [`ToolCall::run`] (which
/// [`crate::mcp::handle_tool`] uses). The call then finishes with
/// `mcp_k8s.decision=denied`, `mcp_k8s.denied_rule=<rule>` and
/// `error.type=permission_denied`. Returns `false`, and records nothing, when
/// no call is running. The first rule recorded wins.
pub fn record_denial(rule: &str) -> bool {
    ACTIVE_CALL.try_with(|call| call.deny(rule)).is_ok()
}

/// How a tool call ended, for [`ToolCall::finish_with`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallOutcome {
    /// The tool succeeded.
    Ok,
    /// The tool failed (`error.type=tool_error`).
    Error,
    /// A permission setting or policy refused the call
    /// (`error.type=permission_denied`). Prefer [`ToolCall::deny`] or
    /// [`record_denial`], which also record the rule.
    Denied,
}

/// Instrumentation for one `tools/call`: the span attributes, the
/// `mcp.server.operation.duration` metric event and the end-of-call log event.
///
/// [`crate::mcp::handle_tool`] uses it for every tool it serves, so embedders
/// only need it for tools they implement themselves:
///
/// ```no_run
/// # async fn my_tool(_args: &serde_json::Value) -> Result<String, String> { Ok(String::new()) }
/// # async fn example(args: serde_json::Value) {
/// use mcp_k8s::telemetry::ToolCall;
///
/// let call = ToolCall::start("create_application", &args);
/// let result = call.run(my_tool(&args)).await;
/// call.finish(&result);
/// # }
/// ```
///
/// - **Span.** Started inside a request span from [`super::server_span`]
///   (it must be the current span), the call records into that span. Started
///   anywhere else, it creates its own `tools/call <tool>` server span. Either
///   way a call has exactly one span.
/// - **Nesting.** Inside [`Self::run`], [`crate::mcp::handle_tool`] and any
///   other `ToolCall::start` see the running call and report into it (a
///   permission denial, say) instead of instrumenting themselves. An
///   embedder that wraps a tool which falls through to `handle_tool` still
///   gets one span, one metric point and one log event.
/// - **Cardinality.** The tool name goes on the metric, so only start a call
///   for a tool you actually serve, never for an arbitrary client-supplied
///   name.
/// - **Dropping** an unfinished call (the request was cancelled) finishes it
///   with `error.type=cancelled`.
#[must_use = "a ToolCall records nothing useful until finish() is called"]
pub struct ToolCall {
    state: Arc<CallState>,
    /// Set once the call has emitted (or must never emit: nested calls).
    finished: bool,
}

impl ToolCall {
    /// Start instrumenting a call of `tool` with `args`, classified by
    /// [`ActionClass::for_tool`].
    pub fn start(tool: &str, args: &Value) -> Self {
        Self::start_with_action(tool, args, ActionClass::for_tool(tool))
    }

    /// Like [`Self::start`], with an explicit action class (for tools whose
    /// names don't follow mcp-k8s's `list_`/`create_`/`delete_`… prefixes).
    ///
    /// Inside another call's [`Self::run`], this returns a handle to that
    /// outer call instead: [`Self::deny`] reports into it, and finishing
    /// the handle records nothing (the outer call does).
    pub fn start_with_action(tool: &str, args: &Value, action: ActionClass) -> Self {
        if let Ok(outer) = ACTIVE_CALL.try_with(Arc::clone) {
            return Self {
                state: outer,
                finished: true,
            };
        }
        let mut summary = summarize(tool, args);
        summary.action = action;
        let span =
            current_server_span().unwrap_or_else(|| server_span(semconv::TOOLS_CALL, Some(tool)));
        Self {
            state: Arc::new(CallState {
                tool: super::summary::clean(tool),
                summary,
                span,
                started: Instant::now(),
                denied_by: Mutex::new(None),
            }),
            finished: false,
        }
    }

    /// The span this call records into.
    pub fn span(&self) -> &Span {
        &self.state.span
    }

    /// The redacted summary of the call's arguments.
    pub fn summary(&self) -> &ToolCallSummary {
        &self.state.summary
    }

    /// Run the tool's body inside the call's span with the call active, so
    /// nested `handle_tool` calls and [`record_denial`] report into it.
    pub fn run<F: Future>(&self, body: F) -> impl Future<Output = F::Output> {
        let span = self.state.span.clone();
        ACTIVE_CALL.scope(self.state.clone(), body.instrument(span))
    }

    /// Mark the call as denied by `rule`. Same as [`record_denial`] inside
    /// [`Self::run`].
    pub fn deny(&self, rule: &str) {
        self.state.deny(rule);
    }

    /// Finish with the tool's result: `Ok` succeeds, `Err` is a `tool_error`
    /// (or a denial, if one was recorded). Records the outcome on the span
    /// and emits the metric and log events.
    pub fn finish<T, E>(self, result: &Result<T, E>) {
        let outcome = if result.is_ok() {
            CallOutcome::Ok
        } else {
            CallOutcome::Error
        };
        self.finish_with(outcome);
    }

    /// Finish with an explicit outcome.
    pub fn finish_with(mut self, outcome: CallOutcome) {
        if !self.finished {
            self.emit(Some(outcome));
        }
    }

    /// Finish without recording an outcome or emitting anything: the tool
    /// turned out not to be served here (`handle_tool` returning `None`), and
    /// the caller will report the request itself.
    pub fn abandon(mut self) {
        self.finished = true;
    }

    fn emit(&mut self, outcome: Option<CallOutcome>) {
        self.finished = true;
        let state = &self.state;
        let elapsed = state.started.elapsed();
        let denied_by = state
            .denied_by
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();

        let (outcome, error_type) = match (&denied_by, outcome) {
            (Some(_), _) | (None, Some(CallOutcome::Denied)) => {
                ("denied", Some(error_type::PERMISSION_DENIED))
            }
            (None, Some(CallOutcome::Ok)) => ("ok", None),
            (None, Some(CallOutcome::Error)) => ("error", Some(error_type::TOOL_ERROR)),
            (None, None) => ("cancelled", Some(error_type::CANCELLED)),
        };
        let decision = if outcome == "denied" {
            "denied"
        } else {
            "allowed"
        };
        let summary = &state.summary;
        let target = &summary.target;
        let action = summary.action.as_str();
        let span = &state.span;

        span.record(semconv::GEN_AI_TOOL_NAME, state.tool.as_str());
        span.record(semconv::GEN_AI_OPERATION_NAME, semconv::EXECUTE_TOOL);
        span.record(semconv::MCP_K8S_ACTION, action);
        span.record(semconv::MCP_K8S_DECISION, decision);
        span.record(semconv::MCP_K8S_OUTCOME, outcome);
        if let Some(rule) = &denied_by {
            span.record(semconv::MCP_K8S_DENIED_RULE, rule.as_str());
        }
        if let Some(et) = error_type {
            span.record(semconv::ERROR_TYPE, et);
            span.record(semconv::OTEL_STATUS_CODE, "error");
        }
        record_opt(span, semconv::K8S_CLUSTER_NAME, &target.cluster);
        record_opt(span, semconv::K8S_NAMESPACE_NAME, &target.namespace);
        record_opt(span, semconv::MCP_K8S_RESOURCE_KIND, &target.kind);
        record_opt(span, semconv::MCP_K8S_RESOURCE_NAME, &target.name);
        record_opt(
            span,
            semconv::MCP_K8S_RESOURCE_API_VERSION,
            &target.api_version,
        );
        if !summary.argument_keys.is_empty() {
            span.record(
                semconv::MCP_K8S_TOOL_ARGUMENT_KEYS,
                summary.argument_keys_string().as_str(),
            );
        }
        if let Some(arguments) = summary.arguments_json() {
            span.record(semconv::MCP_K8S_TOOL_ARGUMENTS, arguments.as_str());
        }

        span.in_scope(|| {
            emit_tool_metric(&state.tool, action, decision, error_type, elapsed);
            tracing::event!(
                target: semconv::TARGET,
                tracing::Level::INFO,
                {
                    { semconv::MCP_METHOD_NAME } = semconv::TOOLS_CALL,
                    { semconv::GEN_AI_TOOL_NAME } = state.tool.as_str(),
                    { semconv::MCP_K8S_OUTCOME } = outcome,
                    { semconv::DURATION_MS } = millis(elapsed),
                    { semconv::MCP_K8S_DECISION } = decision,
                    { semconv::MCP_K8S_DENIED_RULE } = denied_by.as_deref(),
                    { semconv::MCP_K8S_ACTION } = action,
                    { semconv::ERROR_TYPE } = error_type,
                    { semconv::K8S_NAMESPACE_NAME } = target.namespace.as_deref(),
                    { semconv::MCP_K8S_RESOURCE_KIND } = target.kind.as_deref(),
                    { semconv::MCP_K8S_RESOURCE_NAME } = target.name.as_deref(),
                },
                "tool call finished"
            );
        });
    }
}

impl Drop for ToolCall {
    fn drop(&mut self) {
        if !self.finished {
            self.emit(None);
        }
    }
}

/// Instrument one call of a tool you serve: [`ToolCall::start`], run `body`
/// inside it, and [`ToolCall::finish`] with its result.
pub async fn instrument_tool_call<F, T, E>(tool: &str, args: &Value, body: F) -> Result<T, E>
where
    F: Future<Output = Result<T, E>>,
{
    let call = ToolCall::start(tool, args);
    let result = call.run(body).await;
    call.finish(&result);
    result
}

fn record_opt(span: &Span, field: &str, value: &Option<String>) {
    if let Some(v) = value {
        span.record(field, v.as_str());
    }
}

/// The metric event for a tool call. Only low-cardinality fields: the
/// `MetricsLayer` turns every field into a metric attribute, and a message
/// would become one too, so there is none.
fn emit_tool_metric(
    tool: &str,
    action: &str,
    decision: &str,
    error_type: Option<&str>,
    elapsed: Duration,
) {
    tracing::event!(
        target: semconv::METRICS_TARGET,
        tracing::Level::INFO,
        {
            { semconv::HISTOGRAM_MCP_SERVER_OPERATION_DURATION } = elapsed.as_secs_f64(),
            { semconv::MCP_METHOD_NAME } = semconv::TOOLS_CALL,
            { semconv::GEN_AI_TOOL_NAME } = tool,
            { semconv::GEN_AI_OPERATION_NAME } = semconv::EXECUTE_TOOL,
            { semconv::MCP_K8S_ACTION } = action,
            { semconv::MCP_K8S_DECISION } = decision,
            { semconv::ERROR_TYPE } = error_type,
        }
    );
}
