# mcp-k8s

A Kubernetes MCP (Model Context Protocol) server that exposes **211 tools**
covering all GA Kubernetes API resources, CRDs, multi-cluster management, and
resource watching. Works as a stdio server for Claude Code or as an HTTP
endpoint for in-cluster deployment.

## Features

- **211 MCP tools** across 49 resource modules covering every GA Kubernetes API group
- **Typed workload tools that cover the pod template**: containers, init containers and native sidecars, env from secrets/configmaps/fields, probes, lifecycle hooks, resources, volumes, scheduling, security context, rollout strategies, Services and Ingresses (see [coverage](docs/src/tools/coverage.md)), so agents don't need `apply_manifest` for everyday work
- **Workload guardrails on every write path**: privileged containers, host namespaces, hostPath, host ports, extra capabilities, NodePort/LoadBalancer Services, ingress snippet annotations and RBAC escalation are rejected unless an operator opts in, and `apply_manifest` enforces the same rules
- **Two runtime modes**: stdio (for Claude Code) and HTTP (for in-cluster deployment)
- **SSE transport**: `POST /mcp/sse` endpoint for Server-Sent Events streaming
- **Multi-cluster support**: load multiple kubeconfig contexts and switch between clusters at runtime
- **CRD discovery**: full CRUD lifecycle for any installed CRD via dynamic API discovery
- **Watch/subscribe**: `watch_resource` tool observes resource changes over a time window
- **MCP resources and prompts**: `resources/read` for direct `k8s://` URI access, built-in diagnostic prompts
- **OpenTelemetry**: one SERVER span per MCP request, the `mcp.server.operation.duration` histogram and logs over OTLP (http/protobuf), named after the OpenTelemetry MCP conventions, with tool arguments redacted to an allowlist ([Telemetry](#telemetry))
- **Prometheus metrics**: `GET /metrics` endpoint with request counts and tool call latency histograms
- **Bearer token auth and TLS**: `--auth-token` for HTTP authentication, `--tls-cert`/`--tls-key` for HTTPS
- **Action permission controls**: disable create/update/delete globally or per-resource via CLI flags and env vars
- **Namespace allowlisting**: restrict operations to specific namespaces
- **Token-efficient responses**: focused summary/detail types instead of raw K8s API objects
- **Structured JSON logging**: `--log-format json` for production log aggregation, one searchable line per call
- **Swagger UI**: interactive API docs at `/swagger-ui` in HTTP mode
- **Helm chart and Kustomize manifests** for production deployment

## Quick Start

### Claude Code (stdio mode)

Add to your Claude Code MCP server configuration:

```json
{
  "mcpServers": {
    "k8s": {
      "command": "/path/to/mcp-k8s",
      "args": []
    }
  }
}
```

### In-cluster (HTTP mode)

```bash
docker run -p 8080:8080 ghcr.io/alexconrey/mcp-k8s:latest
```

Or deploy with Helm:

```bash
helm install mcp-k8s ./helm/mcp-k8s
```

## Kubernetes version support

mcp-k8s 0.5 uses kube 5 and k8s-openapi 0.29 and supports **Kubernetes 1.33
and newer**. The binary and the Helm chart (`kubeVersion: ">=1.33.0-0"`) are
built against the `v1_33` API types. The k3s integration tests run against
Kubernetes 1.35.

Most tools use GA APIs, which every supported cluster serves. The tools for
newer APIs below use the newest version that k8s-openapi `v1_33` has. Clusters
serve alpha and beta versions only when they are enabled (`--runtime-config`);
otherwise the tool returns the API server's 404.

| Tools for | API version |
|-----------|-------------|
| IPAddress, ServiceCIDR | `networking.k8s.io/v1` (GA) |
| ClusterTrustBundle | `certificates.k8s.io/v1beta1` |
| LeaseCandidate | `coordination.k8s.io/v1beta1` |
| ResourceClaim, ResourceClaimTemplate, ResourceSlice, DeviceClass | `resource.k8s.io/v1beta2` (GA `v1` since 1.34 needs a `v1_34` floor) |
| VolumeAttributesClass | `storage.k8s.io/v1beta1` (GA `v1` since 1.34 needs a `v1_34` floor) |
| MutatingAdmissionPolicy, MutatingAdmissionPolicyBinding | `admissionregistration.k8s.io/v1alpha1` |
| StorageVersion | `internal.apiserver.k8s.io/v1alpha1` |
| StorageVersionMigration | `storagemigration.k8s.io/v1alpha1` (removed in Kubernetes 1.35) |

## Supported Resources

| Category | Resources |
|----------|-----------|
| **Core** | Pods, Services, ConfigMaps, Secrets, Namespaces, Events, Endpoints, PVs, PVCs, ServiceAccounts, Nodes, ResourceQuotas, LimitRanges |
| **Workloads** | Deployments, StatefulSets, DaemonSets, ReplicaSets, Jobs, CronJobs, HPAs |
| **Networking** | Ingresses, IngressClasses, NetworkPolicies, EndpointSlices |
| **RBAC** | Roles, RoleBindings, ClusterRoles, ClusterRoleBindings |
| **Policy** | PodDisruptionBudgets |
| **Storage** | StorageClasses, CSIDrivers, CSINodes, CSIStorageCapacity, VolumeAttachments |
| **Cluster** | Leases, PriorityClasses, RuntimeClasses, FlowSchemas, PriorityLevelConfigs |
| **Certificates** | CertificateSigningRequests (list, get, approve, deny) |
| **Admission** | MutatingWebhookConfigs, ValidatingWebhookConfigs, ValidatingAdmissionPolicies |
| **Auth** | `can_i`, `whoami`, `list_my_permissions` |
| **Generic** | `apply_manifest` (server-side apply), `get_resource_json` (full object as JSON; `get_resource_yaml` is a deprecated alias) |
| **Metrics** | Pod and node CPU/memory from metrics-server |
| **CRDs** | `list_crds`, `get_crd`, `list_custom_resources`, `get_custom_resource`, `create_custom_resource`, `update_custom_resource`, `delete_custom_resource` |
| **Watch** | `watch_resource` (observe ADDED/MODIFIED/DELETED events over a time window) |
| **Cluster Mgmt** | `list_clusters`, `switch_cluster`, `get_active_cluster` (multi-context switching) |

## Guardrails

Every write tool runs the same workload policy: the typed `create_*`/`update_*` tools, `apply_manifest`, and the custom resource tools. The defaults are restrictive and every rejection names the flag that relaxes it. Typed creates also default to a restricted securityContext (runAsNonRoot, seccomp `RuntimeDefault`, no privilege escalation, drop ALL capabilities) and don't mount a service account token unless asked.

| Rejected by default | Opt-in flag |
|---------------------|-------------|
| Privileged containers | `--allow-privileged` |
| `allowPrivilegeEscalation: true` | `--allow-privilege-escalation` |
| Capabilities beyond the runtime's defaults | `--allow-capabilities NET_ADMIN,...` |
| `hostNetwork` / `hostPID` / `hostIPC` | `--allow-host-namespaces` |
| `hostPath` volumes / `hostPort` | `--allow-host-path` / `--allow-host-ports` |
| nfs, inline csi and other risky volume types | `--allow-volume-types nfs,csi` |
| Unconfined seccomp/AppArmor, unsafe sysctls | `--allow-unconfined` |
| NodePort / LoadBalancer / ExternalName Services | `--allow-nodeport` / `--allow-loadbalancer` / `--allow-external-services` |
| Ingress `*-snippet` annotations and nginx config injection | `--allow-ingress-snippets` |
| RBAC escalation (escalate/bind/impersonate, wildcards, binding cluster-admin) | `--allow-rbac-escalation` |
| RBAC, webhook, CRD and cluster-scoped kinds in `apply_manifest` | `--allow-manifest-kinds Kind,...` |
| Custom resource writes | `--allow-custom-resource-groups group,...` |

Each flag has an `MCP_K8S_*` environment variable. Optional tightening: `--allowed-registries`, `--ingress-annotation-allowlist`, `--ingress-annotation-denylist`. See [Permissions](docs/src/permissions.md#workload-guardrails).

### Special Actions

Beyond standard CRUD, the server supports:
- **Deployment**: restart, scale, rollback
- **Node**: cordon, uncordon, drain, metrics
- **Pod**: exec (command execution), evict (PDB-aware)

## Configuration

| Flag | Env Var | Default | Description |
|------|---------|---------|-------------|
| `--http` | — | off | Run as HTTP server instead of stdio |
| `--listen` | `MCP_K8S_LISTEN` | `0.0.0.0:8080` | HTTP listen address |
| `--namespaces` | `MCP_K8S_NAMESPACES` | (all) | Comma-separated namespace allowlist |
| `--contexts` | `MCP_K8S_CONTEXTS` | (default) | Comma-separated kubeconfig contexts for multi-cluster |
| `--disable-create` | `DISABLE_CREATE` | `false` | Disable all create operations |
| `--disable-update` | `DISABLE_UPDATE` | `false` | Disable all update operations |
| `--disable-delete` | `DISABLE_DELETE` | `false` | Disable all delete operations |
| `--disable` | `MCP_K8S_DISABLE` | (none) | Comma-separated per-resource disables (e.g. `deployment-delete,secret-create`) |
| `--auth-token` | `AUTH_TOKEN` | (none) | Bearer token for HTTP endpoint authentication |
| `--tls-cert` | `TLS_CERT` | (none) | Path to TLS certificate PEM file (enables HTTPS) |
| `--tls-key` | `TLS_KEY` | (none) | Path to TLS private key PEM file (enables HTTPS) |
| `--disable-apply-manifest` | `DISABLE_APPLY_MANIFEST` | `false` | Block the `apply_manifest` tool entirely (it already enforces the same guardrails as the typed tools) |
| `--allow-*`, `--allowed-registries`, `--ingress-annotation-*`, `--no-secure-defaults` | `MCP_K8S_ALLOW_*` etc. | restrictive | Workload guardrails, see [Guardrails](#guardrails) |
| `--disable-secret-decode` | `DISABLE_SECRET_DECODE` | `false` | Prevent secret value decoding |
| `--cache-ttl` | `CACHE_TTL` | `0` | Response cache TTL in seconds (0 = disabled) |
| `--log-format` | `LOG_FORMAT` | `text` | Log format: `text` or `json` (stderr in stdio mode) |
| — | `KUBECONFIG` | in-cluster | Kubernetes config path |
| — | `OTEL_EXPORTER_OTLP_ENDPOINT` | (none) | OTLP http/protobuf collector URL; telemetry export is off without it. See [Telemetry](#telemetry) |
| — | `MCP_K8S_OTEL_FILTER` | `info` | Filter for spans and logs exported over OTLP (`RUST_LOG` only drives console logs) |

### Read-only mode

```bash
mcp-k8s --disable-create --disable-update --disable-delete
```

Disabled tools are hidden from `tools/list` so the LLM won't attempt to call them.

## Telemetry

Every MCP request produces one OpenTelemetry SERVER span, one observation of
the `mcp.server.operation.duration` histogram (seconds) and one log line,
named after the OpenTelemetry
[MCP semantic conventions](https://github.com/open-telemetry/semantic-conventions-genai/blob/main/docs/gen-ai/mcp.md)
(Development status, implemented against commit `7cb940f`). The library emits
them as plain `tracing`; the binary exports them over OTLP once an endpoint is
set:

```bash
OTEL_EXPORTER_OTLP_ENDPOINT=http://otel-collector:4318 \
OTEL_RESOURCE_ATTRIBUTES=k8s.cluster.name=prod mcp-k8s --http
```

- **Configuration:** the standard `OTEL_EXPORTER_OTLP_ENDPOINT` (and
  per-signal endpoints), `OTEL_EXPORTER_OTLP_PROTOCOL` (only
  `http/protobuf`), `OTEL_EXPORTER_OTLP_HEADERS`, `OTEL_SERVICE_NAME`
  (default `mcp-k8s`), `OTEL_RESOURCE_ATTRIBUTES`, `OTEL_TRACES_SAMPLER`,
  `OTEL_{TRACES,METRICS,LOGS}_EXPORTER=none` and `OTEL_SDK_DISABLED`, plus
  `MCP_K8S_OTEL_FILTER` (default `info`) for what gets exported. In Helm, set
  `otel.endpoint`. TLS uses rustls with the same ring provider as the rest of
  the binary. Telemetry is flushed on SIGTERM/SIGINT.
- **Span** `tools/call <tool>` (kind SERVER): `mcp.method.name`,
  `gen_ai.tool.name`, `gen_ai.operation.name=execute_tool`,
  `jsonrpc.request.id`, `mcp.protocol.version`, `client.address`,
  `network.transport`, `rpc.status_code`, `error.type`
  (`tool_error`, `permission_denied`, `unknown_tool`, `cancelled`, or the
  JSON-RPC code), `mcp_k8s.action` (`read`/`write`/`delete`/`exec`),
  `mcp_k8s.decision` and `mcp_k8s.denied_rule`, `mcp_k8s.outcome`, the target
  (`k8s.namespace.name`, `mcp_k8s.resource.kind`, `mcp_k8s.resource.name`,
  `mcp_k8s.resource.api_version`; `k8s.cluster.name` with `--contexts`), the
  argument keys and allowlisted values, `mcp_k8s.client.name`/`.version` from
  `initialize`, and `mcp_k8s.request_id`. The parent comes from
  `params._meta.traceparent`, else the HTTP `traceparent` header.
- **Metric** `mcp.server.operation.duration`: only `mcp.method.name`,
  `gen_ai.tool.name`, `gen_ai.operation.name`, `mcp_k8s.action`,
  `mcp_k8s.decision` and `error.type`. Never namespaces, names, clients or
  sessions.
- **Log** `tool call finished` (tool, outcome, `duration_ms`, decision,
  action, `error.type`, target) or `request finished`, on target
  `mcp_k8s::telemetry`.
- **Redaction:** telemetry records argument keys plus the values of
  `namespace`, `name`, `kind`, `api_version`, `group`, `version`,
  `pod_name`, `job_name`, `container`, `image`, `replicas`, `cluster` and
  `decode`, and never anything else: not Secret data, env values, exec
  commands, manifest or patch bodies, label or annotation values, selectors,
  plugin config or error messages. A canary test checks every tool's schema.
- **Old metric names:** the Prometheus counters at `/metrics` stay as they
  are for now; a later release will deprecate them in favour of
  `mcp.server.operation.duration`.

Embedders get the same telemetry from the library (see
[Library Usage](#library-usage)). The
[Telemetry docs page](https://alexconrey.github.io/mcp-k8s/latest/telemetry.html)
has the full attribute tables.

## Building

```bash
cargo build --release
```

## Container Image

```bash
docker build -t mcp-k8s .
```

Uses `gcr.io/distroless/cc-debian12:nonroot` as the runtime base. Runs
`mcp-k8s --http` on port 8080 by default.

## Deployment

### Helm

```bash
helm install mcp-k8s ./helm/mcp-k8s \
  --set namespaces="{staging,production}" \
  --set permissions.disableDelete=true
```

See [helm/mcp-k8s/values.yaml](helm/mcp-k8s/values.yaml) for all configuration options.

### Kustomize

```bash
# Full access
kubectl apply -k helm/charts/mcp-k8s/

# Read-only mode
kubectl apply -k helm/charts/mcp-k8s/read-only/
```

### RBAC

The server needs a ServiceAccount with appropriate K8s RBAC permissions.
See [docs/RBAC.md](docs/src/deployment/rbac.md) for full-access, read-only,
and per-namespace ClusterRole examples.

## Library Usage

The `mcp_k8s` crate can be embedded in other applications. Turn off the
default `server` feature, and choose the k8s-openapi version feature in your
own binary:

```toml
[dependencies]
mcp-k8s = { git = "https://github.com/alexconrey/mcp-k8s.git", tag = "v0.5.0", default-features = false }
kube = "5"
k8s-openapi = { version = "0.29", features = ["v1_33"] }
```

**Consumers choose the k8s-openapi version feature.** k8s-openapi allows only
one `v1_*` feature per build, so the library doesn't enable one: a second
choice would fail to compile. The `server` feature picks `v1_33` for the
`mcp-k8s` binary, which is another reason to disable it. The library is
tested with `v1_33` and also compiles with `v1_34`; `v1_35` and newer don't
compile yet.

```rust
use mcp_k8s::{K8sClient, mcp};
use mcp_k8s::permissions::ActionPermissions;

let client = K8sClient::try_default().await?;

// Get tool definitions (respects permission filtering)
let tools = mcp::tool_definitions(client.permissions());

// Dispatch a tool call
if let Some(result) = mcp::handle_tool(&client, "list_deployments", &args).await {
    // result: Result<String, String>
}
```

The workload guardrails live on `ActionPermissions`. `ActionPermissions::default()` uses the restrictive `WorkloadPolicy::default()`; relax individual rules with `with_workload_policy`:

```rust
use mcp_k8s::policy::WorkloadPolicy;

let mut policy = WorkloadPolicy::default();
policy.allow_load_balancer = true;
let permissions = ActionPermissions::default().with_workload_policy(policy);
```

`mcp_k8s::mcp::SERVER_INSTRUCTIONS` is the MCP `instructions` text the server sends. It steers agents to the typed tools, and embedding applications can include it in their own instructions.

### Telemetry in an embedding application

The library emits `tracing` only, never an `opentelemetry` crate, so it works
with whatever OpenTelemetry version you export with (OTel's global providers
live in the `opentelemetry` crate, so a library pinned to another 0.x version
would silently send nowhere). `mcp::handle_tool` instruments every call it
serves. To give your own tools identical telemetry under one request span:

```rust
use mcp_k8s::telemetry;
use tracing::Instrument;

// One SERVER span per request. Name it after the tool only if you serve it.
let target = (is_own_tool(tool) || telemetry::is_known_tool(tool)).then_some(tool);
let span = telemetry::server_span("tools/call", target);
// With your OTel crates, before the span is entered:
// span.set_parent(cx), cx from telemetry::W3cTraceContext::from_meta(&params)
let result = async {
    if is_own_tool(tool) {
        // A handle_tool call inside your tool reports into this call.
        Some(telemetry::instrument_tool_call(tool, &args, own_tool(tool, &args)).await)
    } else {
        mcp::handle_tool(&client, tool, &args).await
    }
}
.instrument(span)
.await;
```

Export with `tracing-opentelemetry` (spans, and its `MetricsLayer` for the
metric) and `opentelemetry-appender-tracing` (logs). Keep `mcp_k8s=info` in
their filter, keep the target `mcp_k8s::telemetry::metrics` (metric events)
out of console and OTLP logs, and give the meter provider a view with unit
`s`, the conventions' buckets and `telemetry::semconv::METRIC_ATTRIBUTES` as
the allowed keys. The rustdoc of `mcp_k8s::telemetry` and the Telemetry docs
page have the full recipe; the binary's `src/otel.rs` is a working example.

## Testing

```bash
cargo test                                      # @@UNIT@@ unit + @@BIN@@ binary + 8 integration + 30 guardrail tests
cargo test --no-default-features                # the same, as library consumers build it
cargo test --test k3s_integration -- --ignored  # 13 k3s tests (needs Docker)
cargo test --test k3s_workloads -- --ignored --test-threads=1  # 2 k3s tests for the typed workload tools and guardrails
```

The unit tests cover type extraction, serialization, tool definitions,
permission filtering, the pod template builder, every guardrail rule and
telemetry, including a canary test that runs every tool with marker values in
all its string arguments and fails if one outside the allowlist reaches a span,
metric or log. `tests/guardrails.rs` drives the tools against a mock API server
to check that the typed tools and `apply_manifest` reject the same things
before anything reaches the cluster. The binary's tests export the real
dispatch path into in-memory OpenTelemetry exporters. Never run a bare
`cargo test -- --ignored`: other ignored tests load your kubeconfig. The k3s
tests run against `rancher/k3s:v1.35.10-k3s1`;
set `MCP_K8S_K3S_TAG` to use another k3s release.

## License

MIT
