# Introduction

**mcp-k8s** is a Kubernetes [Model Context Protocol (MCP)](https://modelcontextprotocol.io/) server written in Rust. It exposes Kubernetes cluster operations as MCP tools consumable by Claude Code and other MCP-compatible clients.

## What is mcp-k8s?

mcp-k8s bridges the gap between AI assistants and Kubernetes clusters. It translates natural-language requests from an MCP client into Kubernetes API calls, returning structured, token-efficient responses. Instead of raw Kubernetes JSON objects (which can be hundreds of lines), mcp-k8s returns focused summaries containing only the fields relevant to the caller.

## Key Features

- **166 tools** covering every GA Kubernetes resource type -- Deployments, Pods, Services, Ingresses, ConfigMaps, Secrets, StatefulSets, DaemonSets, CronJobs, Jobs, RBAC resources, Nodes, PVs, PVCs, HPAs, PDBs, NetworkPolicies, and more.
- **Two runtime modes** -- stdio for local use with Claude Code, and HTTP for in-cluster deployment as a Kubernetes pod.
- **Fine-grained permission controls** -- Global and per-resource flags to disable create, update, or delete operations. Read operations are always allowed.
- **Namespace allowlisting** -- Restrict tool operations to a set of allowed namespaces.
- **Token-efficient responses** -- Focused summary and detail types strip irrelevant fields from Kubernetes API responses, reducing context window consumption.
- **Generic tools** -- `apply_manifest` and `get_resource_yaml` work with any Kubernetes resource type, including CRDs.
- **Auth introspection** -- `whoami`, `can_i`, and `list_my_permissions` tools let the AI understand its own access level.

## Architecture

mcp-k8s is structured as both a library crate (`mcp_k8s`) and a binary crate (`mcp-k8s`):

```
src/
+-- lib.rs              # Public API re-exports
+-- main.rs             # Binary entry point (stdio and HTTP modes)
+-- client.rs           # K8sClient -- kube::Client wrapper with namespace allowlist
+-- permissions.rs      # CRUD permission controls
+-- error.rs            # Error types
+-- types.rs            # Summary/detail structs for K8s resources
+-- extract.rs          # K8s API objects -> summary/detail extraction
+-- mcp/
|   +-- protocol.rs     # JSON-RPC 2.0 types and helpers
|   +-- definitions.rs  # MCP tool inputSchema definitions
|   +-- handlers.rs     # Tool dispatch
+-- resources/
    +-- deployment.rs   # Deployment tools (CRUD, scale, restart, rollback)
    +-- pod.rs          # Pod tools (CRUD, exec, evict, logs)
    +-- service.rs      # Service tools
    +-- ...             # 30+ resource modules
```

The library crate can be embedded in other applications. The [deckwatch](https://github.com/alexconrey/deckwatch) project uses `mcp_k8s` as a dependency for shared Kubernetes tool implementations.

## MCP Protocol

mcp-k8s implements [MCP specification version 2025-11-25](https://spec.modelcontextprotocol.io/) using JSON-RPC 2.0. It handles four methods:

| Method | Description |
|--------|-------------|
| `initialize` | Returns server info and capabilities |
| `notifications/initialized` | Acknowledges client initialization |
| `tools/list` | Returns all available tool definitions (filtered by permissions) |
| `tools/call` | Dispatches a tool call and returns the result |

## Next Steps

- [Installation](./getting-started/installation.md) -- Build from source or pull the Docker image
- [Configuration](./getting-started/configuration.md) -- CLI args, environment variables, and permission flags
- [Quick Start](./getting-started/quickstart.md) -- Get up and running in minutes
- [Tools Reference](./tools/overview.md) -- Complete reference for all 166 tools
