# mcp-k8s

A Kubernetes MCP (Model Context Protocol) server that exposes **202 tools**
covering all GA Kubernetes API resources, CRDs, multi-cluster management, and
resource watching. Works as a stdio server for Claude Code or as an HTTP
endpoint for in-cluster deployment.

## Features

- **202 MCP tools** across 49 resource modules covering every GA Kubernetes API group
- **Two runtime modes**: stdio (for Claude Code) and HTTP (for in-cluster deployment)
- **SSE transport**: `POST /mcp/sse` endpoint for Server-Sent Events streaming
- **Multi-cluster support**: load multiple kubeconfig contexts and switch between clusters at runtime
- **CRD discovery**: full CRUD lifecycle for any installed CRD via dynamic API discovery
- **Watch/subscribe**: `watch_resource` tool observes resource changes over a time window
- **MCP resources and prompts**: `resources/read` for direct `k8s://` URI access, built-in diagnostic prompts
- **Prometheus metrics**: `GET /metrics` endpoint with request counts and tool call latency histograms
- **Bearer token auth and TLS**: `--auth-token` for HTTP authentication, `--tls-cert`/`--tls-key` for HTTPS
- **Action permission controls**: disable create/update/delete globally or per-resource via CLI flags and env vars
- **Namespace allowlisting**: restrict operations to specific namespaces
- **Token-efficient responses**: focused summary/detail types instead of raw K8s API objects
- **Structured JSON logging**: `--log-format json` for production log aggregation
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
| **Generic** | `apply_manifest` (server-side apply), `get_resource_yaml` (raw JSON) |
| **Metrics** | Pod and node CPU/memory from metrics-server |
| **CRDs** | `list_crds`, `get_crd`, `list_custom_resources`, `get_custom_resource`, `create_custom_resource`, `update_custom_resource`, `delete_custom_resource` |
| **Watch** | `watch_resource` (observe ADDED/MODIFIED/DELETED events over a time window) |
| **Cluster Mgmt** | `list_clusters`, `switch_cluster`, `get_active_cluster` (multi-context switching) |

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
| `--disable-secret-decode` | `DISABLE_SECRET_DECODE` | `false` | Prevent secret value decoding |
| `--log-format` | `LOG_FORMAT` | `text` | Log format: `text` or `json` |
| — | `KUBECONFIG` | in-cluster | Kubernetes config path |

### Read-only mode

```bash
mcp-k8s --disable-create --disable-update --disable-delete
```

Disabled tools are hidden from `tools/list` so the LLM won't attempt to call them.

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

The `mcp_k8s` crate can be embedded in other applications:

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

## Testing

```bash
cargo test
```

325 unit tests covering type extraction, serialization, tool definitions,
and permission filtering.

## License

MIT
