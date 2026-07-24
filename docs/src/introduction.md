# Introduction

**mcp-k8s** is a Kubernetes [Model Context Protocol (MCP)](https://modelcontextprotocol.io/) server written in Rust. It exposes Kubernetes cluster operations as MCP tools consumable by Claude Code and other MCP-compatible clients.

## What is mcp-k8s?

mcp-k8s bridges the gap between AI assistants and Kubernetes clusters. It translates natural-language requests from an MCP client into Kubernetes API calls, returning structured, token-efficient responses. Instead of raw Kubernetes JSON objects (which can be hundreds of lines), mcp-k8s returns focused summaries containing only the fields relevant to the caller.

## Key Features

- **202 tools** across 49 resource modules covering every GA Kubernetes resource type -- Deployments, Pods, Services, Ingresses, ConfigMaps, Secrets, StatefulSets, DaemonSets, CronJobs, Jobs, RBAC resources, Nodes, PVs, PVCs, HPAs, PDBs, NetworkPolicies, admission controllers, CSI resources, and more.
- **Two runtime modes** -- stdio for local use with Claude Code, and HTTP for in-cluster deployment as a Kubernetes pod.
- **SSE transport** -- `POST /mcp/sse` endpoint for Server-Sent Events streaming alongside the standard `POST /mcp` JSON-RPC endpoint.
- **MCP resources** -- `resources/list` and `resources/read` support for direct resource access via `k8s://` URIs (pods, deployments, services, configmaps, secrets, statefulsets, daemonsets, jobs, cronjobs, ingresses, nodes, namespaces).
- **MCP prompts** -- Built-in diagnostic prompts (`diagnose-pod`, `review-namespace-rbac`, `cluster-health-check`, `resource-usage-report`) that guide the AI through multi-step investigation workflows.
- **Prometheus metrics** -- `GET /metrics` endpoint exposing `mcp_requests_total`, `mcp_tool_calls_total`, and `mcp_tool_call_duration_seconds` with per-tool labels.
- **Fine-grained permission controls** -- Global and per-resource flags to disable create, update, or delete operations. Read operations are always allowed. Disabled tools are filtered from `tools/list` responses and rejected in `tools/call`.
- **Namespace allowlisting** -- Restrict tool operations to a set of allowed namespaces.
- **Token-efficient responses** -- Focused summary and detail types strip irrelevant fields from Kubernetes API responses, reducing context window consumption.
- **Multi-cluster support** -- Load multiple kubeconfig contexts with `--contexts` and switch between clusters at runtime using `list_clusters`, `switch_cluster`, and `get_active_cluster` tools.
- **Watch/subscribe** -- `watch_resource` tool watches any resource type for ADDED, MODIFIED, and DELETED events over a configurable time window.
- **CRD discovery** -- `list_crds`, `get_crd`, `list_custom_resources`, `get_custom_resource`, `create_custom_resource`, `update_custom_resource`, and `delete_custom_resource` tools provide full lifecycle management of any installed CRD via dynamic API discovery.
- **Generic tools** -- `apply_manifest` (server-side apply) and `get_resource_yaml` work with any Kubernetes resource type, including CRDs.
- **Auth introspection** -- `whoami`, `can_i`, and `list_my_permissions` tools let the AI understand its own access level.
- **TLS and bearer token auth** -- Optional `--tls-cert`/`--tls-key` for HTTPS and `--auth-token` for bearer token authentication on the HTTP server.
- **Swagger UI** -- Interactive API documentation at `GET /swagger-ui` with OpenAPI spec at `GET /openapi.json`.
- **Structured JSON logging** -- `--log-format json` for production log aggregation.

## Architecture

```
┌─────────────────┐     ┌──────────────────────────────────────────┐
│  Claude Code    │     │              mcp-k8s                     │
│  (MCP Client)   │────▶│                                          │
│                 │stdio│  ┌──────────┐    ┌───────────────────┐   │
└─────────────────┘     │  │ JSON-RPC │    │   Permissions     │   │
                        │  │ Dispatch │───▶│   Check           │   │
┌─────────────────┐     │  └──────────┘    └───────────────────┘   │
│  HTTP Client    │     │       │                    │              │
│  (in-cluster)   │────▶│       ▼                    ▼              │
│                 │http │  ┌──────────┐    ┌───────────────────┐   │
└─────────────────┘     │  │ Resource │    │   K8sClient       │   │
                        │  │ Modules  │───▶│   (kube-rs)       │──▶│ K8s API
┌─────────────────┐     │  │ (49)     │    └───────────────────┘   │
│  Prometheus     │────▶│  └──────────┘                            │
│                 │/metrics                                        │
└─────────────────┘     │  Endpoints:                              │
                        │  POST /mcp      — JSON-RPC               │
                        │  POST /mcp/sse  — SSE transport          │
                        │  GET  /healthz  — health check           │
                        │  GET  /metrics  — Prometheus             │
                        │  GET  /swagger-ui — API docs             │
                        └──────────────────────────────────────────┘
```

mcp-k8s is structured as both a library crate (`mcp_k8s`) and a binary crate (`mcp-k8s`):

```
src/
├── lib.rs              # Public API re-exports
├── main.rs             # Binary entry point (stdio and HTTP modes)
├── client.rs           # K8sClient — kube::Client wrapper with namespace allowlist
├── permissions.rs      # CRUD permission controls
├── error.rs            # Error types
├── types.rs            # Summary/detail structs for K8s resources
├── extract.rs          # K8s API objects → summary/detail extraction
├── mcp/
│   ├── protocol.rs     # JSON-RPC 2.0 types and helpers (with utoipa schemas)
│   ├── definitions.rs  # MCP tool inputSchema definitions (filtered by permissions)
│   ├── handlers.rs     # Tool dispatch (with permission checks)
│   └── tests.rs        # MCP dispatch and permission filtering tests
└── resources/
    ├── mod.rs           # Aggregates all resource modules (tool_definitions + handle_tool)
    ├── deployment.rs    # Deployment tools (CRUD, scale, restart, rollback)
    ├── pod.rs           # Pod tools (CRUD, exec, evict, logs)
    ├── service.rs       # Service tools
    └── ...              # 46 resource modules total
```

The library crate can be embedded in other applications. The [deckwatch](https://github.com/alexconrey/deckwatch) project uses `mcp_k8s` as a dependency for shared Kubernetes tool implementations.

## MCP Protocol

mcp-k8s implements [MCP specification version 2025-11-25](https://spec.modelcontextprotocol.io/) using JSON-RPC 2.0. It handles the following methods:

| Method | Description |
|--------|-------------|
| `initialize` | Returns server info and capabilities (tools, resources, prompts) |
| `notifications/initialized` | Acknowledges client initialization |
| `tools/list` | Returns all available tool definitions (filtered by permissions) |
| `tools/call` | Dispatches a tool call and returns the result |
| `resources/list` | Lists available `k8s://` resource URIs |
| `resources/read` | Reads a specific Kubernetes resource by URI |
| `prompts/list` | Lists available diagnostic prompts |
| `prompts/get` | Returns prompt messages for a given prompt name |

## Next Steps

- [Installation](./getting-started/installation.md) -- Build from source or pull the Docker image
- [Configuration](./getting-started/configuration.md) -- CLI args, environment variables, and permission flags
- [Quick Start](./getting-started/quickstart.md) -- Get up and running in minutes
- [Tools Reference](./tools/overview.md) -- Complete reference for all 202 tools
