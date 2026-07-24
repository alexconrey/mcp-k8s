# Installation

mcp-k8s can be installed by building from source, using the pre-built Docker image, or deploying to a Kubernetes cluster via Helm or Kustomize.

## Building from Source

### Prerequisites

- [Rust](https://rustup.rs/) (stable toolchain)
- Access to a Kubernetes cluster (via `KUBECONFIG` or in-cluster config)

### Build

```bash
git clone https://github.com/alexconrey/mcp-k8s.git
cd mcp-k8s
cargo build --release
```

The binary is located at `target/release/mcp-k8s`.

### Install to PATH

```bash
cargo install --path .
```

## Docker Image

Pre-built container images are available from GitHub Container Registry:

```bash
docker pull ghcr.io/alexconrey/mcp-k8s:latest
```

The image uses `gcr.io/distroless/cc-debian12:nonroot` as the runtime base and runs `mcp-k8s --http` on port 8080 by default.

```bash
# Run with your local kubeconfig
docker run --rm \
  -v ~/.kube/config:/home/nonroot/.kube/config:ro \
  -p 8080:8080 \
  ghcr.io/alexconrey/mcp-k8s:latest
```

## Claude Code MCP Configuration (stdio mode)

To use mcp-k8s as an MCP server with Claude Code, add it to your Claude Code MCP settings. In your project's `.claude/settings.json` (or the global `~/.claude/settings.json`):

```json
{
  "mcpServers": {
    "mcp-k8s": {
      "command": "mcp-k8s",
      "args": []
    }
  }
}
```

If you installed from source and the binary is not on your PATH, use the full path:

```json
{
  "mcpServers": {
    "mcp-k8s": {
      "command": "/path/to/mcp-k8s",
      "args": []
    }
  }
}
```

### With namespace restrictions

```json
{
  "mcpServers": {
    "mcp-k8s": {
      "command": "mcp-k8s",
      "args": ["--namespaces", "default,staging,production"]
    }
  }
}
```

### Read-only mode

```json
{
  "mcpServers": {
    "mcp-k8s": {
      "command": "mcp-k8s",
      "args": ["--disable-create", "--disable-update", "--disable-delete"]
    }
  }
}
```

### With environment variables

```json
{
  "mcpServers": {
    "mcp-k8s": {
      "command": "mcp-k8s",
      "env": {
        "KUBECONFIG": "/path/to/kubeconfig",
        "MCP_K8S_NAMESPACES": "default,production",
        "RUST_LOG": "info"
      }
    }
  }
}
```

## In-Cluster Deployment

For deploying mcp-k8s inside a Kubernetes cluster as an HTTP server, see the [Deployment](../deployment/docker.md) section.
