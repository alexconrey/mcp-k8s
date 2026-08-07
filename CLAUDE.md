# mcp-k8s

Kubernetes MCP (Model Context Protocol) server and library. Exposes
202 Kubernetes cluster operations as MCP tools consumable by Claude Code
and other MCP clients. Supports multi-cluster management, CRD discovery,
watch/subscribe, and all GA + alpha/beta K8s API groups.

## Architecture

Rust project with a library crate (`mcp_k8s`) and a binary crate (`mcp-k8s`).

### Source Layout

```
src/
├── lib.rs              # Public API re-exports
├── main.rs             # Binary: stdio/HTTP/HTTPS modes, auth middleware, metrics
├── cache.rs            # Optional in-memory response cache with TTL
├── client.rs           # K8sClient — kube::Client wrapper with namespace allowlist + permissions
├── cluster.rs          # ClusterManager — multi-cluster support with named contexts
├── error.rs            # Error types (NamespaceNotAllowed, Kube, BadRequest, ActionNotAllowed)
├── permissions.rs      # ActionPermissions — global/per-resource action controls + secret decode gating
├── types.rs            # Core summary/detail structs + re-exports from resource modules
├── extract.rs          # K8s API objects → summary/detail extraction functions
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
    ├── cluster_trust_bundle.rs # ClusterTrustBundle (v1alpha1)
    ├── clusterrole.rs   # ClusterRole CRUD
    ├── clusterrolebinding.rs  # ClusterRoleBinding CRUD
    ├── configmap.rs     # ConfigMap CRUD
    ├── crd.rs           # CRD discovery + custom resource CRUD (7 tools)
    ├── cronjob.rs       # CronJob CRUD
    ├── csr.rs           # CertificateSigningRequest (list, get, approve, deny)
    ├── daemonset.rs     # DaemonSet CRUD
    ├── deployment.rs    # Deployment create/update/delete/restart/scale/rollback
    ├── device_resources.rs # ResourceClaim, ResourceSlice, DeviceClass (v1beta1)
    ├── endpoints.rs     # Endpoints (read-only, legacy)
    ├── endpointslice.rs # EndpointSlice (read-only)
    ├── flowcontrol.rs   # FlowSchema + PriorityLevelConfiguration (read-only)
    ├── generic.rs       # apply_manifest (server-side apply), get_resource_yaml
    ├── hpa.rs           # HorizontalPodAutoscaler CRUD
    ├── ingress.rs       # Ingress get + delete
    ├── ingressclass.rs  # IngressClass (read-only, cluster-scoped)
    ├── ip_networking.rs # IPAddress, ServiceCIDR (v1beta1)
    ├── job.rs           # Job list/get/create/delete
    ├── lease.rs         # Lease (read-only)
    ├── lease_candidate.rs # LeaseCandidate (v1alpha2)
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

**Observability**: Prometheus metrics (`mcp_tool_calls_total`,
`mcp_tool_call_duration_seconds`, `mcp_tool_call_errors_total`,
`mcp_requests_total`) at `/metrics`. Structured JSON logging via
`--log-format json`. UUID trace IDs per request.

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
| `--log-format` | `LOG_FORMAT` | `text` | Log format: `text` or `json` |
| — | `KUBECONFIG` | in-cluster | K8s config path |
| — | `RUST_LOG` | `info` | Log level |

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
│   └── k3s_integration.rs  # Testcontainers k3s tests (10 tests, #[ignore])
├── docs/                   # mdBook documentation (GitHub Pages)
│   ├── book.toml
│   ├── src/                # 20 documentation pages
│   └── theme/              # Version switcher (JS + CSS)
├── helm/
│   ├── mcp-k8s/            # Helm chart (with PDB, HPA, NetworkPolicy, Ingress)
│   └── charts/mcp-k8s/     # Raw K8s manifests + Kustomize
├── .github/workflows/
│   ├── ci.yaml             # Build, test, clippy, fmt on PR
│   ├── release.yaml        # Docker + Helm OCI + GitHub Release on tag
│   └── docs.yaml           # Versioned mdBook deploy to GitHub Pages
├── Dockerfile              # Static musl binary in scratch container
├── Makefile                # 17 targets (build, test, fmt, clippy, docker, deploy, etc.)
├── deny.toml               # cargo-deny license/vulnerability audit config
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
gh pr merge --squash --delete-branch
```

Do **not** push directly to `main`. Even one-liner changes need a PR so the
CI workflow (build, test, clippy, fmt) runs against them before merge.

## Building and Testing

```bash
cargo build --release       # Build
cargo test                  # 398 unit + 8 integration tests
cargo test -- --ignored     # Run k3s integration tests (needs Docker)
cargo clippy --all-targets -- -D warnings  # Lint
cargo fmt --check           # Format check
make check                  # All of the above
```

## Container Image

```bash
docker build -t mcp-k8s .
```

Static musl binary in `scratch` container. Runs `/mcp-k8s --http` on
port 8080. Installs `rustls::crypto::ring` provider at startup.

## Releasing

Cutting a new release:

```bash
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
- Docker image: `ghcr.io/alexconrey/mcp-k8s:vX.Y.Z`
- Helm chart: `oci://ghcr.io/alexconrey/charts/mcp-k8s:X.Y.Z`
- Versioned docs: `https://alexconrey.github.io/mcp-k8s/vX.Y.Z/`
- GitHub Release with release notes: `https://github.com/alexconrey/mcp-k8s/releases/tag/vX.Y.Z`

The release workflow uses `softprops/action-gh-release@v2` with
`generate_release_notes: true` for GitHub's auto-generated changelog
plus a custom body with commit log, artifact table, and quick start.

## Helm Chart

Published to GHCR OCI registry on tag push:
```bash
helm install mcp-k8s oci://ghcr.io/alexconrey/charts/mcp-k8s --version 0.1.0
```

## Documentation

Published to GitHub Pages at https://alexconrey.github.io/mcp-k8s/

Built with mdBook, versioned per tag (`/latest/`, `/v0.1.0/`).

## Origin

Extracted from deckwatch's MCP server (`src/handlers/mcp.rs`) and K8s type
layer (`src/kube_ext.rs`). Deckwatch can depend on this crate for the shared
K8s tool implementations while keeping its own database-backed tools
(gitops, applications, addons, templates) in-tree.
