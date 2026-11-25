//! Telemetry tests: `summarize` per action class, span/metric/log shape, the
//! one-call-one-span guarantee, and the canary test that proves no argument
//! value outside the allowlist reaches any span or event, for every tool.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use http::{Request, Response};
use kube::client::Body;
use serde_json::{json, Value};
use tracing::field::{Field, Visit};
use tracing::instrument::WithSubscriber;
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Instrument, Subscriber};
use tracing_subscriber::layer::{Context, Layer, SubscriberExt};
use tracing_subscriber::registry::LookupSpan;

use super::semconv;
use super::*;
use crate::permissions::ActionPermissions;
use crate::{ClusterManager, K8sClient};

// ---------------------------------------------------------------------------
// Capture layer
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
enum Kind {
    NewSpan,
    RecordSpan,
    Event,
}

#[derive(Clone, Debug)]
struct Captured {
    kind: Kind,
    /// Span name, or event message for events.
    name: String,
    target: String,
    span_id: Option<u64>,
    fields: BTreeMap<String, String>,
}

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<Captured>>>);

impl Capture {
    fn all(&self) -> Vec<Captured> {
        self.0.lock().unwrap().clone()
    }

    fn dispatch(&self) -> tracing::Dispatch {
        tracing::Dispatch::new(tracing_subscriber::registry().with(CaptureLayer(self.clone())))
    }

    fn new_spans(&self) -> Vec<Captured> {
        self.all()
            .into_iter()
            .filter(|c| c.kind == Kind::NewSpan && c.name == semconv::SERVER_SPAN_NAME)
            .collect()
    }

    fn events(&self, target: &str) -> Vec<Captured> {
        self.all()
            .into_iter()
            .filter(|c| c.kind == Kind::Event && c.target == target)
            .collect()
    }

    /// All fields recorded on span `id`, creation and later records merged.
    fn span_fields(&self, id: u64) -> BTreeMap<String, String> {
        let mut out = BTreeMap::new();
        for c in self.all() {
            if c.kind != Kind::Event && c.span_id == Some(id) {
                out.extend(c.fields);
            }
        }
        out
    }

    /// Every string telemetry produced: names, targets, field names and values.
    fn haystack(&self) -> String {
        let mut s = String::new();
        for c in self.all() {
            s.push_str(&c.name);
            s.push('\n');
            s.push_str(&c.target);
            s.push('\n');
            for (k, v) in &c.fields {
                s.push_str(k);
                s.push('=');
                s.push_str(v);
                s.push('\n');
            }
        }
        s
    }
}

/// Captures every mcp-k8s span and event at any level, and other crates'
/// spans and events at `INFO` and above (what the binary exports by default).
/// Dependencies' DEBUG output can echo request data: kube-client logs
/// `Unsuccessful: <API server message>` at DEBUG, and API servers echo names
/// and invalid values in their messages.
struct CaptureLayer(Capture);

fn captured(meta: &tracing::Metadata<'_>) -> bool {
    meta.target().starts_with("mcp_k8s") || *meta.level() <= tracing::Level::INFO
}

struct FieldVisitor<'a>(&'a mut BTreeMap<String, String>, &'a mut String);

impl Visit for FieldVisitor<'_> {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        let v = format!("{value:?}");
        if field.name() == "message" {
            *self.1 = v.clone();
        }
        self.0.insert(field.name().to_string(), v);
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            *self.1 = value.to_string();
        }
        self.0.insert(field.name().to_string(), value.to_string());
    }
}

impl<S> Layer<S> for CaptureLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, _ctx: Context<'_, S>) {
        if !captured(attrs.metadata()) {
            return;
        }
        let mut fields = BTreeMap::new();
        let mut msg = String::new();
        attrs.record(&mut FieldVisitor(&mut fields, &mut msg));
        self.0 .0.lock().unwrap().push(Captured {
            kind: Kind::NewSpan,
            name: attrs.metadata().name().to_string(),
            target: attrs.metadata().target().to_string(),
            span_id: Some(id.into_u64()),
            fields,
        });
    }

    fn on_record(&self, id: &Id, values: &Record<'_>, ctx: Context<'_, S>) {
        if !ctx.metadata(id).is_some_and(captured) {
            return;
        }
        let mut fields = BTreeMap::new();
        let mut msg = String::new();
        values.record(&mut FieldVisitor(&mut fields, &mut msg));
        self.0 .0.lock().unwrap().push(Captured {
            kind: Kind::RecordSpan,
            name: String::new(),
            target: String::new(),
            span_id: Some(id.into_u64()),
            fields,
        });
    }

    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        if !captured(event.metadata()) {
            return;
        }
        let mut fields = BTreeMap::new();
        let mut msg = String::new();
        event.record(&mut FieldVisitor(&mut fields, &mut msg));
        let span_id = ctx.event_span(event).map(|s| s.id().into_u64());
        self.0 .0.lock().unwrap().push(Captured {
            kind: Kind::Event,
            name: msg,
            target: event.metadata().target().to_string(),
            span_id,
            fields,
        });
    }
}

// ---------------------------------------------------------------------------
// Mock Kubernetes API: every request gets a 404 Status
// ---------------------------------------------------------------------------

/// Like a real API server, the error message echoes the request (URL and
/// body), so tool errors carry argument values. Telemetry must never record
/// error text, and the canary test proves it.
fn mock_client(perms: ActionPermissions) -> K8sClient {
    let (svc, mut handle) = tower_test::mock::pair::<Request<Body>, Response<Body>>();
    tokio::spawn(async move {
        while let Some((req, send)) = handle.next_request().await {
            let uri = req.uri().to_string();
            let body = req.into_body().collect_bytes().await.unwrap_or_default();
            let status = json!({
                "kind": "Status", "apiVersion": "v1", "metadata": {},
                "status": "Failure", "reason": "NotFound", "code": 404,
                "message": format!("not found: {uri} {}", String::from_utf8_lossy(&body)),
            });
            send.send_response(
                Response::builder()
                    .status(404)
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&status).unwrap()))
                    .unwrap(),
            );
        }
    });
    K8sClient::new(kube::Client::new(svc, "default"), vec![], perms)
}

fn field<'a>(fields: &'a BTreeMap<String, String>, name: &str) -> Option<&'a str> {
    fields.get(name).map(String::as_str)
}

// ---------------------------------------------------------------------------
// summarize
// ---------------------------------------------------------------------------

#[test]
fn action_classes() {
    assert_eq!(ActionClass::for_tool("get_pod"), ActionClass::Read);
    assert_eq!(ActionClass::for_tool("list_deployments"), ActionClass::Read);
    assert_eq!(ActionClass::for_tool("get_pod_logs"), ActionClass::Read);
    assert_eq!(
        ActionClass::for_tool("create_deployment"),
        ActionClass::Write
    );
    assert_eq!(ActionClass::for_tool("apply_manifest"), ActionClass::Write);
    assert_eq!(
        ActionClass::for_tool("scale_deployment"),
        ActionClass::Write
    );
    assert_eq!(ActionClass::for_tool("drain_node"), ActionClass::Write);
    assert_eq!(ActionClass::for_tool("delete_secret"), ActionClass::Delete);
    assert_eq!(ActionClass::for_tool("evict_pod"), ActionClass::Delete);
    assert_eq!(ActionClass::for_tool("exec_pod"), ActionClass::Exec);
    assert_eq!(ActionClass::Exec.as_str(), "exec");
    assert_eq!(ActionClass::Write.to_string(), "write");
}

#[test]
fn action_class_follows_the_permission_classification() {
    use crate::permissions::Action;
    // For every tool, the label agrees with what the permission check uses,
    // so a call labelled `read` can never be one that a disable flag gates.
    for tool in
        crate::mcp::tool_definitions(&ActionPermissions::default().with_apply_manifest(true))
    {
        let name = tool["name"].as_str().unwrap();
        let expected = match ActionPermissions::action_for_tool(name) {
            _ if name.starts_with("exec_") => ActionClass::Exec,
            Action::Read => ActionClass::Read,
            Action::Create | Action::Update => ActionClass::Write,
            Action::Delete => ActionClass::Delete,
        };
        assert_eq!(ActionClass::for_tool(name), expected, "{name}");
    }
    // Once exec_pod is gated as an update, disabling updates denies it with
    // the global rule (on older classifications it is a read and allowed).
    let no_update = ActionPermissions::new(false, true, false, vec![], true, true);
    if ActionPermissions::action_for_tool("exec_pod") == Action::Update {
        assert_eq!(
            no_update.denied_by("exec_pod").as_deref(),
            Some("global:update")
        );
    }
    let p = ActionPermissions::new(false, false, false, vec!["pod-update".into()], true, true);
    assert_eq!(
        p.action_denied_by("pod", &Action::Update).as_deref(),
        Some("override:pod-update")
    );
    assert_eq!(p.action_denied_by("pod", &Action::Create), None);
    assert_eq!(
        no_update
            .action_denied_by("deployment", &Action::Update)
            .as_deref(),
        Some("global:update")
    );
}

#[tokio::test]
async fn guardrail_denial_inside_a_tool_is_recorded() {
    // A workload guardrail deep inside a tool body (as the policy engine
    // does) reports the rule through record_denial; the call ends denied.
    let cap = Capture::default();
    let dispatch = cap.dispatch();
    let span = tracing::dispatcher::with_default(&dispatch, || {
        server_span("tools/call", Some("create_pod"))
    });
    async {
        let call = ToolCall::start("create_pod", &json!({"namespace": "prod", "name": "p"}));
        let result: Result<String, String> = call
            .run(async {
                // e.g. policy::into_result() rejecting a privileged container
                assert!(record_denial("policy:allow_privileged"));
                Err("rejected by the mcp-k8s workload policy".to_string())
            })
            .await;
        call.finish(&result);
    }
    .instrument(span)
    .with_subscriber(dispatch)
    .await;

    let spans = cap.new_spans();
    assert_eq!(spans.len(), 1);
    let f = cap.span_fields(spans[0].span_id.unwrap());
    assert_eq!(field(&f, semconv::MCP_K8S_DECISION), Some("denied"));
    assert_eq!(
        field(&f, semconv::MCP_K8S_DENIED_RULE),
        Some("policy:allow_privileged")
    );
    assert_eq!(field(&f, semconv::ERROR_TYPE), Some("permission_denied"));
    let metrics = cap.events(semconv::METRICS_TARGET);
    assert_eq!(metrics.len(), 1);
    assert_eq!(
        field(&metrics[0].fields, semconv::MCP_K8S_DECISION),
        Some("denied")
    );
}

#[test]
fn summarize_read() {
    let s = summarize(
        "get_pod_logs",
        &json!({"namespace": "prod", "pod_name": "web-1", "container": "app", "tail_lines": 50}),
    );
    assert_eq!(s.action, ActionClass::Read);
    assert_eq!(s.target.namespace.as_deref(), Some("prod"));
    assert_eq!(s.target.name.as_deref(), Some("web-1"));
    assert_eq!(
        s.argument_keys,
        ["container", "namespace", "pod_name", "tail_lines"]
    );
    assert_eq!(
        s.arguments_json().unwrap(),
        r#"{"container":"app","namespace":"prod","pod_name":"web-1"}"#
    );
}

#[test]
fn summarize_write_drops_env_values() {
    let s = summarize(
        "create_deployment",
        &json!({
            "namespace": "prod", "name": "web", "image": "nginx:1.27", "replicas": 3,
            "env": {"DB_PASSWORD": "hunter2"},
            "env_from": [{"secret_ref": "db-creds"}]
        }),
    );
    assert_eq!(s.action, ActionClass::Write);
    assert_eq!(
        s.argument_keys,
        ["env", "env_from", "image", "name", "namespace", "replicas"]
    );
    let json = s.arguments_json().unwrap();
    assert_eq!(
        json,
        r#"{"image":"nginx:1.27","name":"web","namespace":"prod","replicas":3}"#
    );
    assert!(!json.contains("hunter2") && !json.contains("DB_PASSWORD"));
    assert!(!json.contains("db-creds"));
}

#[test]
fn summarize_delete() {
    let s = summarize(
        "delete_secret",
        &json!({"namespace": "prod", "name": "tls"}),
    );
    assert_eq!(s.action, ActionClass::Delete);
    assert_eq!(s.target.name.as_deref(), Some("tls"));
    assert_eq!(s.target.namespace.as_deref(), Some("prod"));
}

#[test]
fn summarize_exec_drops_command() {
    let s = summarize(
        "exec_pod",
        &json!({"namespace": "prod", "name": "web-1", "container": "app",
                "command": ["sh", "-c", "cat /run/secrets/token"]}),
    );
    assert_eq!(s.action, ActionClass::Exec);
    assert_eq!(
        s.argument_keys,
        ["command", "container", "name", "namespace"]
    );
    let json = s.arguments_json().unwrap();
    assert!(!json.contains("token") && !json.contains("sh"), "{json}");
}

#[test]
fn summarize_apply_manifest_reads_only_the_target() {
    let manifest = json!({
        "apiVersion": "v1", "kind": "Secret",
        "metadata": {"name": "db", "namespace": "prod", "labels": {"team": "payments"}},
        "stringData": {"password": "hunter2"}
    });
    let s = summarize("apply_manifest", &json!({"manifest": manifest.to_string()}));
    assert_eq!(s.action, ActionClass::Write);
    assert_eq!(
        s.target,
        Target {
            cluster: None,
            namespace: Some("prod".into()),
            kind: Some("Secret".into()),
            name: Some("db".into()),
            api_version: Some("v1".into()),
        }
    );
    assert_eq!(s.argument_keys, ["manifest"]);
    assert_eq!(s.arguments_json(), None);
    let debug = format!("{s:?}");
    assert!(!debug.contains("hunter2") && !debug.contains("payments"));
    // Not JSON: no target, no failure.
    let s = summarize("apply_manifest", &json!({"manifest": "kind: Secret"}));
    assert_eq!(s.target, Target::default());
}

#[test]
fn summarize_custom_resource_prefers_explicit_arguments() {
    let s = summarize(
        "create_custom_resource",
        &json!({
            "group": "stable.example.com", "version": "v1", "kind": "CronTab",
            "namespace": "prod",
            "manifest": {"apiVersion": "other/v9", "kind": "Other",
                         "metadata": {"name": "nightly", "namespace": "elsewhere"},
                         "spec": {"cronSpec": "* * * * */5"}}
        }),
    );
    assert_eq!(s.target.kind.as_deref(), Some("CronTab"));
    assert_eq!(
        s.target.api_version.as_deref(),
        Some("stable.example.com/v1")
    );
    assert_eq!(s.target.namespace.as_deref(), Some("prod"));
    assert_eq!(s.target.name.as_deref(), Some("nightly"));
}

#[test]
fn summarize_keys_and_values_are_bounded() {
    // A key the tool's schema doesn't declare is counted, never named.
    let s = summarize(
        "get_pod",
        &json!({"namespace": "a", "name": "b", "AKIAEXAMPLE": 1}),
    );
    assert_eq!(s.argument_keys, ["name", "namespace", "?"]);
    // Tools mcp-k8s doesn't know keep identifier-like keys only.
    let s = summarize("trigger_build", &json!({"app": 1, "not a key!": 2}));
    assert_eq!(s.argument_keys, ["app", "?"]);
    // Values are cut and control characters dropped.
    let s = summarize(
        "get_pod",
        &json!({"name": format!("a\nb{}", "x".repeat(1000))}),
    );
    let name = s.target.name.unwrap();
    assert_eq!(name.len(), summary::MAX_VALUE_LEN);
    assert!(name.starts_with("abx"));
    // Non-scalar values of allowlisted keys are dropped.
    let s = summarize("get_pod", &json!({"name": {"nested": "x"}}));
    assert_eq!(s.arguments_json(), None);
    // Non-object arguments.
    let s = summarize("get_pod", &json!("nonsense"));
    assert!(s.argument_keys.is_empty());
    // decode is allowlisted (a boolean).
    let s = summarize(
        "get_secret",
        &json!({"namespace": "a", "name": "b", "decode": true}),
    );
    assert_eq!(s.arguments["decode"], json!(true));
}

#[test]
fn allowlist_is_reviewed() {
    // Widening this list widens what telemetry records. Each entry needs a
    // reason in ALLOWED_ARGUMENTS and a review; change this snapshot with it.
    let keys: Vec<&str> = ALLOWED_ARGUMENTS.iter().map(|(k, _)| *k).collect();
    assert_eq!(
        keys,
        [
            "namespace",
            "name",
            "kind",
            "api_version",
            "group",
            "version",
            "replicas",
            "image",
            "container",
            "pod_name",
            "job_name",
            "cluster",
            "decode",
        ]
    );
}

#[test]
fn known_tools() {
    assert!(is_known_tool("apply_manifest"));
    assert!(is_known_tool("list_clusters"));
    assert!(!is_known_tool("create_application"));
    assert!(!is_known_tool(""));
}

#[test]
fn semconv_consistency() {
    assert_eq!(
        semconv::HISTOGRAM_MCP_SERVER_OPERATION_DURATION,
        format!("histogram.{}", semconv::MCP_SERVER_OPERATION_DURATION)
    );
    let attrs: BTreeSet<_> = semconv::METRIC_ATTRIBUTES.iter().collect();
    assert_eq!(attrs.len(), semconv::METRIC_ATTRIBUTES.len());
    assert!(semconv::MCP_SERVER_OPERATION_DURATION_BUCKETS
        .windows(2)
        .all(|w| w[0] < w[1]));
    assert!(semconv::METRICS_TARGET.starts_with(semconv::TARGET));
}

#[test]
fn denied_by_matches_is_tool_allowed() {
    let configs = [
        ActionPermissions::default(),
        ActionPermissions::new(true, true, true, vec![], true, true),
        ActionPermissions::new(true, false, false, vec![], true, false),
        ActionPermissions::new(
            false,
            false,
            false,
            vec!["deployment-delete".into(), "pod-create".into()],
            true,
            false,
        ),
    ];
    let tools =
        crate::mcp::tool_definitions(&ActionPermissions::default().with_apply_manifest(true));
    for perms in &configs {
        for tool in &tools {
            let name = tool["name"].as_str().unwrap();
            assert_eq!(
                perms.denied_by(name).is_some(),
                !perms.is_tool_allowed(name),
                "{name} {perms:?}"
            );
        }
    }
    let p = &configs[3];
    assert_eq!(
        p.denied_by("delete_deployment").as_deref(),
        Some("override:deployment-delete")
    );
    assert_eq!(
        p.denied_by("apply_manifest").as_deref(),
        Some("apply_manifest:disabled")
    );
    assert_eq!(
        configs[1].denied_by("create_pod").as_deref(),
        Some("global:create")
    );
    assert_eq!(configs[0].denied_by("delete_pod"), None);
}

// ---------------------------------------------------------------------------
// Request span, client info, trace context
// ---------------------------------------------------------------------------

#[test]
fn server_span_names() {
    let cap = Capture::default();
    tracing::dispatcher::with_default(&cap.dispatch(), || {
        let _a = server_span("tools/call", Some("get_pod"));
        let _b = server_span("tools/list", None);
        let _c = server_span("evil/method\n", Some("x"));
        let _d = server_span("prompts/get", Some("diagnose-pod"));
    });
    let spans = cap.new_spans();
    let names: Vec<_> = spans
        .iter()
        .map(|s| field(&s.fields, semconv::OTEL_NAME).unwrap().to_string())
        .collect();
    assert_eq!(
        names,
        [
            "tools/call get_pod",
            "tools/list",
            "_OTHER x",
            "prompts/get diagnose-pod"
        ]
    );
    for s in &spans {
        assert_eq!(field(&s.fields, semconv::OTEL_KIND), Some("server"));
        assert_eq!(s.target, semconv::TARGET);
    }
    assert_eq!(
        field(&spans[2].fields, semconv::MCP_METHOD_NAME),
        Some("evil/method")
    );
    // Tool attributes are ToolCall's to record, once.
    let first = cap.span_fields(spans[0].span_id.unwrap());
    assert_eq!(field(&first, semconv::GEN_AI_TOOL_NAME), None);
    let last = cap.span_fields(spans[3].span_id.unwrap());
    assert_eq!(
        field(&last, semconv::GEN_AI_PROMPT_NAME),
        Some("diagnose-pod")
    );
}

#[test]
fn client_info_and_trace_context() {
    let params = json!({
        "protocolVersion": "2025-11-25",
        "clientInfo": {"name": "claude-code", "version": "2.1.0"},
        "_meta": {"traceparent": "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
                  "tracestate": "rojo=00f067aa0ba902b7"}
    });
    let info = ClientInfo::from_initialize(&params);
    assert_eq!(info.name.as_deref(), Some("claude-code"));
    assert_eq!(info.version.as_deref(), Some("2.1.0"));
    assert_eq!(info.protocol_version.as_deref(), Some("2025-11-25"));
    let bad = ClientInfo::from_initialize(&json!({"protocolVersion": "latest; drop"}));
    assert_eq!(bad, ClientInfo::default());

    let cx = W3cTraceContext::from_meta(&params).unwrap();
    assert!(cx.traceparent.starts_with("00-4bf92f"));
    assert_eq!(cx.tracestate.as_deref(), Some("rojo=00f067aa0ba902b7"));
    assert_eq!(W3cTraceContext::from_meta(&json!({})), None);
    assert_eq!(
        W3cTraceContext::from_meta(&json!({"_meta": {"traceparent": 7}})),
        None
    );

    assert!(valid_protocol_version("2025-06-18"));
    assert!(!valid_protocol_version("2025-6-18"));
    assert!(!valid_protocol_version("2025-06-18x"));
}

// ---------------------------------------------------------------------------
// handle_tool instrumentation
// ---------------------------------------------------------------------------

/// The metric event may only carry the histogram field and the allowed
/// low-cardinality attributes.
fn assert_low_cardinality(event: &Captured) {
    for key in event.fields.keys() {
        assert!(
            key == semconv::HISTOGRAM_MCP_SERVER_OPERATION_DURATION
                || semconv::METRIC_ATTRIBUTES.contains(&key.as_str()),
            "metric event carries `{key}`: {event:?}"
        );
    }
    assert!(event.name.is_empty(), "metric events carry no message");
}

#[tokio::test]
async fn handle_tool_records_into_the_request_span() {
    let cap = Capture::default();
    let dispatch = cap.dispatch();
    let client = mock_client(ActionPermissions::default());
    let span =
        tracing::dispatcher::with_default(&dispatch, || server_span("tools/call", Some("get_pod")));
    let result = crate::mcp::handle_tool(
        &client,
        "get_pod",
        &json!({"namespace": "prod", "name": "web-1"}),
    )
    .instrument(span)
    .with_subscriber(dispatch)
    .await;
    assert!(matches!(result, Some(Err(_))), "mock answers 404");

    let spans = cap.new_spans();
    assert_eq!(spans.len(), 1, "one request span, no second tool span");
    let f = cap.span_fields(spans[0].span_id.unwrap());
    assert_eq!(field(&f, semconv::GEN_AI_TOOL_NAME), Some("get_pod"));
    assert_eq!(field(&f, semconv::MCP_K8S_ACTION), Some("read"));
    assert_eq!(field(&f, semconv::MCP_K8S_DECISION), Some("allowed"));
    assert_eq!(field(&f, semconv::MCP_K8S_OUTCOME), Some("error"));
    assert_eq!(field(&f, semconv::ERROR_TYPE), Some("tool_error"));
    assert_eq!(field(&f, semconv::OTEL_STATUS_CODE), Some("error"));
    assert_eq!(field(&f, semconv::K8S_NAMESPACE_NAME), Some("prod"));
    assert_eq!(field(&f, semconv::MCP_K8S_RESOURCE_NAME), Some("web-1"));
    assert_eq!(
        field(&f, semconv::MCP_K8S_TOOL_ARGUMENT_KEYS),
        Some("name,namespace")
    );

    let metrics = cap.events(semconv::METRICS_TARGET);
    assert_eq!(metrics.len(), 1);
    assert_low_cardinality(&metrics[0]);
    assert_eq!(metrics[0].span_id, spans[0].span_id);
    assert_eq!(
        field(&metrics[0].fields, semconv::GEN_AI_TOOL_NAME),
        Some("get_pod")
    );
    assert_eq!(
        field(&metrics[0].fields, semconv::ERROR_TYPE),
        Some("tool_error")
    );

    let logs = cap.events(semconv::TARGET);
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].name, "tool call finished");
    assert_eq!(
        field(&logs[0].fields, semconv::MCP_K8S_OUTCOME),
        Some("error")
    );
    assert_eq!(
        field(&logs[0].fields, semconv::MCP_K8S_DECISION),
        Some("allowed")
    );
    assert!(logs[0].fields.contains_key(semconv::DURATION_MS));
}

async fn audit_events_for(perms: ActionPermissions, tool: &str, args: Value) -> Vec<Captured> {
    let cap = Capture::default();
    let dispatch = cap.dispatch();
    let client = mock_client(perms);
    let span =
        tracing::dispatcher::with_default(&dispatch, || server_span("tools/call", Some(tool)));
    let _ = crate::mcp::handle_tool(&client, tool, &args)
        .instrument(span)
        .with_subscriber(dispatch)
        .await;
    cap.events(semconv::AUDIT_TARGET)
}

#[tokio::test]
async fn writes_and_denials_are_audited_reads_are_not() {
    // A write that reached the API (the mock answers 404): audited.
    let events = audit_events_for(
        ActionPermissions::default(),
        "delete_pod",
        json!({"namespace": "prod", "name": "web-1"}),
    )
    .await;
    assert_eq!(events.len(), 1, "{events:?}");
    let f = &events[0].fields;
    assert_eq!(events[0].name, "audit");
    assert_eq!(field(f, semconv::GEN_AI_TOOL_NAME), Some("delete_pod"));
    assert_eq!(field(f, semconv::MCP_K8S_ACTION), Some("delete"));
    assert_eq!(field(f, semconv::MCP_K8S_DECISION), Some("allowed"));
    assert_eq!(field(f, semconv::K8S_NAMESPACE_NAME), Some("prod"));
    assert_eq!(field(f, semconv::MCP_K8S_RESOURCE_NAME), Some("web-1"));
    assert_eq!(
        field(f, semconv::MCP_K8S_TOOL_ARGUMENT_KEYS),
        Some("name,namespace")
    );

    // A denied write: audited with the rule.
    let events = audit_events_for(
        ActionPermissions::new(false, false, true, vec![], false, false),
        "delete_pod",
        json!({"namespace": "prod", "name": "web-1"}),
    )
    .await;
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(
        field(&events[0].fields, semconv::MCP_K8S_DECISION),
        Some("denied")
    );
    assert_eq!(
        field(&events[0].fields, semconv::MCP_K8S_DENIED_RULE),
        Some("global:delete")
    );

    // A read: not audited.
    let events = audit_events_for(
        ActionPermissions::default(),
        "get_pod",
        json!({"namespace": "prod", "name": "web-1"}),
    )
    .await;
    assert!(events.is_empty(), "{events:?}");
}

#[tokio::test]
async fn handle_tool_without_a_request_span_creates_one() {
    let cap = Capture::default();
    let client = mock_client(ActionPermissions::default());
    crate::mcp::handle_tool(&client, "list_pods", &json!({"namespace": "prod"}))
        .with_subscriber(cap.dispatch())
        .await;
    let spans = cap.new_spans();
    assert_eq!(spans.len(), 1);
    assert_eq!(
        field(&spans[0].fields, semconv::OTEL_NAME),
        Some("tools/call list_pods")
    );
    assert_eq!(field(&spans[0].fields, semconv::OTEL_KIND), Some("server"));
    assert_eq!(cap.events(semconv::METRICS_TARGET).len(), 1);
    assert_eq!(cap.events(semconv::TARGET).len(), 1);
}

#[tokio::test]
async fn namespace_allowlist_refusals_are_recorded_as_denials() {
    let cap = Capture::default();
    let (svc, _handle) = tower_test::mock::pair::<Request<Body>, Response<Body>>();
    let client = K8sClient::new(
        kube::Client::new(svc, "default"),
        vec!["team-a".to_string()],
        ActionPermissions::default(),
    );
    let result = crate::mcp::handle_tool(
        &client,
        "get_pod",
        &json!({"namespace": "kube-system", "name": "etcd"}),
    )
    .with_subscriber(cap.dispatch())
    .await;
    assert!(matches!(result, Some(Err(_))));
    let spans = cap.new_spans();
    assert_eq!(spans.len(), 1);
    let f = cap.span_fields(spans[0].span_id.unwrap());
    assert_eq!(field(&f, semconv::MCP_K8S_DECISION), Some("denied"));
    assert_eq!(
        field(&f, semconv::MCP_K8S_DENIED_RULE),
        Some(crate::client::NAMESPACE_ALLOWLIST_RULE)
    );
    assert_eq!(field(&f, semconv::ERROR_TYPE), Some("permission_denied"));
    // Reads that are refused are audited too.
    let audit = cap.events(semconv::AUDIT_TARGET);
    assert_eq!(audit.len(), 1, "{audit:?}");
}

#[tokio::test]
async fn denied_call_records_decision_and_rule() {
    let cap = Capture::default();
    let client = mock_client(ActionPermissions::new(
        true,
        false,
        false,
        vec![],
        true,
        true,
    ));
    let result = crate::mcp::handle_tool(
        &client,
        "create_deployment",
        &json!({"namespace": "prod", "name": "web", "image": "nginx"}),
    )
    .with_subscriber(cap.dispatch())
    .await;
    assert!(matches!(result, Some(Err(_))));
    let spans = cap.new_spans();
    assert_eq!(spans.len(), 1);
    let f = cap.span_fields(spans[0].span_id.unwrap());
    assert_eq!(field(&f, semconv::MCP_K8S_DECISION), Some("denied"));
    assert_eq!(
        field(&f, semconv::MCP_K8S_DENIED_RULE),
        Some("global:create")
    );
    assert_eq!(field(&f, semconv::ERROR_TYPE), Some("permission_denied"));
    assert_eq!(field(&f, semconv::MCP_K8S_OUTCOME), Some("denied"));
    let metrics = cap.events(semconv::METRICS_TARGET);
    assert_eq!(metrics.len(), 1);
    assert_eq!(
        field(&metrics[0].fields, semconv::MCP_K8S_DECISION),
        Some("denied")
    );
    assert_eq!(
        field(&metrics[0].fields, semconv::ERROR_TYPE),
        Some("permission_denied")
    );
    assert!(!metrics[0].fields.contains_key(semconv::MCP_K8S_DENIED_RULE));
}

#[tokio::test]
async fn unknown_tool_emits_nothing() {
    let cap = Capture::default();
    let client = mock_client(ActionPermissions::default());
    let result = crate::mcp::handle_tool(&client, "not_a_tool", &json!({}))
        .with_subscriber(cap.dispatch())
        .await;
    assert!(result.is_none());
    assert!(cap.all().is_empty(), "{:?}", cap.all());
}

#[tokio::test]
async fn wrapping_handle_tool_is_still_one_call() {
    // An embedder's own tool that falls through to handle_tool (as deckwatch's
    // create_deployment interceptor does), with a denial inside.
    let cap = Capture::default();
    let client = mock_client(ActionPermissions::new(
        true,
        false,
        false,
        vec![],
        true,
        true,
    ));
    let args = json!({"namespace": "prod", "name": "web", "image": "nginx"});
    let dispatch = cap.dispatch();
    let span = tracing::dispatcher::with_default(&dispatch, || {
        server_span("tools/call", Some("create_deployment"))
    });
    async {
        let call = ToolCall::start("create_deployment", &args);
        let result = call
            .run(async {
                assert!(tool_call_active());
                crate::mcp::handle_tool(&client, "create_deployment", &args)
                    .await
                    .unwrap()
            })
            .await;
        call.finish(&result);
    }
    .instrument(span)
    .with_subscriber(dispatch)
    .await;

    assert_eq!(cap.new_spans().len(), 1);
    assert_eq!(cap.events(semconv::METRICS_TARGET).len(), 1);
    let logs = cap.events(semconv::TARGET);
    assert_eq!(logs.len(), 1);
    assert_eq!(
        field(&logs[0].fields, semconv::MCP_K8S_DECISION),
        Some("denied")
    );
    assert_eq!(
        field(&logs[0].fields, semconv::MCP_K8S_DENIED_RULE),
        Some("global:create")
    );
    assert_eq!(
        field(&logs[0].fields, semconv::GEN_AI_TOOL_NAME),
        Some("create_deployment")
    );
}

#[tokio::test]
async fn nested_tool_call_reports_into_the_outer_one() {
    let cap = Capture::default();
    async {
        let outer = ToolCall::start("restart_application", &json!({"namespace": "prod"}));
        outer
            .run(async {
                // e.g. an own tool that wraps another own tool
                let inner = ToolCall::start("restart_deployment", &json!({"name": "web"}));
                inner.deny("policy:freeze");
                inner.finish::<(), ()>(&Err(()));
            })
            .await;
        outer.finish::<(), ()>(&Ok(()));
    }
    .with_subscriber(cap.dispatch())
    .await;
    assert_eq!(cap.new_spans().len(), 1);
    assert_eq!(cap.events(semconv::METRICS_TARGET).len(), 1);
    let logs = cap.events(semconv::TARGET);
    assert_eq!(logs.len(), 1);
    assert_eq!(
        field(&logs[0].fields, semconv::GEN_AI_TOOL_NAME),
        Some("restart_application")
    );
    assert_eq!(
        field(&logs[0].fields, semconv::MCP_K8S_DENIED_RULE),
        Some("policy:freeze")
    );
}

#[tokio::test]
async fn own_tool_helpers() {
    let cap = Capture::default();
    async {
        let ok: Result<&str, String> =
            instrument_tool_call("list_templates", &json!({}), async { Ok("[]") }).await;
        assert!(ok.is_ok());
        let call = ToolCall::start_with_action(
            "trigger_build",
            &json!({"namespace": "prod", "name": "web"}),
            ActionClass::Write,
        );
        assert!(!record_denial("outside run: ignored"));
        call.deny("gitops:disabled");
        call.finish::<(), ()>(&Err(()));
        // Dropped before finishing: cancelled.
        let _dropped = ToolCall::start("watch_build", &json!({}));
    }
    .with_subscriber(cap.dispatch())
    .await;

    let logs = cap.events(semconv::TARGET);
    let outcomes: Vec<_> = logs
        .iter()
        .map(|l| field(&l.fields, semconv::MCP_K8S_OUTCOME).unwrap())
        .collect();
    assert_eq!(outcomes, ["ok", "denied", "cancelled"]);
    assert_eq!(
        field(&logs[1].fields, semconv::MCP_K8S_ACTION),
        Some("write")
    );
    assert_eq!(
        field(&logs[1].fields, semconv::MCP_K8S_DENIED_RULE),
        Some("gitops:disabled")
    );
    assert_eq!(
        field(&logs[2].fields, semconv::ERROR_TYPE),
        Some("cancelled")
    );
    assert_eq!(cap.new_spans().len(), 3);
}

#[test]
fn finish_operation_events() {
    let cap = Capture::default();
    tracing::dispatcher::with_default(&cap.dispatch(), || {
        let span = server_span("tools/list", None);
        span.in_scope(|| finish_operation("tools/list", Duration::from_millis(5), None));
        let span = server_span("x/y", None);
        span.in_scope(|| {
            record_rpc_status(&span, -32601);
            finish_operation("x/y", Duration::from_millis(1), None)
        });
        let span = server_span("resources/read", None);
        span.in_scope(|| {
            record_rpc_status(&span, -32000);
            finish_operation("resources/read", Duration::from_millis(1), Some("-32000"))
        });
    });
    let metrics = cap.events(semconv::METRICS_TARGET);
    assert_eq!(metrics.len(), 3);
    metrics.iter().for_each(assert_low_cardinality);
    assert_eq!(
        field(&metrics[1].fields, semconv::MCP_METHOD_NAME),
        Some("_OTHER")
    );
    assert_eq!(
        field(&metrics[2].fields, semconv::ERROR_TYPE),
        Some("-32000")
    );
    let spans = cap.new_spans();
    let unknown = cap.span_fields(spans[1].span_id.unwrap());
    assert_eq!(field(&unknown, semconv::RPC_STATUS_CODE), Some("-32601"));
    assert_eq!(
        field(&unknown, semconv::OTEL_STATUS_CODE),
        None,
        "caller error"
    );
    let failed = cap.span_fields(spans[2].span_id.unwrap());
    assert_eq!(field(&failed, semconv::OTEL_STATUS_CODE), Some("error"));
    assert_eq!(field(&failed, semconv::ERROR_TYPE), Some("-32000"));
    let logs = cap.events(semconv::TARGET);
    assert!(logs.iter().all(|l| l.name == "request finished"));
}

// ---------------------------------------------------------------------------
// Canary test
// ---------------------------------------------------------------------------

/// Unique, non-overlapping markers: fixed width plus a terminator, so no
/// canary is a substring of another.
#[derive(Default)]
struct Canaries {
    by_marker: Vec<(String, String)>,
}

impl Canaries {
    fn value(&mut self, path: &str) -> String {
        let m = format!("canary{:05}x", self.by_marker.len());
        self.by_marker.push((m.clone(), path.to_string()));
        m
    }

    fn key(&mut self, path: &str) -> String {
        let m = format!("canarykey{:05}x", self.by_marker.len());
        self.by_marker
            .push((m.clone(), format!("{path} (map key)")));
        m
    }

    /// A manifest-like object with a canary in every string, for free-form
    /// objects (`manifest`, `patch`, `pod_selector`, policy rules).
    fn object(&mut self, p: &str) -> Value {
        json!({
            "apiVersion": self.value(&format!("{p}.apiVersion")),
            "kind": self.value(&format!("{p}.kind")),
            "metadata": {
                "name": self.value(&format!("{p}.metadata.name")),
                "namespace": self.value(&format!("{p}.metadata.namespace")),
                "labels": { self.key(&format!("{p}.metadata.labels")): self.value(&format!("{p}.metadata.labels.*")) },
                "annotations": { self.key(&format!("{p}.metadata.annotations")): self.value(&format!("{p}.metadata.annotations.*")) },
            },
            "spec": {
                "containers": [{
                    "name": self.value(&format!("{p}.spec.containers[].name")),
                    "image": self.value(&format!("{p}.spec.containers[].image")),
                    "command": [self.value(&format!("{p}.spec.containers[].command[]"))],
                    "env": [{"name": self.value(&format!("{p}.spec.containers[].env[].name")),
                             "value": self.value(&format!("{p}.spec.containers[].env[].value"))}],
                }],
            },
            "data": { self.key(&format!("{p}.data")): self.value(&format!("{p}.data.*")) },
            "stringData": { self.key(&format!("{p}.stringData")): self.value(&format!("{p}.stringData.*")) },
        })
    }

    /// Arguments for a JSON schema, every string a canary unique to its path.
    fn fill(&mut self, schema: &Value, path: &str) -> Value {
        if let Some(alt) = schema
            .get("oneOf")
            .or_else(|| schema.get("anyOf"))
            .and_then(|a| a.get(0))
        {
            return self.fill(alt, path);
        }
        let ty = schema.get("type").and_then(Value::as_str);
        match ty {
            Some("string") if path == "manifest" => {
                // apply_manifest takes a JSON string: canaries inside it.
                Value::String(self.object(path).to_string())
            }
            Some("string") => Value::String(self.value(path)),
            Some("integer") => json!(3),
            Some("number") => json!(1.5),
            Some("boolean") => json!(true),
            Some("array") => {
                let items = schema.get("items").cloned().unwrap_or(json!({}));
                json!([self.fill(&items, &format!("{path}[]"))])
            }
            Some("object") | None if schema.get("properties").is_some() => {
                let mut out = serde_json::Map::new();
                for (k, v) in schema["properties"].as_object().unwrap() {
                    let p = if path.is_empty() {
                        k.clone()
                    } else {
                        format!("{path}.{k}")
                    };
                    out.insert(k.clone(), self.fill(v, &p));
                }
                if let Some(ap) = schema.get("additionalProperties").filter(|a| a.is_object()) {
                    out.insert(self.key(path), self.fill(ap, &format!("{path}.*")));
                }
                Value::Object(out)
            }
            Some("object") => match schema.get("additionalProperties") {
                Some(ap) if ap.is_object() => {
                    json!({ self.key(path): self.fill(ap, &format!("{path}.*")) })
                }
                _ => self.object(path),
            },
            // Untyped (int-or-string, free-form array items).
            _ => {
                if path.ends_with("[]") {
                    self.object(path)
                } else {
                    Value::String(self.value(path))
                }
            }
        }
    }
}

/// Run a tool the way the server does: cluster tools go to the manager inside
/// a `ToolCall`, everything else through `handle_tool`.
async fn call_like_the_server(
    manager: &ClusterManager,
    client: &K8sClient,
    tool: &str,
    args: &Value,
) -> Option<Result<String, String>> {
    match tool {
        "list_clusters" | "switch_cluster" | "get_active_cluster" => {
            let call = ToolCall::start(tool, args);
            let result = call
                .run(crate::resources::cluster_mgmt::handle_tool(
                    manager, tool, args,
                ))
                .await;
            match &result {
                Some(r) => call.finish(r),
                None => call.abandon(),
            }
            result
        }
        _ => crate::mcp::handle_tool(client, tool, args).await,
    }
}

#[tokio::test]
async fn canary_no_argument_value_reaches_telemetry() {
    let tools =
        crate::mcp::tool_definitions(&ActionPermissions::default().with_apply_manifest(true));
    assert!(
        tools.len() > 200,
        "expected every tool, got {}",
        tools.len()
    );
    let allowed: BTreeSet<&str> = ALLOWED_ARGUMENTS.iter().map(|(k, _)| *k).collect();
    // Manifest fields that may become the target (explicit arguments win).
    let manifest_target = [
        "manifest.kind",
        "manifest.apiVersion",
        "manifest.metadata.name",
        "manifest.metadata.namespace",
    ];

    let mut failures = Vec::new();
    for tool in &tools {
        let name = tool["name"].as_str().unwrap();
        let mut canaries = Canaries::default();
        let args = canaries.fill(&tool["inputSchema"], "");

        let client = mock_client(ActionPermissions::default());
        let manager = ClusterManager::new(client.clone(), "default".into()).await;
        let cap = Capture::default();
        let outcome = tokio::time::timeout(
            Duration::from_secs(20),
            call_like_the_server(&manager, &client, name, &args).with_subscriber(cap.dispatch()),
        )
        .await;
        let Ok(result) = outcome else {
            failures.push(format!("{name}: timed out"));
            continue;
        };
        if result.is_none() {
            failures.push(format!("{name}: listed but not handled"));
            continue;
        }

        // One call, one span, one metric point, one log line.
        let spans = cap.new_spans().len();
        let metrics = cap.events(semconv::METRICS_TARGET);
        let logs = cap.events(semconv::TARGET).len();
        if spans != 1 || metrics.len() != 1 || logs != 1 {
            failures.push(format!(
                "{name}: {spans} spans, {} metric events, {logs} log events",
                metrics.len()
            ));
        }
        for m in &metrics {
            for key in m.fields.keys() {
                if key != semconv::HISTOGRAM_MCP_SERVER_OPERATION_DURATION
                    && !semconv::METRIC_ATTRIBUTES.contains(&key.as_str())
                {
                    failures.push(format!("{name}: metric attribute {key}"));
                }
            }
        }

        let haystack = cap.haystack();
        for (marker, path) in &canaries.by_marker {
            let top_level_allowed = allowed.contains(path.as_str());
            let may_appear = top_level_allowed || manifest_target.contains(&path.as_str());
            let appears = haystack.contains(marker.as_str());
            if appears && !may_appear {
                failures.push(format!("{name}: value of `{path}` leaked"));
            }
            if top_level_allowed && !appears {
                failures.push(format!("{name}: allowlisted `{path}` missing"));
            }
        }
        if name == "apply_manifest" {
            for path in manifest_target {
                // An explicit `namespace` argument wins over the manifest's.
                if path == "manifest.metadata.namespace" && args.get("namespace").is_some() {
                    continue;
                }
                let marker = &canaries
                    .by_marker
                    .iter()
                    .find(|(_, p)| p == path)
                    .unwrap()
                    .0;
                if !haystack.contains(marker.as_str()) {
                    failures.push(format!("{name}: manifest target `{path}` missing"));
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} problems:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[tokio::test]
async fn policy_rejection_through_handle_tool_is_a_denial() {
    let cap = Capture::default();
    let client = mock_client(ActionPermissions::default());
    let result = crate::mcp::handle_tool(
        &client,
        "create_service",
        &json!({"namespace": "prod", "name": "web", "type": "LoadBalancer", "port": 80}),
    )
    .with_subscriber(cap.dispatch())
    .await;
    assert!(matches!(result, Some(Err(_))), "{result:?}");
    let spans = cap.new_spans();
    assert_eq!(spans.len(), 1);
    let f = cap.span_fields(spans[0].span_id.unwrap());
    assert_eq!(field(&f, semconv::MCP_K8S_DECISION), Some("denied"));
    assert_eq!(
        field(&f, semconv::MCP_K8S_DENIED_RULE),
        Some("policy:allow_load_balancer")
    );
    assert_eq!(field(&f, semconv::ERROR_TYPE), Some("permission_denied"));
}

#[tokio::test]
async fn exec_is_denied_when_updates_are_disabled() {
    let cap = Capture::default();
    let client = mock_client(ActionPermissions::new(
        false,
        true,
        false,
        vec![],
        true,
        true,
    ));
    let result = crate::mcp::handle_tool(
        &client,
        "exec_pod",
        &json!({"namespace": "prod", "name": "web-1", "command": ["id"]}),
    )
    .with_subscriber(cap.dispatch())
    .await;
    assert!(matches!(result, Some(Err(_))));
    let f = cap.span_fields(cap.new_spans()[0].span_id.unwrap());
    assert_eq!(field(&f, semconv::MCP_K8S_ACTION), Some("exec"));
    assert_eq!(field(&f, semconv::MCP_K8S_DECISION), Some("denied"));
    assert_eq!(
        field(&f, semconv::MCP_K8S_DENIED_RULE),
        Some("global:update")
    );
}
