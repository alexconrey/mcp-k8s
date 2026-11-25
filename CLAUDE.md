# mcp-k8s

Kubernetes MCP (Model Context Protocol) server and library. Exposes
211 Kubernetes cluster operations as MCP tools consumable by Claude Code
and other MCP clients. Supports multi-cluster management, CRD discovery,
watch/subscribe, and all GA + alpha/beta K8s API groups.

## Agent bootstrap and keeping docs current

- **Local planning doc:** `PLAN.md`, if present, is the maintainer's working plan for releases, CI and security hardening. It is local-only (`.git/info/exclude`), so it may be missing on a fresh clone. On your first session with it, read it in full, verify its claims against the code and GitHub, and fold durable rules and facts into **this file** in a PR. Keep the roadmap and progress in `PLAN.md`.
- **Same-PR rule:** any change to behaviour, flags, tools, CI, releases or process updates the matching docs in the same PR: this file, `README.md`, `docs/src/` (mdBook), and `CHANGELOG.md` (under `## [Unreleased]`). When a mistake costs you time, add the lesson here.
- Before finishing, check that the counts and lists in this file (tools, modules, tests, flags) still match the code.

## Architecture

Rust project with a library crate (`mcp_k8s`) and a binary crate (`mcp-k8s`).

### Source Layout

```
src/
├── lib.rs              # Public API re-exports
├── main.rs             # Binary: stdio/HTTP/HTTPS modes, auth middleware, metrics, request spans
├── http_security.rs    # Binary: Host allowlist (DNS rebinding), cross-site blocking, optional CORS
├── otel.rs             # Binary only (`mod otel` in main.rs): OTLP pipeline, W3C trace context
├── cache.rs            # Optional in-memory response cache with TTL
├── client.rs           # K8sClient — kube::Client wrapper with namespace allowlist + permissions
├── cluster.rs          # ClusterManager — multi-cluster support with named contexts
├── error.rs            # Error types (NamespaceNotAllowed, Kube, BadRequest, ActionNotAllowed)
├── permissions.rs      # ActionPermissions — global/per-resource action controls + secret decode gating + WorkloadPolicy
├── policy/             # Workload guardrails, enforced on every write path
│   ├── mod.rs          # WorkloadPolicy (restrictive default, permissive()), flags table, Violation/PolicyError
│   ├── pod.rs          # Pod spec rules (privileged, escalation, capabilities, host namespaces/paths/ports, volumes, profiles, registries)
│   ├── network.rs      # Service rules (NodePort/LoadBalancer/ExternalName/externalIPs) and Ingress rules (snippets, nginx injection)
│   ├── rbac.rs         # RBAC escalation rules, service-account token Secrets, pod-security namespace labels
│   ├── csr.rs          # approve_csr: parse the PKCS#10 subject, refuse system:masters/system: identities
│   ├── csr_fixtures.rs # (test only) openssl-generated CSR PEMs
│   ├── manifest.rs     # check_object: kind dispatch, raw-manifest kind gates, custom resource group allowlist + deep scan
│   ├── admission.rs    # admit_manifest (custom resources: full policy; apply_manifest: scoping only), patch_checked (dry-run updates), check_typed, bound role lookup
│   └── tests.rs        # One test per rule: rejection and opt-in acceptance
├── types.rs            # Core summary/detail structs + re-exports from resource modules
├── extract.rs          # K8s API objects → summary/detail extraction functions
├── telemetry/          # tracing-only telemetry (no opentelemetry crates)
│   ├── mod.rs          # server_span, finish_operation, ClientInfo, W3cTraceContext, embedder docs
│   ├── semconv.rs      # Every attribute/metric name (OTel GenAI MCP conventions)
│   ├── summary.rs      # summarize(): action class, target, ALLOWED_ARGUMENTS redaction
│   ├── call.rs         # ToolCall: span attributes, metric event, end-of-call log, nesting
│   └── tests.rs        # Unit tests + the canary test over every tool schema
├── mcp/
│   ├── mod.rs          # Module re-exports
│   ├── protocol.rs     # JSON-RPC 2.0 types (with utoipa schemas)
│   ├── definitions.rs  # Tool inputSchema definitions (filtered by permissions)
│   ├── handlers.rs     # Tool dispatch with permission checks + metrics recording
│   └── tests.rs        # MCP dispatch, permission filtering, tool count and steering tests
└── resources/          # 49 resource modules + the shared pod template builder
    ├── mod.rs           # Aggregates all modules; adds "Preferred over apply_manifest" to every write tool
    ├── podspec.rs       # Shared container/pod argument schema, parser and builder (+ podspec/tests.rs)
    ├── admission.rs     # MutatingWebhookConfig, ValidatingWebhookConfig, ValidatingAdmissionPolicy
    ├── admission_alpha.rs # MutatingAdmissionPolicy/Binding (v1alpha1)
    ├── auth.rs          # can_i, whoami, list_my_permissions
    ├── cluster_mgmt.rs  # list_clusters, switch_cluster, get_active_cluster
    ├── cluster_trust_bundle.rs # ClusterTrustBundle (v1beta1)
    ├── clusterrole.rs   # ClusterRole CRUD
    ├── clusterrolebinding.rs  # ClusterRoleBinding CRUD
    ├── configmap.rs     # ConfigMap CRUD
    ├── crd.rs           # CRD discovery + custom resource CRUD (7 tools); writes go through admit_manifest/patch_checked
    ├── cronjob.rs       # CronJob CRUD (pod template + CronJob/Job settings)
    ├── csr.rs           # CertificateSigningRequest (list, get, approve, deny); approve runs policy::check_csr
    ├── daemonset.rs     # DaemonSet CRUD (pod template + update strategy)
    ├── deployment.rs    # Deployment create/update/delete/restart/scale/rollback (+ labels/annotations/affinity)
    ├── device_resources.rs # ResourceClaim, ResourceSlice, DeviceClass (v1beta2)
    ├── endpoints.rs     # Endpoints (read-only, legacy)
    ├── endpointslice.rs # EndpointSlice (read-only)
    ├── flowcontrol.rs   # FlowSchema + PriorityLevelConfiguration (read-only)
    ├── generic.rs       # apply_manifest (raw server-side apply; off by default, no workload policy, destructiveHint), get_resource_json (+ deprecated get_resource_yaml alias)
    ├── hpa.rs           # HorizontalPodAutoscaler CRUD
    ├── ingress.rs       # Ingress get/create/update/delete (rules, TLS, default backend)
    ├── ingressclass.rs  # IngressClass (read-only, cluster-scoped)
    ├── ip_networking.rs # IPAddress, ServiceCIDR (v1, GA)
    ├── job.rs           # Job list/get/create/delete + create_job_from_cronjob
    ├── lease.rs         # Lease (read-only)
    ├── lease_candidate.rs # LeaseCandidate (v1beta1)
    ├── limitrange.rs    # LimitRange CRUD
    ├── namespace.rs     # Namespace get/create/update/delete
    ├── networkpolicy.rs # NetworkPolicy CRUD
    ├── node.rs          # Node list/get + metrics/cordon/uncordon/drain
    ├── pdb.rs           # PodDisruptionBudget CRUD
    ├── pod.rs           # Pod list/get/create/delete/evict/exec/port_forward_check
    ├── priorityclass.rs # PriorityClass (read-only, cluster-scoped)
    ├── pv.rs            # PersistentVolume list/get/delete (cluster-scoped)
    ├── pvc.rs           # PersistentVolumeClaim CRUD
    ├── replicaset.rs    # ReplicaSet list/get (read-only)
    ├── resourcequota.rs # ResourceQuota CRUD
    ├── role.rs          # Role CRUD
    ├── rolebinding.rs   # RoleBinding CRUD
    ├── runtimeclass.rs  # RuntimeClass (read-only, cluster-scoped)
    ├── secret.rs        # Secret CRUD (values redacted by default, decode gated by permissions)
    ├── service.rs       # Service list/get/create/update/delete
    ├── serviceaccount.rs # ServiceAccount list/get/create/delete
    ├── statefulset.rs   # StatefulSet CRUD (pod template + volume claim templates, update strategy)
    ├── storage_internals.rs  # CSIDriver, CSINode, VolumeAttachment, CSIStorageCapacity
    ├── storage_migration.rs  # StorageVersionMigration (v1alpha1)
    ├── storage_version.rs    # StorageVersion (v1alpha1)
    ├── storageclass.rs  # StorageClass CRUD (cluster-scoped)
    ├── volume_attributes.rs  # VolumeAttributesClass (v1beta1)
    └── watch.rs         # watch_resource — stream K8s events for configurable duration
```

### Key Components

**K8sClient** (`client.rs`): Wraps `kube::Client` with a namespace allowlist
and `ActionPermissions`. Provides namespace-checked API factory methods.
The `permissions()` getter exposes permission controls including
`secret_decode_enabled` (off by default) for gating secret value access.

**ClusterManager** (`cluster.rs`): Holds multiple named `K8sClient` instances
behind `Arc<RwLock<HashMap>>` with an active cluster tracker. Supports
`--contexts` CLI flag for loading multiple kubeconfig contexts. Tools
`list_clusters`, `switch_cluster`, `get_active_cluster` manage the active
context at runtime.

**Permissions** (`permissions.rs`): `ActionPermissions` struct with global
create/update/delete disable flags, per-resource overrides,
`secret_decode_enabled`, `apply_manifest_enabled` (off by default;
`with_apply_manifest(true)`), and the `WorkloadPolicy`
(`with_workload_policy`, `workload_policy()`). Disabled tools are filtered
from `tools/list` and rejected in `tools/call`. `exec_pod` counts as an
update (`pod-update`).

**Workload policy** (`policy/`): the guardrails. `check_object(policy, json,
Source)` is the single rule engine; it works on the Kubernetes JSON form, so
typed tools and raw manifests get the same verdict. Every write path must go
through it:
- typed creates: build the object, then `policy::check_typed(client, &obj)`;
- updates that can change pod templates, Service/Ingress specs or namespace
  labels: `policy::patch_checked` (server-side dry run, check, then write
  with the dry run's `resourceVersion`);
- custom resource manifests: `policy::admit_manifest` (static checks, kind
  gates, discovery, namespace allowlist, bound roles). For `apply_manifest`
  it runs only discovery, the namespace allowlist and the create/update
  switches: `apply_manifest` skips the policy by design (see Security
  posture).
A new typed write tool that skips these is a guardrail bypass. Rejections name the
flag that allows them (`policy::flags`); keep CLI flags in `main.rs`, the
flags table and the docs in sync.

**Pod template builder** (`resources/podspec.rs`): one schema and parser for
container and pod arguments, shared by every workload create/update tool.
snake_case arguments with camelCase aliases, unknown fields rejected
(`normalize_args` for top-level arguments, `deny_unknown_fields` for nested
ones), secure defaults applied when `WorkloadPolicy::secure_defaults` is on.

**Types and extraction** (`types.rs`, `extract.rs`): Core summary/detail
structs. `types.rs` re-exports all per-resource module types for a unified
import path.

**Response cache** (`cache.rs`): Optional in-memory cache with configurable
TTL for read operations. Disabled by default (`--cache-ttl 0`). When enabled,
caches list operation responses to reduce K8s API load on large clusters.

**Resource modules** (`resources/*.rs`): 49 modules (plus `podspec.rs`), each with
`tool_definitions()` and `handle_tool()`. CRD module uses `DynamicObject`
+ `kube::discovery::pinned_kind` for runtime GVK resolution. Watch module
uses `Api::watch()` with timeout for event streaming (30s hard cap).

**MCP protocol** (`mcp/`): JSON-RPC 2.0 with utoipa schemas. Supports
`tools/list`, `tools/call`, `resources/list`, `resources/read`,
`prompts/list`, `prompts/get`, and `ping` keepalive. SSE transport at
`/mcp/sse`.

**Observability**: Prometheus metrics at `/metrics`:
`mcp_k8s_tool_calls_total{tool,status}` (`status` is `ok`, `error` or
`denied`) and `mcp_k8s_tool_call_duration_seconds{tool}`, both emitted by the
library's `mcp::handle_tool()`, plus `mcp_requests_total{method}` from the
server. There is no separate errors counter. **Deprecated since v0.7.0,
removed in v0.8.0** in favour of the OTel metric; their `# HELP` says so.
Every write/delete/exec and every denied call also emits an `audit` event on
the `mcp_k8s::audit` target (redacted, like the `tool call finished` event).

**MCP errors:** a tool that ran and failed, or was refused, returns a result
with `isError: true` (the model sees the message). An unknown tool or prompt
is JSON-RPC `-32602`.

**Telemetry** (`telemetry/`, `otel.rs`): one SERVER span per MCP request
(`telemetry::server_span`, created by `dispatch` in `main.rs`), the
`mcp.server.operation.duration` histogram as a `tracing` event for
`tracing-opentelemetry`'s `MetricsLayer`, and a `tool call finished` /
`request finished` log line, named after the OTel GenAI MCP conventions
(`telemetry::semconv`, which records the commit implemented). `handle_tool`
instruments itself with `telemetry::ToolCall`; inside a running `ToolCall`
(the binary wraps every known tool in one; deckwatch can too) it reports into
it instead, so a call never gets two spans. The binary exports over OTLP
http/protobuf when `OTEL_EXPORTER_OTLP_ENDPOINT` is set, and drains and
flushes on SIGTERM/SIGINT. Spans carry `mcp_k8s.request_id` always and
`trace_id` when exporting. Structured JSON logging via `--log-format json`;
logs go to stderr in stdio mode. The Telemetry page in `docs/src/` lists
every name.

## Library consumers (deckwatch)

deckwatch vendors the `mcp_k8s` library as a git dependency with
`default-features = false`. Since #12, the HTTP server stack (`axum`,
`axum-server`, `utoipa`, `utoipa-swagger-ui`, `clap`, `tower-http`, …) is
behind the default-on `server` feature, which the `mcp-k8s` binary requires.

- The library must keep building, linting and testing **without** `server`.
  CI runs `cargo clippy --lib --no-default-features --features
  k8s-openapi/v1_33 -- -D warnings` and `cargo test --no-default-features`.
- **Consumers choose the k8s-openapi version feature.** k8s-openapi allows
  one `v1_*` feature per build, so never enable one on the `[dependencies]`
  entry (CI fails if you do). The `server` feature enables
  `k8s-openapi/v1_33` for our binary, `[dev-dependencies]` enables it for
  tests, and deckwatch enables its own. Library-only commands that don't build
  dev-dependencies (`cargo check`/`clippy`/`doc` with `--no-default-features`)
  need `--features k8s-openapi/v1_33` (or `K8S_OPENAPI_ENABLED_VERSION=1.33`).
- **Kubernetes floor: 1.33** (`v1_33`, the oldest k8s-openapi 0.29 offers;
  production runs 1.35). Alpha/beta modules use the newest API version in
  `v1_33`; README "Kubernetes version support" lists them. Moving one to
  another API version changes tool behaviour: note it in the changelog. The
  library also compiles with `v1_34`, not yet with `v1_35`+.
- Don't add server-only dependencies to library code. Gate them with
  `#[cfg(feature = "server")]` or `#[cfg_attr(feature = "server", ...)]`.
- Changes to the public library API, such as `K8sClient::new(client,
  allowed_namespaces, permissions)` and `mcp::handle_tool`, are **breaking for
  deckwatch**. Ship them in a release with a semver-appropriate version bump
  (see [Releasing](#releasing)).
- deckwatch relies on the namespace allowlist and `ActionPermissions` being
  enforced by every tool.
- **The library stays tracing-only.** No `opentelemetry*` crate may enter its
  dependency graph or public API: OTel's global providers are statics inside
  the `opentelemetry` crate, so an embedder on another 0.x version would
  silently get no telemetry. `cargo tree -e normal --no-default-features |
  grep -i opentelemetry` must print nothing. OTel crates belong behind the
  `otel` feature (enabled by `server`) and are used only in `main.rs` and
  `otel.rs`.
- **The OTel crates move together** (`opentelemetry*` 0.33,
  `tracing-opentelemetry` 0.34): each `tracing-opentelemetry` release pins
  one `opentelemetry` minor. Dependabot groups them (`otel-stack`).
- **Telemetry names live in `telemetry::semconv`.** Don't spell an attribute
  name inline, and keep metric events to `semconv::METRIC_ATTRIBUTES`:
  `MetricsLayer` turns every event field (even a message) into a metric
  attribute. `tracing-opentelemetry` 0.34 ignores `otel.name` changes once a
  span has been entered, so the span name must be final at creation.
- The `kube` and `k8s-openapi` versions (currently kube 5, k8s-openapi 0.29;
  the library enables no `v1_*` feature, see above) are part of the public
  API, because `K8sClient::new` takes a `kube::Client`. deckwatch must use the
  same majors, so bumping them is a breaking change: coordinate it with a
  deckwatch PR and ship it in a minor release. Raising the `k8s-openapi`
  feature in `server` and `[dev-dependencies]` also raises the minimum
  supported Kubernetes version.

## Runtime Modes

### Stdio (default)

```
mcp-k8s [--namespaces ns1,ns2] [--disable-create] [--contexts ctx1,ctx2]
```

Reads newline-delimited JSON-RPC from stdin, writes responses to stdout.

### HTTP server

```
mcp-k8s --http [--listen 0.0.0.0:8080] [--auth-token SECRET]
```

Axum-based HTTP server with endpoints:
- `POST /mcp` — MCP JSON-RPC endpoint
- `POST /mcp/sse` — SSE transport (Server-Sent Events)
- `GET /healthz` — health check
- `GET /metrics` — Prometheus metrics
- `GET /swagger-ui` — interactive API docs
- `GET /openapi.json` — OpenAPI spec

### HTTPS server

```
mcp-k8s --http --tls-cert /path/cert.pem --tls-key /path/key.pem
```

## Configuration

| Flag | Env Var | Default | Description |
|------|---------|---------|-------------|
| `--http` | — | off | Run as HTTP server |
| `--listen` | `MCP_K8S_LISTEN` | `0.0.0.0:8080` | HTTP listen address |
| `--namespaces` | `MCP_K8S_NAMESPACES` | (all) | Namespace allowlist |
| `--contexts` | `MCP_K8S_CONTEXTS` | (default) | Kubeconfig contexts for multi-cluster |
| `--disable-create` | `DISABLE_CREATE` | `false` | Disable all create ops |
| `--disable-update` | `DISABLE_UPDATE` | `false` | Disable all update ops |
| `--disable-delete` | `DISABLE_DELETE` | `false` | Disable all delete ops |
| `--disable` | `MCP_K8S_DISABLE` | (none) | Per-resource disables (e.g. `deployment-delete`) |
| `--auth-token` | `AUTH_TOKEN` | (none) | Bearer token for HTTP auth |
| `--auth-token-file` | `AUTH_TOKEN_FILE` | (none) | Read the bearer token from a file |
| `--allowed-hosts` | `MCP_K8S_ALLOWED_HOSTS` | loopback names on a loopback `--listen`, else any | `Host` allowlist (DNS rebinding); `/healthz` exempt |
| `--cors-allowed-origins` | `MCP_K8S_CORS_ALLOWED_ORIGINS` | (none) | CORS + cross-site allowlist for browser clients |
| `--max-request-bytes` | `MCP_K8S_MAX_REQUEST_BYTES` | `4194304` | Request body limit (413) |
| `--request-timeout-secs` | `MCP_K8S_REQUEST_TIMEOUT_SECS` | `360` | Request timeout (408) |
| `--tls-cert` | `TLS_CERT` | (none) | TLS certificate PEM path |
| `--tls-key` | `TLS_KEY` | (none) | TLS private key PEM path |
| `--enable-apply-manifest` | `ENABLE_APPLY_MANIFEST` | `false` | Enable `apply_manifest`: unrestricted raw server-side apply, no workload policy (namespace allowlist and disable switches still apply) |
| `--disable-apply-manifest` | `DISABLE_APPLY_MANIFEST` | `false` | Deprecated (off by default); still wins over `--enable-apply-manifest` |
| `--allow-privileged` | `MCP_K8S_ALLOW_PRIVILEGED` | `false` | Guardrail opt-in: privileged containers |
| `--allow-privilege-escalation` | `MCP_K8S_ALLOW_PRIVILEGE_ESCALATION` | `false` | Guardrail opt-in |
| `--allow-capabilities` | `MCP_K8S_ALLOW_CAPABILITIES` | (none) | Extra capabilities (`ALL` = any) |
| `--allow-host-namespaces` | `MCP_K8S_ALLOW_HOST_NAMESPACES` | `false` | hostNetwork/hostPID/hostIPC |
| `--allow-host-path` | `MCP_K8S_ALLOW_HOST_PATH` | `false` | hostPath volumes |
| `--allow-host-ports` | `MCP_K8S_ALLOW_HOST_PORTS` | `false` | hostPort |
| `--allow-volume-types` | `MCP_K8S_ALLOW_VOLUME_TYPES` | (none) | Extra volume types (`nfs,csi`; `*` = any but hostPath) |
| `--allow-unconfined` | `MCP_K8S_ALLOW_UNCONFINED` | `false` | Unconfined profiles, unsafe sysctls, probe hosts |
| `--allowed-registries` | `MCP_K8S_ALLOWED_REGISTRIES` | (any) | Image registry allowlist |
| `--allow-nodeport` | `MCP_K8S_ALLOW_NODEPORT` | `false` | NodePort Services |
| `--allow-loadbalancer` | `MCP_K8S_ALLOW_LOADBALANCER` | `false` | LoadBalancer Services + LB fields/annotations |
| `--allow-external-services` | `MCP_K8S_ALLOW_EXTERNAL_SERVICES` | `false` | ExternalName, externalIPs |
| `--allow-ingress-snippets` | `MCP_K8S_ALLOW_INGRESS_SNIPPETS` | `false` | Snippet annotations, nginx config syntax |
| `--ingress-annotation-allowlist` / `--ingress-annotation-denylist` | `MCP_K8S_INGRESS_ANNOTATION_ALLOWLIST` / `_DENYLIST` | (none) | Ingress annotation filters |
| `--allow-manifest-kinds` | `MCP_K8S_ALLOW_MANIFEST_KINDS` | (none) | Cluster-scoped custom resource kinds the custom resource tools may write |
| `--allow-custom-resource-groups` | `MCP_K8S_ALLOW_CUSTOM_RESOURCE_GROUPS` | (none) | Custom resource groups that may be written |
| `--allow-rbac-escalation` | `MCP_K8S_ALLOW_RBAC_ESCALATION` | `false` | RBAC escalation primitives, SA token Secrets |
| `--allow-privileged-csr` | `MCP_K8S_ALLOW_PRIVILEGED_CSR` | `false` | `approve_csr` for `system:masters`/other `system:` identities and unparseable requests |
| `--no-secure-defaults` | `MCP_K8S_NO_SECURE_DEFAULTS` | `false` | Skip the restricted securityContext defaults |
| `--enable-secret-decode` | `ENABLE_SECRET_DECODE` | `false` | Let tools return Secret values (redacted everywhere otherwise) |
| `--disable-secret-decode` | `DISABLE_SECRET_DECODE` | `false` | Deprecated (off by default); still wins |
| `--cache-ttl` | `CACHE_TTL` | `0` | Response cache TTL in seconds (0 = disabled) |
| `--log-format` | `LOG_FORMAT` | `text` | Log format: `text` or `json` (stderr in stdio mode) |
| — | `KUBECONFIG` | in-cluster | K8s config path |
| — | `RUST_LOG` | `info` | Console log level |
| — | `OTEL_EXPORTER_OTLP_ENDPOINT` | (none: off) | OTLP http/protobuf collector; other standard `OTEL_*` variables apply (protocol, headers, service name, resource attributes, sampler, `*_EXPORTER=none`, `OTEL_SDK_DISABLED`) |
| — | `MCP_K8S_OTEL_FILTER` | `info` | `EnvFilter` for spans and logs exported over OTLP |

## Security posture

mcp-k8s is a standalone server with broad cluster powers, so treat server
weaknesses as security issues. Current defaults:

- `--listen` defaults to `0.0.0.0:8080`, and the auth token is optional
  (maintainer's call: warn, don't refuse). Without one the HTTP API is
  unauthenticated, and the server logs a warning at startup on a
  non-loopback address.
- Browser protections live in `src/http_security.rs`: no CORS headers unless
  `--cors-allowed-origins`, browser-initiated cross-site writes get 403, and a
  `Host` allowlist (`--allowed-hosts`; loopback names by default on a
  loopback listener) stops DNS rebinding. Keep the guard layer outside the
  auth layer, and keep `/healthz` exempt from the Host check.
- Secret decoding is on by default (`ActionPermissions::default()`, which
  deckwatch also uses). The workload guardrails are restrictive by default
  (`WorkloadPolicy::default()`).
- **Secret values are off by default** (`--enable-secret-decode`,
  `with_secret_decode(true)`). Every path that can return a Secret must
  redact it when decoding is off: `get_secret` refuses `decode`, and the
  generic object tools call `resources::secret::redact_secret_json` (values
  and the `last-applied-configuration` annotation). A new tool that returns
  raw objects must do the same.
- `apply_manifest` is **off by default**. Enabled (`--enable-apply-manifest`),
  it's an unrestricted raw server-side apply that skips the workload policy.
  That's deliberate, the maintainer's call: it's for users who know exactly
  what they're applying, and the risk is an agent that's confident while its
  user isn't. So the controls belong to humans: an operator opt-in, the
  `destructiveHint` annotation (clients ask the user per call), and
  agent-facing text that requires the user's explicit request and
  confirmation.
- An empty namespace allowlist means every namespace.

Rules:

- Compare tokens in constant time (`subtle::ConstantTimeEq`, as `src/main.rs`
  does). Never log tokens or secret values.
- **Tool argument values never go into telemetry** (spans, metrics, logs,
  error messages), apart from the scalars in
  `telemetry::summary::ALLOWED_ARGUMENTS`. Record arguments only through
  `telemetry::summarize`, never `tracing::info!(?args)` or an error string: API
  server messages echo names and invalid values. Widening the allowlist needs
  a reason in `ALLOWED_ARGUMENTS` and an update to the `allowlist_is_reviewed`
  snapshot test.
- **Every tool is covered by the canary test**
  (`telemetry::tests::canary_no_argument_value_reaches_telemetry`), which
  walks `tool_definitions()` and fills every string with a marker. A new tool
  is covered automatically; if it fails, fix the leak, don't exempt the tool.
  It also fails for a listed tool that `handle_tool` (or the cluster
  manager) doesn't handle.
- Every namespaced tool must check the namespace allowlist and
  `ActionPermissions`. New tools need tests that prove both.
  `tests/namespace_allowlist.rs` calls every tool with a `namespace`
  argument with a disallowed namespace and requires a refusal before any
  API request; a tool without one fails it until it's added to an exemption
  list with a reason. Check the allowlist before discovery, policy or
  lookups (e.g. a binding's role). Under an allowlist an omitted namespace
  must never mean "all namespaces": refuse (`client.namespace_required_error()`).
- `exec_pod` refuses pods with node-level access (the policy's privileged,
  host namespace, hostPath, capability and unconfined rules, via
  `resources::pod::exec_violations`) unless the operator allows them, and
  has a timeout (default 60s, max 300s) and a 1 MiB per-stream output cap.
- Every typed write tool and the custom resource tools must run the workload
  policy (see Key Components, "Workload policy"). The custom resource tools
  must never be a way around a rule a typed tool enforces; add a test
  (`tests/guardrails.rs`) for each new rule.
- **Don't add guardrails to `apply_manifest`, and don't turn it on by
  default.** Keep its description, `SERVER_INSTRUCTIONS` and the policy
  rejection message telling agents to use it only on the user's explicit
  request, never as a workaround, and keep its `destructiveHint: true`
  annotation (tests check all of these). Anything that enumerates "every
  tool" (telemetry's known-tool list, the canary test, tool counts) must use
  `ActionPermissions::default().with_apply_manifest(true)`.
- Don't loosen defaults. Tightening a default is a breaking change: record it
  in `CHANGELOG.md` and ship it in a minor release, coordinated with
  deckwatch.

## Resource Module Pattern

Each resource module in `src/resources/` follows this pattern:

```rust
fn api(client: &K8sClient, ns: &str) -> Result<kube::Api<T>, String> {
    if !client.is_namespace_allowed(ns) {
        return Err(format!("Namespace '{ns}' is not in the allowed list"));
    }
    Ok(kube::Api::namespaced(client.inner().clone(), ns))
}

#[derive(Serialize, Debug)]
pub struct ResourceSummary { ... }

fn extract_summary(resource: &T) -> ResourceSummary { ... }

pub fn tool_definitions() -> Vec<serde_json::Value> { ... }

pub async fn handle_tool(
    client: &K8sClient, name: &str, args: &serde_json::Value
) -> Option<Result<String, String>> { ... }

#[cfg(test)] mod tests { ... }
```

List operations support `label_selector` and `field_selector` parameters.
Write operations add `app.kubernetes.io/managed-by: mcp-k8s` labels.
Workload create/update tools build their pod template with
`resources::podspec` and pass the result through the policy before writing.
All output uses `serde_json::to_string_pretty`.

## Project Layout

```
mcp-k8s/
├── src/                    # Rust source (lib + bin)
├── tests/
│   ├── integration.rs      # Tower-test mock integration tests (8 tests)
│   ├── guardrails.rs       # Mock-API guardrail tests for the typed tools; apply_manifest off by default / raw when enabled; approve_csr identity check; exec_pod node-access check (36 tests)
│   ├── namespace_allowlist.rs # Every namespaced tool refuses a namespace outside the allowlist offline; omitted-namespace, namespace-object and drain_node cases (15 tests)
│   ├── typed_tools.rs      # Mock-API request-body tests for ConfigMap/Secret/PVC/PDB/SA/HPA/RBAC tools (9 tests)
│   ├── k3s_integration.rs  # Testcontainers k3s tests (13 tests, #[ignore]; k3s v1.35, MCP_K8S_K3S_TAG overrides)
│   └── k3s_workloads.rs    # k3s tests for the typed tools and guardrails (3 tests, #[ignore])
├── docs/                   # mdBook documentation (GitHub Pages)
│   ├── book.toml
│   ├── src/                # 23 documentation pages
│   └── theme/              # Version switcher (JS + CSS)
├── helm/
│   ├── mcp-k8s/            # Helm chart (with PDB, HPA, NetworkPolicy, Ingress)
│   └── charts/mcp-k8s/     # Raw K8s manifests + Kustomize (base/, read-only/, network-policy/)
├── .github/
│   ├── dependabot.yml      # Weekly cargo/docker/actions updates, grouped
│   └── workflows/
│       ├── ci.yaml         # Build, test, clippy, fmt, cargo deny on PR
│       ├── audit.yaml      # Weekly RustSec advisory scan (cargo deny)
│       ├── release.yaml    # Docker + Helm OCI + GitHub Release on tag
│       └── docs.yaml       # Versioned mdBook deploy to GitHub Pages
├── Dockerfile              # Static musl binary in scratch container
├── Makefile                # 18 targets (build, test, test-lib, fmt, clippy, clippy-lib, docker, deploy, etc.)
├── CHANGELOG.md            # Keep a Changelog; checked by the release workflow
├── deny.toml               # cargo-deny config (advisories, licenses, bans, sources)
├── .pre-commit-config.yaml # fmt + clippy pre-commit hooks
├── rust-toolchain.toml     # Pinned toolchain; must match the Dockerfile rust: tag
├── Cargo.toml
└── README.md
```

## Git Workflow

The `main` branch is protected — direct pushes are blocked. All changes must
go through a feature branch and PR, including tool additions, bug fixes, and
documentation updates.

```bash
git checkout -b feature/my-change
# make changes
cargo check && cargo clippy -- -D warnings && cargo fmt
git push -u origin feature/my-change
gh pr create --title "..." --body "..."
```

Do **not** push directly to `main`. Even one-liner changes need a PR so the
CI workflow (build, test, clippy, fmt) runs against them before merge.

- **The maintainer merges** (squash) once CI is green. Agents open PRs but
  don't merge them.
- No `Co-Authored-By` or Claude/AI attribution in commits or PR descriptions.
- No force-pushes. No tags, releases or ruleset changes without explicit
  maintainer approval.
- Work in a git worktree under `/tmp`, because the maintainer's checkout may
  sit on an old branch. Never use `git stash`.
- **Never run the server against a real cluster by accident.** The local
  kube context may point at a production cluster, and the binary connects to
  the current context at startup. Use the
  testcontainers k3s tests (`cargo test --test k3s_integration -- --ignored`)
  for end-to-end checks.

## Building and Testing

```bash
cargo build --release       # Build
cargo test                  # 568 unit + 26 binary (router, telemetry, HTTP) + 11 integration + 36 guardrail + 15 namespace-allowlist + 9 typed-tool tests
cargo test --no-default-features  # Same tests without `server` (what deckwatch compiles)
cargo test --test k3s_integration -- --ignored --test-threads=2  # 13 k3s tests (needs Docker), as CI runs them
cargo test --test k3s_workloads -- --ignored --test-threads=1  # k3s typed-tool and guardrail tests, as CI runs them
K8S_OPENAPI_ENABLED_VERSION=1.33 cargo semver-checks check-release --baseline-rev <last tag> --only-explicit-features  # Library API vs the last release
cargo clippy --all-targets -- -D warnings  # Lint
cargo clippy --lib --no-default-features --features k8s-openapi/v1_33 -- -D warnings  # Library-only build (what deckwatch compiles)
cargo fmt --check           # Format check
make check                  # cargo fmt (rewrites) + both clippy and both test configs.
                            # Not the k8s-openapi feature check, k3s or semver, so not quite CI
make deny                   # cargo deny check (advisories, licenses, bans, sources)
```

### What CI does and doesn't cover

`ci.yaml` runs these on every PR:

- **`check`:** the toolchain/Dockerfile match, the binary-file check,
  `fmt --check`, both clippy configurations, a check that the library's
  k8s-openapi dependency enables no version feature, `build --release`, and
  `cargo test` with and without default features. Cargo commands use
  `--locked`, so commit `Cargo.lock` changes.
- **`msrv`:** checks the library-only build on Cargo.toml's `rust-version`.
- **`deny`:** runs `cargo deny` with `deny.toml`. Advisories only warn; see
  [Dependencies and advisories](#dependencies-and-advisories).
- **`k3s`:** the testcontainers k3s tests: `k3s_integration` (13 tests, each
  starting its own cluster) and `k3s_workloads` (the typed workload tools and
  the guardrails, one thread). The only end-to-end coverage of the real kube
  API paths, including the namespace allowlist and the policy.
- **`semver`:** `cargo semver-checks` of the library-only API against the last
  release tag. **Warn-only**: PRs don't bump the version, so a breaking change
  shows as a failed `semver` check without failing the run. That's the signal
  that the next release must bump the minor version (see
  [Releasing](#releasing)). Never make `semver` a required status check.
- **`docker-build`:** builds the release image without pushing it.

Runs on `main` publish the image instead (`docker` job). A new push to a PR
cancels its outdated run; runs on `main` are never cancelled. Every action is
pinned to a commit SHA with a version comment, and Dependabot keeps them
current.

**Toolchain:**
- `rust-toolchain.toml` pins the toolchain CI and local builds use (currently
  1.99), and CI installs exactly that.
- The Dockerfile's `rust:` tag must have the same minor version, and the
  `check` job fails if they differ. Dependabot's docker PRs bump only the
  Dockerfile, so push a `rust-toolchain.toml` bump to the same PR before
  merging it.
- `rust-version` in Cargo.toml (1.95, kube 5's minimum) is the MSRV for
  library consumers. Raise it only when a dependency requires it, since it's
  a breaking change for consumers on an older rustc.

Gaps to cover yourself:

- **k3s test images are pinned by digest** (`DEFAULT_K3S_TAG`, `BUSYBOX`,
  `PAUSE`, … in `tests/k3s_*.rs`). Dependabot can't see them: bump them by
  hand with `docker buildx imagetools inspect <image>` (use the index digest
  so amd64 CI and arm64 laptops get the same image).
- **Only k3s tests are `#[ignore]`d**, and none of the tests read your
  kubeconfig. (The `cluster.rs` tests used to, behind `#[ignore]`; they now use
  a mock client and run in plain `cargo test`.) Keep it that way: tests that
  need a cluster use testcontainers k3s, never `Client::try_default()`.
- **The `default` ruleset has no required status checks**, so red CI doesn't
  block a merge. Check `gh pr checks` is green before handing a PR over.

### Dependencies and advisories

- **Dependabot** (`.github/dependabot.yml`) opens weekly PRs for cargo, the
  Dockerfile base images and GitHub Actions. Minor and patch cargo updates are
  grouped, and majors of `kube`/`k8s-openapi`, the axum/tower/hyper stack and
  `utoipa` are grouped so they land together. Cargo updates wait 3 days (7 for
  majors). Dependabot alerts and security updates are enabled on the repo, and
  security PRs skip the schedule. Dependency PRs never bump the crate or
  chart version.
- A **`kube-stack` major is a breaking release** (see
  [Library consumers](#library-consumers-deckwatch)): don't merge it as a
  routine bump.
- **`cargo deny`** runs in CI (`deny` job, two legs) and weekly
  (`audit.yaml`). The `bans licenses sources` leg blocks: non-crates.io
  sources, wildcard versions and licenses outside the allowlist fail it.
  The `advisories` leg **only warns**: a vulnerable, unsound or yanked crate
  shows as a failed `deny (advisories)` check, but the CI run still passes, so
  a new RustSec advisory doesn't block unrelated PRs. The weekly `audit.yaml`
  run fails on advisories and is the backstop. Never make
  `deny (advisories)` a required status check. To fix an advisory, try
  `cargo update -p <crate>` first, and `--precise <version>` if that does
  nothing. Otherwise add an `ignore` entry in `deny.toml` with a reason.
  Duplicate crate versions only warn.

## Container Image

```bash
docker build -t mcp-k8s .
```

Static musl binary in `scratch` container. Runs `/mcp-k8s --http` on
port 8080. Installs `rustls::crypto::ring` provider at startup. The OTLP
exporter's reqwest uses `rustls-no-provider`, so it shares that provider
instead of adding aws-lc-rs (reqwest's `rustls` feature would). A missing
or duplicate process-level provider panics at runtime.

## Releasing

### Release policy and downstream version locking

Downstream consumers must pin a **release**, not `main`, so every change they
need has to ship in one.

- **Ship a GitHub Release for every change downstream needs.** Releases stalled
  after `v0.3.1` (2026-08-05) while `main` kept changing. deckwatch had to track
  `branch = "main"`, so any merge here could change deckwatch on its next
  `cargo update`.
- **The version must equal the tag.** Before tagging, a PR sets the same
  version in `Cargo.toml` `version`, `helm/mcp-k8s/Chart.yaml` `version` and
  `appVersion`, and adds a `## [X.Y.Z] - YYYY-MM-DD` section to
  `CHANGELOG.md` (moving the `[Unreleased]` entries). The `verify-version` job
  in `release.yaml` refuses to publish if any of these don't match the tag.
- **Semver within 0.x:** breaking library API or behaviour changes bump the
  minor version.
- **Downstream pins the tag**, e.g. deckwatch:
  `mcp-k8s = { git = "https://github.com/alexconrey/mcp-k8s.git", tag = "vX.Y.Z", default-features = false }`.
- **Only the maintainer creates tags and releases.**
- **Cadence:** release on demand, and at least monthly while `main` has
  unreleased changes, so fixes don't sit in `main` (releases stalled for
  almost four months after v0.3.1).
- **Tag a merge commit of `main` after its CI passed.** The `ci-passed` job in
  `release.yaml` checks that commit's CI run on `main` (waiting up to 45
  minutes if it's still running) and refuses to publish otherwise.

Cutting a new release (maintainer):

```bash
# after the version-bump PR has merged
git tag v0.X.Y
git push origin v0.X.Y
```

This triggers three workflows:
1. **Release** (`release.yaml`) — after `verify-version` and `ci-passed`,
   builds and pushes the Docker image, packages and pushes the Helm chart to
   the GHCR OCI registry, and creates a GitHub Release. The release body is
   the version's CHANGELOG section plus artifact links and a quick start,
   after GitHub's generated PR list
2. **Docs** (`docs.yaml`) — builds mdBook and deploys to GitHub Pages under
   `/vX.Y.Z/` and updates `/latest/`
3. **Pages build** — GitHub deploys the updated `gh-pages` branch

Artifacts produced per release:
- Docker image: `ghcr.io/alexconrey/mcp-k8s:vX.Y.Z`, and the release moves
  `:vX.Y` and `:latest`. Pushes to `main` publish only `:main` and `:<sha>`,
  so `:latest` is always the newest release.
- Helm chart: `oci://ghcr.io/alexconrey/charts/mcp-k8s:X.Y.Z`
- Versioned docs: `https://alexconrey.github.io/mcp-k8s/vX.Y.Z/`
- GitHub Release with release notes: `https://github.com/alexconrey/mcp-k8s/releases/tag/vX.Y.Z`

The Helm chart's `image.tag` defaults to `v<appVersion>`, so a chart
installs the image released with it. Don't reintroduce a `latest` default:
through v0.5.1 it made every chart install pull whatever `main` built last.

## Helm Chart

Published to GHCR OCI registry on tag push:
```bash
helm install mcp-k8s oci://ghcr.io/alexconrey/charts/mcp-k8s --version <X.Y.Z>  # latest release
```

Hardening defaults and rules to keep when editing the chart or manifests:

- **securityContext**: `podSecurityContext`/`securityContext` values default
  to non-root 65534, `readOnlyRootFilesystem`, no privilege escalation, drop
  ALL capabilities, seccomp `RuntimeDefault` (the Pod Security "restricted"
  profile). The scratch image writes nothing to disk; keep it that way.
  `automountServiceAccountToken` stays true (the server needs its token).
- **RBAC** is generated by `templates/_rbac.tpl`: per-resource rules scoped to
  what the tools need, no wildcards (`rbac.extraRules` may add them only with
  `rbac.allowWildcards`). `rbac.readOnly` grants get/list/watch only and sets
  `DISABLE_CREATE/UPDATE/DELETE` so `tools/list` matches; it's the documented
  starting point. `rbac.secrets: false` drops Secrets access. When a tool
  starts calling a new resource, verb or subresource, update `_rbac.tpl`,
  regenerate `helm/charts/mcp-k8s/base/clusterrole.yaml` and
  `read-only/clusterrole-patch.yaml` from `helm template`, and update
  `docs/src/deployment/rbac.md` (also generated from the chart) in the same PR.
- **NetworkPolicy** (`networkPolicy.enabled`, off by default): ingress only to
  the MCP port from configured sources; egress only to the API server, DNS
  and the OTLP endpoint from `otel.endpoint`.
- Validate with `helm lint helm/mcp-k8s`, `helm template` (default,
  `rbac.readOnly=true`, `networkPolicy.enabled=true`) and
  `kustomize build helm/charts/mcp-k8s{,/read-only,/network-policy}`. Runtime
  checks only against a throwaway cluster you create (e.g. k3s in Docker with
  its own kubeconfig), never the current kube context.

## Documentation

Published to GitHub Pages at https://alexconrey.github.io/mcp-k8s/

Built with mdBook, versioned per tag (`/latest/`, `/v0.1.0/`).

## Origin

Extracted from deckwatch's MCP server (`src/handlers/mcp.rs`) and K8s type
layer (`src/kube_ext.rs`). Deckwatch can depend on this crate for the shared
K8s tool implementations while keeping its own database-backed tools
(gitops, applications, plugins, templates) in-tree.
