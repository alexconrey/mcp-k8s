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
| `/mcp/sse` | POST | MCP JSON-RPC 2.0 via Server-Sent Events (SSE) transport |
| `/healthz` | GET | Health check (returns `ok`) |
| `/metrics` | GET | Prometheus metrics (request counts, tool call durations) |
| `/swagger-ui` | GET | OpenAPI / Swagger UI |
| `/openapi.json` | GET | OpenAPI JSON spec |

## Network Configuration

| Flag | Env Var | Default | Description |
|------|---------|---------|-------------|
| `--listen` | `MCP_K8S_LISTEN` | `0.0.0.0:8080` | HTTP listen address (only used with `--http`) |

## Multi-Cluster Support

| Flag | Env Var | Default | Description |
|------|---------|---------|-------------|
| `--contexts` | `MCP_K8S_CONTEXTS` | *(empty)* | Comma-separated list of kubeconfig context names to load. Each context becomes a named cluster that can be switched at runtime via the `list_clusters`, `switch_cluster`, and `get_active_cluster` tools. The first context becomes the active cluster on startup. When omitted, mcp-k8s uses the default kubeconfig context. |

Example:

```bash
# Load two clusters; staging is active initially
mcp-k8s --contexts staging,production

# Via environment variable
MCP_K8S_CONTEXTS=staging,production mcp-k8s
```

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
| `--disable-apply-manifest` | `DISABLE_APPLY_MANIFEST` | `false` | Completely disable the `apply_manifest` tool. Unlike `--disable-create`, this is a dedicated kill-switch for `apply_manifest` because it can both create and update arbitrary resources. See [Permissions](../permissions.md) for details. |

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

## Security

### Bearer Token Authentication

| Flag | Env Var | Default | Description |
|------|---------|---------|-------------|
| `--auth-token` | `AUTH_TOKEN` | *(none)* | When set, all HTTP requests (except `/healthz`, `/metrics`, `/swagger-ui`, and `/openapi.json`) must include an `Authorization: Bearer <token>` header. Unauthorized requests receive a 401 response. |

Example:

```bash
mcp-k8s --http --auth-token my-secret-token
```

Clients must then include the header:

```
Authorization: Bearer my-secret-token
```

### TLS / HTTPS

| Flag | Env Var | Default | Description |
|------|---------|---------|-------------|
| `--tls-cert` | `TLS_CERT` | *(none)* | Path to a TLS certificate PEM file. Must be paired with `--tls-key`. |
| `--tls-key` | `TLS_KEY` | *(none)* | Path to a TLS private key PEM file. Must be paired with `--tls-cert`. |

When both are provided, the HTTP server runs over HTTPS using rustls. If only one is provided, the server exits with an error.

Example:

```bash
mcp-k8s --http --tls-cert /etc/certs/tls.crt --tls-key /etc/certs/tls.key
```

### Secret Decoding Control

| Flag | Env Var | Default | Description |
|------|---------|---------|-------------|
| `--disable-secret-decode` | `DISABLE_SECRET_DECODE` | `false` | When set, the `get_secret` tool never returns decoded secret values, even if the caller passes `decode: true`. Use this in environments where secret values must not be exposed to MCP clients. |

## Logging

| Env Var | Default | Description |
|---------|---------|-------------|
| `RUST_LOG` | `info` | Controls log verbosity using the `tracing` crate's `EnvFilter` syntax. |

### Log Format

| Flag | Env Var | Default | Description |
|------|---------|---------|-------------|
| `--log-format` | `LOG_FORMAT` | `text` | Log output format. Set to `json` for structured JSON logging suitable for log aggregation pipelines (e.g. Loki, Datadog, CloudWatch). |

Examples:

```bash
# Debug logging for mcp-k8s only
RUST_LOG=mcp_k8s=debug mcp-k8s

# Trace logging for everything
RUST_LOG=trace mcp-k8s

# Info for mcp-k8s, warn for dependencies
RUST_LOG=mcp_k8s=info,kube=warn mcp-k8s

# Structured JSON logging for production
mcp-k8s --http --log-format json
```

## Response Cache

| Flag | Env Var | Default | Description |
|------|---------|---------|-------------|
| `--cache-ttl` | `CACHE_TTL` | `0` | Response cache TTL in seconds. When set to a value greater than 0, read (list) operation responses are cached in memory to reduce Kubernetes API load. Set to `0` to disable caching entirely. |

When enabled, the cache stores responses from list operations and serves them from memory for subsequent identical requests within the TTL window. This is useful for large clusters where repeated list calls are expensive. The cache is automatically invalidated when the TTL expires.

Example:

```bash
# Enable caching with a 30-second TTL
mcp-k8s --cache-ttl 30

# Via environment variable
CACHE_TTL=60 mcp-k8s
```

## Complete Example

```bash
mcp-k8s \
  --http \
  --listen 0.0.0.0:9090 \
  --namespaces default,production \
  --disable-delete \
  --disable-apply-manifest \
  --disable deployment-create,secret-create \
  --auth-token my-secret-token \
  --tls-cert /etc/certs/tls.crt \
  --tls-key /etc/certs/tls.key \
  --disable-secret-decode \
  --log-format json \
  --cache-ttl 30 \
  --contexts staging,production
```

This starts mcp-k8s as an HTTPS server on port 9090 with bearer token authentication, restricted to the `default` and `production` namespaces, with all delete operations disabled globally, `apply_manifest` blocked entirely, deployment/secret creation disabled individually, secret decoding disabled, structured JSON logging enabled, a 30-second response cache, and two cluster contexts loaded (`staging` active by default).
