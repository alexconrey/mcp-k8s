# Configuration

mcp-k8s is configured through CLI arguments and environment variables. Every flag has a corresponding environment variable, so the same configuration works for both local stdio mode and containerized HTTP deployments.

## Runtime Mode

| Flag | Default | Description |
|------|---------|-------------|
| `--http` | *(off)* | Run in HTTP server mode. Without this flag, mcp-k8s runs in stdio mode (newline-delimited JSON-RPC on stdin/stdout). |

**Stdio mode** (default) is designed for Claude Code's MCP server configuration. The server reads JSON-RPC requests from stdin and writes responses to stdout.

**HTTP mode** is designed for in-cluster Kubernetes deployments. It starts an Axum-based HTTP server with the following endpoints:

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/mcp` | POST | MCP JSON-RPC 2.0 endpoint |
| `/healthz` | GET | Health check (returns `ok`) |
| `/swagger-ui` | GET | OpenAPI / Swagger UI |
| `/openapi.json` | GET | OpenAPI JSON spec |

## Network Configuration

| Flag | Env Var | Default | Description |
|------|---------|---------|-------------|
| `--listen` | `MCP_K8S_LISTEN` | `0.0.0.0:8080` | HTTP listen address (only used with `--http`) |

## Namespace Filtering

| Flag | Env Var | Default | Description |
|------|---------|---------|-------------|
| `--namespaces` | `MCP_K8S_NAMESPACES` | *(empty -- all allowed)* | Comma-separated list of allowed namespaces. When set, all namespace-scoped tools are restricted to this allowlist. Tools that attempt to access a disallowed namespace receive an error. |

Example:

```bash
# CLI
mcp-k8s --namespaces default,production,staging

# Environment variable
MCP_K8S_NAMESPACES=default,production,staging mcp-k8s
```

## Permission Controls

mcp-k8s provides global and per-resource flags to restrict which CRUD actions are available. Read operations are always allowed and cannot be disabled.

### Global Flags

| Flag | Env Var | Default | Description |
|------|---------|---------|-------------|
| `--disable-create` | `DISABLE_CREATE` | `false` | Globally disable all create tools. When set, tools like `create_deployment`, `create_pod`, `create_service`, and `apply_manifest` are removed from the tool list. |
| `--disable-update` | `DISABLE_UPDATE` | `false` | Globally disable all update tools. When set, tools like `update_deployment`, `scale_deployment`, `restart_deployment`, `cordon_node`, and `drain_node` are removed. |
| `--disable-delete` | `DISABLE_DELETE` | `false` | Globally disable all delete tools. When set, tools like `delete_deployment`, `delete_pod`, and `evict_pod` are removed. |

### Per-Resource Flags

| Flag | Env Var | Default | Description |
|------|---------|---------|-------------|
| `--disable` | `MCP_K8S_DISABLE` | *(empty)* | Comma-separated list of `resource-action` pairs to disable. Overrides are case-insensitive. |

Per-resource overrides take precedence over global flags. The format is `<resource>-<action>` where resource is the Kubernetes resource name (singular, lowercase) and action is `create`, `update`, or `delete`.

Examples:

```bash
# Disable deleting deployments and creating pods
mcp-k8s --disable deployment-delete,pod-create

# Via environment variable
MCP_K8S_DISABLE=deployment-delete,pod-create mcp-k8s
```

See the [Permissions](../permissions.md) page for full details on the permission resolution logic and example configurations.

## Kubernetes Authentication

| Env Var | Description |
|---------|-------------|
| `KUBECONFIG` | Path to kubeconfig file. If unset, mcp-k8s uses the default kubeconfig resolution (typically `~/.kube/config`). When running in-cluster, it automatically uses the pod's service account token. |

mcp-k8s uses the standard [kube-rs](https://github.com/kube-rs/kube) client configuration, which supports:

- In-cluster service account token authentication
- Kubeconfig file with multiple contexts
- OIDC, exec-based, and token-based authentication plugins

## Logging

| Env Var | Default | Description |
|---------|---------|-------------|
| `RUST_LOG` | `info` | Controls log verbosity using the `tracing` crate's `EnvFilter` syntax. |

Examples:

```bash
# Debug logging for mcp-k8s only
RUST_LOG=mcp_k8s=debug mcp-k8s

# Trace logging for everything
RUST_LOG=trace mcp-k8s

# Info for mcp-k8s, warn for dependencies
RUST_LOG=mcp_k8s=info,kube=warn mcp-k8s
```

## Complete Example

```bash
mcp-k8s \
  --http \
  --listen 0.0.0.0:9090 \
  --namespaces default,production \
  --disable-delete \
  --disable deployment-create,secret-create
```

This starts mcp-k8s as an HTTP server on port 9090, restricted to the `default` and `production` namespaces, with all delete operations disabled globally and deployment/secret creation disabled individually.
