# mcp-k8s

Kubernetes MCP (Model Context Protocol) server and library. Exposes
210 Kubernetes cluster operations as MCP tools consumable by Claude Code
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
├── otel.rs             # Binary only (`mod otel` in main.rs): OTLP pipeline, W3C trace context
├── cache.rs            # Optional in-memory response cache with TTL
├── client.rs           # K8sClient — kube::Client wrapper with namespace allowlist + permissions
├── cluster.rs          # ClusterManager — multi-cluster support with named contexts
├── error.rs            # Error types (NamespaceNotAllowed, Kube, BadRequest, ActionNotAllowed)
├── permissions.rs      # ActionPermissions — global/per-resource action controls + secret decode gating
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
│   └── tests.rs        # MCP dispatch and permission filtering tests
└── resources/          # 49 resource modules
    ├── mod.rs           # Aggregates all modules
    ├── admission.rs     # MutatingWebhookConfig, ValidatingWebhookConfig, ValidatingAdmissionPolicy
    ├── admission_alpha.rs # MutatingAdmissionPolicy/Binding (v1alpha1)
    ├── auth.rs          # can_i, whoami, list_my_permissions
    ├── cluster_mgmt.rs  # list_clusters, switch_cluster, get_active_cluster
    ├── cluster_trust_bundle.rs # ClusterTrustBundle (v1beta1)
    ├── clusterrole.rs   # ClusterRole CRUD
    ├── clusterrolebinding.rs  # ClusterRoleBinding CRUD
    ├── configmap.rs     # ConfigMap CRUD
    ├── crd.rs           # CRD discovery + custom resource CRUD (7 tools)
    ├── cronjob.rs       # CronJob CRUD
    ├── csr.rs           # CertificateSigningRequest (list, get, approve, deny)
    ├── daemonset.rs     # DaemonSet CRUD
    ├── deployment.rs    # Deployment create/update/delete/restart/scale/rollback
    ├── device_resources.rs # ResourceClaim, ResourceSlice, DeviceClass (v1beta2)
    ├── endpoints.rs     # Endpoints (read-only, legacy)
    ├── endpointslice.rs # EndpointSlice (read-only)
    ├── flowcontrol.rs   # FlowSchema + PriorityLevelConfiguration (read-only)
    ├── generic.rs       # apply_manifest (server-side apply, last resort), get_resource_json (+ deprecated get_resource_yaml alias)
    ├── hpa.rs           # HorizontalPodAutoscaler CRUD
    ├── ingress.rs       # Ingress get + delete
    ├── ingressclass.rs  # IngressClass (read-only, cluster-scoped)
    ├── ip_networking.rs # IPAddress, ServiceCIDR (v1, GA)
    ├── job.rs           # Job list/get/create/delete
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
    ├── service.rs       # Service list/get/update/delete
    ├── serviceaccount.rs # ServiceAccount list/get/create/delete
    ├── statefulset.rs   # StatefulSet CRUD
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
`secret_decode_enabled` for gating secret value access.

**ClusterManager** (`cluster.rs`): Holds multiple named `K8sClient` instances
behind `Arc<RwLock<HashMap>>` with an active cluster tracker. Supports
`--contexts` CLI flag for loading multiple kubeconfig contexts. Tools
`list_clusters`, `switch_cluster`, `get_active_cluster` manage the active
context at runtime.

**Permissions** (`permissions.rs`): `ActionPermissions` struct with global
create/update/delete disable flags, per-resource overrides, and
`secret_decode_enabled`. Disabled tools are filtered from `tools/list`
and rejected in `tools/call`.

**Types and extraction** (`types.rs`, `extract.rs`): Core summary/detail
structs. `types.rs` re-exports all per-resource module types for a unified
import path.

**Response cache** (`cache.rs`): Optional in-memory cache with configurable
TTL for read operations. Disabled by default (`--cache-ttl 0`). When enabled,
caches list operation responses to reduce K8s API load on large clusters.

**Resource modules** (`resources/*.rs`): 49 modules, each with
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
server. There is no separate errors counter. These stay unchanged until a
release deprecates them in favour of the OTel metric.

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
| `--tls-cert` | `TLS_CERT` | (none) | TLS certificate PEM path |
| `--tls-key` | `TLS_KEY` | (none) | TLS private key PEM path |
| `--disable-apply-manifest` | `DISABLE_APPLY_MANIFEST` | `false` | Block the `apply_manifest` tool entirely |
| `--disable-secret-decode` | `DISABLE_SECRET_DECODE` | `false` | Prevent secret value decoding |
| `--cache-ttl` | `CACHE_TTL` | `0` | Response cache TTL in seconds (0 = disabled) |
| `--log-format` | `LOG_FORMAT` | `text` | Log format: `text` or `json` (stderr in stdio mode) |
| — | `KUBECONFIG` | in-cluster | K8s config path |
| — | `RUST_LOG` | `info` | Console log level |
| — | `OTEL_EXPORTER_OTLP_ENDPOINT` | (none: off) | OTLP http/protobuf collector; other standard `OTEL_*` variables apply (protocol, headers, service name, resource attributes, sampler, `*_EXPORTER=none`, `OTEL_SDK_DISABLED`) |
| — | `MCP_K8S_OTEL_FILTER` | `info` | `EnvFilter` for spans and logs exported over OTLP |

## Security posture

mcp-k8s is a standalone server with broad cluster powers, so treat server
weaknesses as security issues. The current defaults are permissive:

- `--listen` defaults to `0.0.0.0:8080`, and `--auth-token` is optional.
  Without a token the HTTP API is unauthenticated.
- The router uses `CorsLayer::permissive()` and doesn't validate `Origin`.
- Secret decoding and `apply_manifest` are on by default
  (`ActionPermissions::default()`, which deckwatch also uses).
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
All output uses `serde_json::to_string_pretty`.

## Project Layout

```
mcp-k8s/
├── src/                    # Rust source (lib + bin)
├── tests/
│   ├── integration.rs      # Tower-test mock integration tests (8 tests)
│   └── k3s_integration.rs  # Testcontainers k3s tests (13 tests, #[ignore]; k3s v1.35, MCP_K8S_K3S_TAG overrides)
├── docs/                   # mdBook documentation (GitHub Pages)
│   ├── book.toml
│   ├── src/                # 21 documentation pages
│   └── theme/              # Version switcher (JS + CSS)
├── helm/
│   ├── mcp-k8s/            # Helm chart (with PDB, HPA, NetworkPolicy, Ingress)
│   └── charts/mcp-k8s/     # Raw K8s manifests + Kustomize
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
cargo test                  # 451 unit + 11 binary (router, telemetry) + 8 integration tests
cargo test --no-default-features  # Same tests without `server` (what deckwatch compiles)
cargo test --test k3s_integration -- --ignored  # k3s integration tests (needs Docker)
cargo clippy --all-targets -- -D warnings  # Lint
cargo clippy --lib --no-default-features --features k8s-openapi/v1_33 -- -D warnings  # Library-only build (what deckwatch compiles)
cargo fmt --check           # Format check
make check                  # cargo fmt (rewrites) + both clippy and both test configs.
                            # Not the k8s-openapi feature check or k3s tests, so not quite CI
make deny                   # cargo deny check (advisories, licenses, bans, sources)
```

### What CI does and doesn't cover

`ci.yaml` runs the binary-file check, `fmt --check`, both clippy
configurations, a check that the library's k8s-openapi dependency enables no
version feature, `build --release`, and `cargo test` with and without default
features on every PR. A separate `deny` job runs `cargo deny` with `deny.toml`
(advisories only warn; see [Dependencies and advisories](#dependencies-and-advisories)). Gaps to cover yourself:

- **The k3s integration tests are `#[ignore]`d and never run in CI.** Run
  `cargo test --test k3s_integration -- --ignored` (needs Docker) when you
  change kube API paths, API versions or the namespace allowlist. Plain
  `cargo test -- --ignored` also runs the `cluster.rs` tests, which load your
  kubeconfig.
- **The Dockerfile is only built on pushes to `main`**, not on PRs. Run
  `docker build .` when you change it or bump the toolchain. CI uses floating
  `stable`, but the Dockerfile pins `rust:1.99` (kube 5 needs at least 1.95).
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

Cutting a new release (maintainer):

```bash
# after the version-bump PR has merged
git tag v0.X.Y
git push origin v0.X.Y
```

This triggers three workflows:
1. **Release** (`release.yaml`) — builds Docker image, packages + pushes Helm
   chart to GHCR OCI registry, creates a GitHub Release with auto-generated
   release notes (commit log, artifact links, quick start, tool/test counts)
2. **Docs** (`docs.yaml`) — builds mdBook and deploys to GitHub Pages under
   `/vX.Y.Z/` and updates `/latest/`
3. **Pages build** — GitHub deploys the updated `gh-pages` branch

Artifacts produced per release:
- Docker image: `ghcr.io/alexconrey/mcp-k8s:vX.Y.Z` (the release also moves
  `:latest`; pushes to `main` publish `:latest` and `:<sha>` too, so `:latest`
  is not a release)
- Helm chart: `oci://ghcr.io/alexconrey/charts/mcp-k8s:X.Y.Z`
- Versioned docs: `https://alexconrey.github.io/mcp-k8s/vX.Y.Z/`
- GitHub Release with release notes: `https://github.com/alexconrey/mcp-k8s/releases/tag/vX.Y.Z`

The release workflow uses `softprops/action-gh-release@v3` with
`generate_release_notes: true` for GitHub's auto-generated changelog
plus a custom body with commit log, artifact table, and quick start.

## Helm Chart

Published to GHCR OCI registry on tag push:
```bash
helm install mcp-k8s oci://ghcr.io/alexconrey/charts/mcp-k8s --version <X.Y.Z>  # latest release
```

## Documentation

Published to GitHub Pages at https://alexconrey.github.io/mcp-k8s/

Built with mdBook, versioned per tag (`/latest/`, `/v0.1.0/`).

## Origin

Extracted from deckwatch's MCP server (`src/handlers/mcp.rs`) and K8s type
layer (`src/kube_ext.rs`). Deckwatch can depend on this crate for the shared
K8s tool implementations while keeping its own database-backed tools
(gitops, applications, plugins, templates) in-tree.
