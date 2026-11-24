# Telemetry

mcp-k8s describes every MCP request with an OpenTelemetry trace span, a
duration metric and a log line, named after the OpenTelemetry
[MCP semantic conventions](https://github.com/open-telemetry/semantic-conventions-genai/blob/main/docs/gen-ai/mcp.md)
(status *Development*; implemented against commit `7cb940f`, 2026-11-20).

- The **library** (`mcp_k8s`) emits plain [`tracing`](https://docs.rs/tracing)
  spans and events. It has no OpenTelemetry dependency, so it works with
  whatever OpenTelemetry version the application uses (see
  [Embedding](#embedding-deckwatch)).
- The **`mcp-k8s` binary** exports them over OTLP when you point it at a
  collector. Without an endpoint nothing is exported, and nothing changes
  apart from the extra log lines described below.
- The Prometheus `/metrics` endpoint keeps working as before (see
  [Old metric names](#old-metric-names)).

## Turning on OTLP export

Set the standard OpenTelemetry environment variables:

```bash
OTEL_EXPORTER_OTLP_ENDPOINT=http://otel-collector.observability:4318 \
OTEL_RESOURCE_ATTRIBUTES=k8s.cluster.name=prod-east,deployment.environment.name=prod \
mcp-k8s --http
```

| Variable | Default | Effect |
|---|---|---|
| `OTEL_EXPORTER_OTLP_ENDPOINT` | *(none: export off)* | Collector base URL; `/v1/traces`, `/v1/metrics` and `/v1/logs` are appended. Use the http/protobuf port, usually 4318. `https://` works (rustls, system roots). |
| `OTEL_EXPORTER_OTLP_{TRACES,METRICS,LOGS}_ENDPOINT` | | Per-signal URL, used as is. Setting only one exports only that signal. |
| `OTEL_EXPORTER_OTLP_PROTOCOL` (and per-signal) | `http/protobuf` | Only `http/protobuf` is supported. Any other value (`grpc`, `http/json`) turns that signal off with a warning, rather than sending protobuf to a gRPC port. |
| `OTEL_EXPORTER_OTLP_HEADERS` (and per-signal) | | Extra headers, e.g. `authorization=Bearer <token>`. |
| `OTEL_EXPORTER_OTLP_TIMEOUT` (and per-signal) | `10000` | Export timeout in milliseconds. |
| `OTEL_SERVICE_NAME` | `mcp-k8s` | `service.name`. `service.version` is set to the mcp-k8s version. |
| `OTEL_RESOURCE_ATTRIBUTES` | | More resource attributes. Put `k8s.cluster.name` here for a single-cluster deployment. |
| `OTEL_TRACES_SAMPLER`, `OTEL_TRACES_SAMPLER_ARG` | `parentbased_always_on` | Trace sampling, e.g. `parentbased_traceidratio` and `0.25`. |
| `OTEL_{TRACES,METRICS,LOGS}_EXPORTER` | `otlp` | `none` turns one signal off. |
| `OTEL_SDK_DISABLED` | `false` | `true` turns all export off. |
| `OTEL_METRIC_EXPORT_INTERVAL`, `OTEL_BSP_*`, `OTEL_BLRP_*` | SDK defaults | Batching and export intervals. |
| `MCP_K8S_OTEL_FILTER` | `info` | [`EnvFilter`](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html) for exported spans and logs. `RUST_LOG` only controls console logs, so `RUST_LOG=warn` doesn't starve the exporter. |

At startup the server logs `exporting OTLP over http/protobuf` with the signals
it exports, or a warning for each setting it can't honour. On SIGTERM or
SIGINT it stops accepting requests, lets in-flight ones finish, and flushes
everything before exiting.

TLS uses rustls with the ring crypto provider that the binary installs at
startup (the same one the HTTPS server and the Kubernetes client use), and the
system's root certificates (the container image ships them). For a collector
with a private CA, point `SSL_CERT_FILE` at a PEM bundle that contains it (it
replaces the system roots, so include any public roots you also need).

With the [Helm chart](./deployment/helm.md#opentelemetry), set `otel.endpoint`
and friends instead.

## What is recorded

### Span

One span of kind `SERVER` per MCP request, named `{mcp.method.name} {target}`:
`tools/call apply_manifest`, `prompts/get diagnose-pod`, `tools/list`. The
target appears only for tools and prompts the server has, and methods outside
the conventions' list appear as `_OTHER`, so clients can't create unbounded
span names. In console logs the span is called `mcp_request`.

If the request carries W3C trace context in `params._meta.traceparent` /
`tracestate` (how the MCP conventions propagate it), that is the span's
parent. Otherwise the HTTP `traceparent` header is. When both are present the
header's context becomes a span link instead.

| Attribute | When | Example |
|---|---|---|
| `mcp.method.name` | always | `tools/call` |
| `jsonrpc.request.id` | requests (not notifications) | `7` |
| `mcp.protocol.version` | HTTP: from the `MCP-Protocol-Version` header; `initialize`; stdio after `initialize` | `2025-11-25` |
| `network.transport` | always | `tcp` (HTTP), `pipe` (stdio) |
| `network.protocol.name` | HTTP | `http` |
| `client.address`, `client.port` | HTTP: the TCP peer (a proxy, if there is one) | `10.0.0.7`, `51000` |
| `gen_ai.tool.name`, `gen_ai.operation.name` | `tools/call` | `apply_manifest`, `execute_tool` |
| `gen_ai.prompt.name` | `prompts/get` | `diagnose-pod` |
| `mcp.resource.uri` | `resources/read` | `k8s://prod/pods/web-1` |
| `mcp_k8s.action` | `tools/call` | `read`, `write`, `delete`, `exec` |
| `mcp_k8s.decision` | `tools/call` | `allowed`, `denied` |
| `mcp_k8s.denied_rule` | denied calls | `global:create`, `override:deployment-delete`, `apply_manifest:disabled`; workload guardrails report `policy:<setting>` (the policy settings that would allow the call) |
| `mcp_k8s.outcome` | always | `ok`, `error`, `denied`, `cancelled` |
| `error.type` | failures | see below |
| `rpc.status_code` | JSON-RPC error responses | `-32000` |
| `k8s.cluster.name` | `tools/call`, `resources/read`, with `--contexts` only | `staging` |
| `k8s.namespace.name`, `mcp_k8s.resource.kind`, `mcp_k8s.resource.name`, `mcp_k8s.resource.api_version` | `tools/call`, when the arguments name them | `prod`, `Deployment`, `web`, `apps/v1` |
| `mcp_k8s.tool.argument_keys` | `tools/call` | `container,name,namespace` |
| `mcp_k8s.tool.arguments` | `tools/call`, allowlisted values only | `{"container":"app","name":"web-1","namespace":"prod"}` |
| `mcp_k8s.client.name`, `mcp_k8s.client.version` | `initialize`; every request on stdio | `claude-code`, `2.1.0` |
| `mcp_k8s.request_id` | always, with or without OTLP | a UUID |
| `trace_id` | when traces are exported | the W3C trace id, for joining console logs with traces |

`mcp_k8s.client.*` is what the client says about itself in `initialize`:
treat it as a label, never as an identity. mcp-k8s doesn't issue
`Mcp-Session-Id`, so over HTTP the client info is on the `initialize` span
only and `mcp.session.id` is never set. `mcp_k8s.actor` is reserved for the
authenticated user once single sign-on exists.

`error.type` values:

| Value | Meaning |
|---|---|
| `tool_error` | The tool ran and failed. The server still answers with JSON-RPC error `-32000` (in `rpc.status_code`). |
| `permission_denied` | A permission setting or workload guardrail refused the call (`mcp_k8s.decision=denied`, rule in `mcp_k8s.denied_rule`). |
| `unknown_tool` | `tools/call` named a tool the server doesn't have. The span status stays unset: a caller mistake. |
| `cancelled` | The request was dropped before the tool finished (client went away). |
| `-32601`, `-32000`, … | Any other JSON-RPC error response, by code. `-32700`, `-32600`, `-32601`, `-32602` and `-32002` are caller mistakes and leave the span status unset. |

The span status is `ERROR` for the other failures. Its description is left
empty, although the conventions suggest the JSON-RPC error message: error
messages can contain argument values (the Kubernetes API echoes names and
invalid values), so they are never recorded.

### Metric

`mcp.server.operation.duration`: a histogram of request duration in seconds
(unit `s`, buckets `0.01, 0.02, 0.05, 0.1, 0.2, 0.5, 1, 2, 5, 10, 30, 60, 120,
300` as the conventions ask), one observation per request. It carries only
low-cardinality attributes:

- `mcp.method.name` (`_OTHER` for unknown methods),
- `gen_ai.tool.name` and `gen_ai.operation.name` (tool calls of known tools),
- `mcp_k8s.action` and `mcp_k8s.decision` (tool calls),
- `error.type` (failures).

Never the namespace, resource name, client, session or denied rule: those are
on the span. The binary's meter provider also drops any other attribute, so a
mistake can't raise the metric's cardinality.

### Logs

One `INFO` event at the end of every request, on target `mcp_k8s::telemetry`:

- `tool call finished`, with `gen_ai.tool.name`, `mcp_k8s.outcome`,
  `duration_ms`, `mcp_k8s.decision`, `mcp_k8s.denied_rule`,
  `mcp_k8s.action`, `error.type`, `k8s.namespace.name`,
  `mcp_k8s.resource.kind` and `mcp_k8s.resource.name`;
- `request finished` for other methods, with `mcp.method.name`,
  `mcp_k8s.outcome`, `duration_ms` and `error.type`;
- `client initialized` for `initialize`, with the client name and version and
  the requested and negotiated protocol versions.

With `--log-format json` each line also carries the request span's fields, so
`jq 'select(.fields["gen_ai.tool.name"] == "apply_manifest")'` finds every
`apply_manifest` call. Exported over OTLP, the log records carry the trace and
span ids. The per-request `processing request` line moved to `DEBUG`. In stdio
mode, console logs go to stderr, because stdout carries the JSON-RPC stream.

## Redaction guarantee

Telemetry never records tool argument values, apart from a short allowlist of
identifiers. It records the top-level argument **keys** (only keys the tool's
input schema declares; anything else is counted as `?`) and the values of
these scalar arguments:

| Argument | Why it is safe |
|---|---|
| `namespace`, `name`, `kind`, `api_version` | identify the object the call targets |
| `group`, `version` | the API group and version of a custom-resource call |
| `pod_name`, `job_name` | the target's name for `get_pod_logs` / `get_build_logs` |
| `container` | a container name |
| `image` | a container image reference |
| `replicas` | a replica count |
| `cluster` | the kubeconfig context an embedder's tool targets |
| `decode` | whether `get_secret` was asked for decoded values: worth auditing, and a boolean carries no data |

For `apply_manifest` and `create_custom_resource`, the manifest's `kind`,
`apiVersion`, `metadata.name` and `metadata.namespace` fill in the target.
Nothing else is read from it.

Everything else is dropped: Secret `data` and `stringData`, env values, exec
commands, manifest and patch bodies, label and annotation keys and values,
label and field selectors, plugin config, and error messages.

A canary test enforces this for every tool. It builds arguments from each
tool's input schema with a unique marker in every string (nested objects,
arrays and map keys included), runs the call against a mock API server that
echoes requests into its error messages, and fails if a marker from outside
the allowlist appears in any span or event field, or an allowlisted one goes
missing. New tools are covered automatically.

The guarantee covers what mcp-k8s itself records, at any level. Dependencies'
`DEBUG` output can contain request details (kube-client's `HTTP` spans carry
the request URL with label selectors, and it logs the API server's error
messages), which is one reason exported spans and logs default to `info`.

## Embedding (deckwatch)

An application that embeds the library gets this telemetry by exporting
`tracing` itself, with its own OpenTelemetry version:

1. **Pipeline.** Install `tracing-opentelemetry` (spans, plus its
   `MetricsLayer`) and `opentelemetry-appender-tracing` (logs). Keep
   `mcp_k8s=info` in the filter that feeds them, and filter the target
   `mcp_k8s::telemetry::metrics` out of console and OTLP logs: its events are
   metric observations, not log lines. Give the meter provider a view for
   `mcp.server.operation.duration` with unit `s`, the buckets above, and
   `mcp_k8s::telemetry::semconv::METRIC_ATTRIBUTES` as the allowed attribute
   keys. `MetricsLayer` creates instruments with none of these and turns every
   event field into an attribute. The binary's `src/otel.rs` does all of
   this.
2. **Request span.** Per request, create
   `mcp_k8s::telemetry::server_span(method, target)` before entering it, and
   set its remote parent with `OpenTelemetrySpanExt::set_parent`
   (`W3cTraceContext::from_meta` reads `params._meta`).
   `tracing-opentelemetry` 0.34 ignores name changes once a span is entered,
   so pass the tool name as `target` up front, but only for tools you serve
   (`telemetry::is_known_tool` covers mcp-k8s's).
3. **Tools.** `mcp_k8s::mcp::handle_tool` records into the request span by
   itself. Wrap your own tools with `telemetry::instrument_tool_call` or a
   `ToolCall` to get identical telemetry. A `handle_tool` call inside one of
   your tools reports into your call rather than starting a second one.
4. **Other methods.** Call `telemetry::finish_operation` at the end, and
   `telemetry::record_rpc_status` for JSON-RPC errors.

```rust
use mcp_k8s::telemetry;
use tracing::Instrument;

let tool = params["name"].as_str().unwrap_or("");
let args = &params["arguments"];
let own = is_deckwatch_tool(tool);
let target = (own || telemetry::is_known_tool(tool)).then_some(tool);
let span = telemetry::server_span("tools/call", target);
// span.set_parent(cx) with cx from telemetry::W3cTraceContext::from_meta(&params)
let result = async {
    if own {
        Some(telemetry::instrument_tool_call(tool, args, deckwatch_tool(state, tool, args)).await)
    } else {
        mcp_k8s::mcp::handle_tool(&client, tool, args).await
    }
}
.instrument(span)
.await;
```

Tools whose names don't use mcp-k8s's prefixes (`trigger_build`) can set the
action with `ToolCall::start_with_action`, and guardrails can call
`telemetry::record_denial(rule)` inside a call. `mcp_k8s.action` follows the
permission classification (`ActionPermissions::action_for_tool`): `exec_*`
tools are labelled `exec`, the permission action that gates them is
`update`. The rustdoc of
`mcp_k8s::telemetry` has the details.

## Old metric names

The Prometheus counters at `/metrics` are unchanged:
`mcp_k8s_tool_calls_total{tool,status}`,
`mcp_k8s_tool_call_duration_seconds{tool}` (both from the library, also inside
embedders) and `mcp_requests_total{method}` (binary). They overlap with
`mcp.server.operation.duration`, which carries the same information with
semantic-convention names. A later release will deprecate them in favour of
the OTel metric and remove them one minor release after that; the changelog
will say when.
