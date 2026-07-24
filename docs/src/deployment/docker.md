# Docker Deployment

mcp-k8s provides a pre-built container image for running as an HTTP server. This is the foundation for both standalone Docker deployments and Kubernetes deployments.

## Image

```
ghcr.io/alexconrey/mcp-k8s:latest
```

The image is built on `gcr.io/distroless/cc-debian12:nonroot` and runs the `mcp-k8s --http` command on port 8080 by default.

## Building the Image

```bash
git clone https://github.com/alexconrey/mcp-k8s.git
cd mcp-k8s
docker build -t mcp-k8s .
```

## Running Standalone

### With local kubeconfig

Mount your kubeconfig into the container:

```bash
docker run --rm \
  -v ~/.kube/config:/home/nonroot/.kube/config:ro \
  -p 8080:8080 \
  ghcr.io/alexconrey/mcp-k8s:latest
```

### With environment variables

```bash
docker run --rm \
  -v ~/.kube/config:/home/nonroot/.kube/config:ro \
  -p 8080:8080 \
  -e MCP_K8S_NAMESPACES=default,production \
  -e DISABLE_DELETE=true \
  -e RUST_LOG=info \
  ghcr.io/alexconrey/mcp-k8s:latest
```

### Read-only mode

```bash
docker run --rm \
  -v ~/.kube/config:/home/nonroot/.kube/config:ro \
  -p 8080:8080 \
  -e DISABLE_CREATE=true \
  -e DISABLE_UPDATE=true \
  -e DISABLE_DELETE=true \
  ghcr.io/alexconrey/mcp-k8s:latest
```

### Custom listen address

```bash
docker run --rm \
  -v ~/.kube/config:/home/nonroot/.kube/config:ro \
  -p 9090:9090 \
  -e MCP_K8S_LISTEN=0.0.0.0:9090 \
  ghcr.io/alexconrey/mcp-k8s:latest
```

### Per-resource restrictions

```bash
docker run --rm \
  -v ~/.kube/config:/home/nonroot/.kube/config:ro \
  -p 8080:8080 \
  -e MCP_K8S_DISABLE=deployment-delete,secret-create,namespace-delete \
  ghcr.io/alexconrey/mcp-k8s:latest
```

## Environment Variables Reference

| Variable | Description | Default |
|----------|-------------|---------|
| `MCP_K8S_LISTEN` | HTTP listen address | `0.0.0.0:8080` |
| `MCP_K8S_NAMESPACES` | Comma-separated namespace allowlist | *(all)* |
| `DISABLE_CREATE` | Disable all create operations | `false` |
| `DISABLE_UPDATE` | Disable all update operations | `false` |
| `DISABLE_DELETE` | Disable all delete operations | `false` |
| `MCP_K8S_DISABLE` | Comma-separated resource-action overrides | *(none)* |
| `KUBECONFIG` | Path to kubeconfig file | *(auto-detected)* |
| `RUST_LOG` | Log level filter | `info` |

## Endpoints

When running in HTTP mode, the following endpoints are available:

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/mcp` | POST | MCP JSON-RPC 2.0 endpoint |
| `/healthz` | GET | Health check (returns `ok`) |
| `/swagger-ui` | GET | OpenAPI / Swagger UI |
| `/openapi.json` | GET | OpenAPI JSON spec |

## Health Checks

The `/healthz` endpoint returns `ok` with a 200 status code when the server is running. Use this for Docker health checks:

```dockerfile
HEALTHCHECK --interval=30s --timeout=3s \
  CMD curl -f http://localhost:8080/healthz || exit 1
```

Or in `docker-compose.yml`:

```yaml
services:
  mcp-k8s:
    image: ghcr.io/alexconrey/mcp-k8s:latest
    ports:
      - "8080:8080"
    volumes:
      - ~/.kube/config:/home/nonroot/.kube/config:ro
    environment:
      - MCP_K8S_NAMESPACES=default
      - DISABLE_DELETE=true
    healthcheck:
      test: ["CMD", "curl", "-f", "http://localhost:8080/healthz"]
      interval: 30s
      timeout: 3s
      retries: 3
```
