# mcp-k8s

Kubernetes MCP (Model Context Protocol) server and library. Exposes
165+ Kubernetes cluster operations as MCP tools consumable by Claude Code
and other MCP clients.

## Architecture

Rust project with a library crate (`mcp_k8s`) and a binary crate (`mcp-k8s`).

### Source Layout

```
src/
├── lib.rs              # Public API re-exports
├── main.rs             # Binary entry point (stdio and HTTP modes)
├── client.rs           # K8sClient — kube::Client wrapper with namespace allowlist + permissions
├── error.rs            # Error types (NamespaceNotAllowed, Kube, BadRequest, ActionNotAllowed)
├── permissions.rs      # ActionPermissions — global and per-resource action controls
├── types.rs            # Core summary/detail structs + re-exports from resource modules
├── extract.rs          # K8s API objects → summary/detail extraction functions
├── mcp/
│   ├── mod.rs          # Module re-exports
│   ├── protocol.rs     # JSON-RPC 2.0 request/response types and helpers (with utoipa schemas)
│   ├── definitions.rs  # Core MCP tool inputSchema definitions (filtered by permissions)
│   ├── handlers.rs     # Core tool implementations and dispatch (with permission checks)
│   └── tests.rs        # MCP dispatch and permission filtering tests
└── resources/
    ├── mod.rs           # Aggregates all resource modules (tool_definitions + handle_tool)
    ├── admission.rs     # MutatingWebhookConfig, ValidatingWebhookConfig, ValidatingAdmissionPolicy
    ├── auth.rs          # can_i, whoami, list_my_permissions
    ├── clusterrole.rs   # ClusterRole CRUD
    ├── clusterrolebinding.rs  # ClusterRoleBinding CRUD
    ├── configmap.rs     # ConfigMap CRUD
    ├── cronjob.rs       # CronJob CRUD
    ├── csr.rs           # CertificateSigningRequest (list, get, approve, deny)
    ├── daemonset.rs     # DaemonSet CRUD
    ├── deployment.rs    # Deployment create/update/delete/restart/scale/rollback
    ├── endpoints.rs     # Endpoints (read-only, legacy)
    ├── endpointslice.rs # EndpointSlice (read-only)
    ├── flowcontrol.rs   # FlowSchema + PriorityLevelConfiguration (read-only)
    ├── generic.rs       # apply_manifest (server-side apply), get_resource_yaml
    ├── hpa.rs           # HorizontalPodAutoscaler CRUD
    ├── ingress.rs       # Ingress get + delete (create/update in core handlers)
    ├── ingressclass.rs  # IngressClass (read-only, cluster-scoped)
    ├── job.rs           # Job list/get/create/delete
    ├── lease.rs         # Lease (read-only)
    ├── limitrange.rs    # LimitRange CRUD
    ├── namespace.rs     # Namespace get/create/update/delete
    ├── networkpolicy.rs # NetworkPolicy CRUD
    ├── node.rs          # Node list/get + metrics/cordon/uncordon/drain
    ├── pdb.rs           # PodDisruptionBudget CRUD
    ├── pod.rs           # Pod list/get/create/delete/evict/exec
    ├── priorityclass.rs # PriorityClass (read-only, cluster-scoped)
    ├── pv.rs            # PersistentVolume list/get/delete (cluster-scoped)
    ├── pvc.rs           # PersistentVolumeClaim CRUD
    ├── replicaset.rs    # ReplicaSet list/get (read-only)
    ├── resourcequota.rs # ResourceQuota CRUD
    ├── role.rs          # Role CRUD
    ├── rolebinding.rs   # RoleBinding CRUD
    ├── runtimeclass.rs  # RuntimeClass (read-only, cluster-scoped)
    ├── secret.rs        # Secret CRUD (values redacted by default, decode: true to reveal)
    ├── service.rs       # Service list/get/update/delete (create in core handlers)
    ├── serviceaccount.rs # ServiceAccount list/get/create/delete
    ├── statefulset.rs   # StatefulSet CRUD
    ├── storageclass.rs  # StorageClass CRUD (cluster-scoped)
    └── storage_internals.rs  # CSIDriver, CSINode, VolumeAttachment, CSIStorageCapacity
```

### Key Components

**K8sClient** (`client.rs`): Wraps `kube::Client` with a namespace allowlist
and `ActionPermissions`. Provides namespace-checked API factory methods
(`deployments_api`, `pods_api`, `ingresses_api`, etc.) that return
`kube::Api<T>` instances. The `permissions()` getter exposes the embedded
permission controls to handlers.

**Permissions** (`permissions.rs`): `ActionPermissions` struct with global
create/update/delete disable flags and per-resource overrides. `action_for_tool()`
maps tool names to `Action` enum variants (Read/Create/Update/Delete).
`is_tool_allowed()` checks if a specific tool is permitted. Disabled tools
are filtered from `tools/list` responses and rejected in `tools/call`.

**Types and extraction** (`types.rs`, `extract.rs`): Core summary/detail
structs for deployments, pods, ingresses, events, nodes, cronjobs. The
`types.rs` module also re-exports all per-resource module types for a
unified import path. The extraction layer produces token-efficient MCP
responses instead of raw K8s objects.

**Resource modules** (`resources/*.rs`): Each resource type has its own module
with `tool_definitions()` and `handle_tool()` functions. The `resources/mod.rs`
aggregates all modules. Each module handles its own namespace checking,
K8s API calls, type extraction, and response formatting.

**MCP protocol** (`mcp/protocol.rs`): JSON-RPC 2.0 types with utoipa
`ToSchema` derives for Swagger UI integration. Implements MCP spec `2025-11-25`.

**Tool dispatch** (`mcp/handlers.rs`): Checks permissions, delegates to
resource modules first, then falls back to core handler implementations.
Returns `Some(Result<String, String>)` for recognized tools, `None` for
unknown (so consumers can layer additional tools on top).

## Runtime Modes

### Stdio (default)

```
mcp-k8s [--namespaces ns1,ns2] [--disable-create] [--disable-update] [--disable-delete]
```

Reads newline-delimited JSON-RPC from stdin, writes responses to stdout.
Designed for Claude Code's MCP server configuration.

### HTTP server

```
mcp-k8s --http [--listen 0.0.0.0:8080] [--namespaces ns1,ns2]
```

Axum-based HTTP server with:
- `POST /mcp` — MCP JSON-RPC endpoint
- `GET /healthz` — health check (liveness/readiness)
- `GET /swagger-ui` — interactive API documentation
- `GET /openapi.json` — OpenAPI spec

Designed for in-cluster Kubernetes deployment.

## Configuration

| Flag | Env Var | Default | Description |
|------|---------|---------|-------------|
| `--http` | — | off | Run as HTTP server |
| `--listen` | `MCP_K8S_LISTEN` | `0.0.0.0:8080` | HTTP listen address |
| `--namespaces` | `MCP_K8S_NAMESPACES` | (all) | Namespace allowlist |
| `--disable-create` | `DISABLE_CREATE` | `false` | Disable all create ops |
| `--disable-update` | `DISABLE_UPDATE` | `false` | Disable all update ops |
| `--disable-delete` | `DISABLE_DELETE` | `false` | Disable all delete ops |
| `--disable` | `MCP_K8S_DISABLE` | (none) | Per-resource disables (e.g. `deployment-delete`) |
| — | `KUBECONFIG` | in-cluster | K8s config path |
| — | `RUST_LOG` | `info` | Log level |

## Resource Module Pattern

Each resource module in `src/resources/` follows this pattern:

```rust
// Namespace-checked API factory (or kube::Api::all for cluster-scoped)
fn api(client: &K8sClient, ns: &str) -> Result<kube::Api<T>, String> {
    if !client.is_namespace_allowed(ns) {
        return Err(format!("Namespace '{ns}' is not in the allowed list"));
    }
    Ok(kube::Api::namespaced(client.inner().clone(), ns))
}

// Summary struct for list responses
#[derive(Serialize, Debug)]
pub struct ResourceSummary { ... }

// Extraction function
fn extract_summary(resource: &T) -> ResourceSummary { ... }

// Tool definitions (JSON schemas)
pub fn tool_definitions() -> Vec<serde_json::Value> { ... }

// Handler dispatch
pub async fn handle_tool(
    client: &K8sClient, name: &str, args: &serde_json::Value
) -> Option<Result<String, String>> { ... }

// Tests
#[cfg(test)] mod tests { ... }
```

List operations support `label_selector` and (where applicable) `field_selector`
parameters. Write operations add `app.kubernetes.io/managed-by: mcp-k8s` labels.
All output uses `serde_json::to_string_pretty`.

## Project Layout

```
mcp-k8s/
├── src/                    # Rust source (lib + bin)
├── docs/                   # mdBook documentation (published to GitHub Pages)
│   ├── book.toml
│   ├── src/                # Documentation content
│   └── theme/              # Version switcher theme
├── helm/
│   ├── mcp-k8s/            # Helm chart
│   └── charts/mcp-k8s/     # Raw K8s manifests + Kustomize
├── .github/workflows/      # CI (build/test/lint), release (docker), docs (GitHub Pages)
├── Dockerfile              # Distroless container image
├── Cargo.toml
└── README.md
```

## Building and Testing

```bash
cargo build --release    # Build
cargo test               # 325 tests
cargo clippy             # Lint
cargo fmt --check        # Format check
```

## Container Image

```bash
docker build -t mcp-k8s .
```

Uses `gcr.io/distroless/cc-debian12:nonroot`. Runs `/mcp-k8s --http` on
port 8080. Docs are NOT included in the image — they're published separately
to GitHub Pages.

## Origin

Extracted from deckwatch's MCP server (`src/handlers/mcp.rs`) and K8s type
layer (`src/kube_ext.rs`). Deckwatch can depend on this crate for the shared
K8s tool implementations while keeping its own database-backed tools
(gitops, applications, addons, templates) in-tree.
