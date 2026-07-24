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

### Bearer token authentication

```bash
docker run --rm \
  -v ~/.kube/config:/home/nonroot/.kube/config:ro \
  -p 8080:8080 \
  -e AUTH_TOKEN=my-secret-token \
  ghcr.io/alexconrey/mcp-k8s:latest
```

When `AUTH_TOKEN` is set, all requests to `/mcp` and `/mcp/sse` must include an `Authorization: Bearer my-secret-token` header. The `/healthz`, `/metrics`, `/swagger-ui`, and `/openapi.json` endpoints remain unauthenticated.

### TLS / HTTPS

```bash
docker run --rm \
  -v ~/.kube/config:/home/nonroot/.kube/config:ro \
  -v /path/to/certs:/certs:ro \
  -p 8443:8443 \
  -e MCP_K8S_LISTEN=0.0.0.0:8443 \
  -e TLS_CERT=/certs/tls.crt \
  -e TLS_KEY=/certs/tls.key \
  ghcr.io/alexconrey/mcp-k8s:latest
```

### Structured JSON logging

```bash
docker run --rm \
  -v ~/.kube/config:/home/nonroot/.kube/config:ro \
  -p 8080:8080 \
  -e LOG_FORMAT=json \
  ghcr.io/alexconrey/mcp-k8s:latest
```

Set `LOG_FORMAT=json` for structured JSON log output, suitable for log aggregation pipelines such as Loki, Datadog, or CloudWatch.

### Multi-cluster

```bash
docker run --rm \
  -v ~/.kube/config:/home/nonroot/.kube/config:ro \
  -p 8080:8080 \
  -e MCP_K8S_CONTEXTS=staging,production \
  ghcr.io/alexconrey/mcp-k8s:latest
```

## Environment Variables Reference

| Variable | Description | Default |
|----------|-------------|---------|
| `MCP_K8S_LISTEN` | HTTP listen address | `0.0.0.0:8080` |
| `MCP_K8S_NAMESPACES` | Comma-separated namespace allowlist | *(all)* |
| `MCP_K8S_CONTEXTS` | Comma-separated kubeconfig context names for multi-cluster | *(default context)* |
| `DISABLE_CREATE` | Disable all create operations | `false` |
| `DISABLE_UPDATE` | Disable all update operations | `false` |
| `DISABLE_DELETE` | Disable all delete operations | `false` |
| `MCP_K8S_DISABLE` | Comma-separated resource-action overrides | *(none)* |
| `AUTH_TOKEN` | Bearer token for HTTP endpoint authentication | *(none)* |
| `TLS_CERT` | Path to TLS certificate PEM file | *(none)* |
| `TLS_KEY` | Path to TLS private key PEM file | *(none)* |
| `DISABLE_SECRET_DECODE` | Prevent secret value decoding | `false` |
| `LOG_FORMAT` | Log output format (`text` or `json`) | `text` |
| `KUBECONFIG` | Path to kubeconfig file | *(auto-detected)* |
| `RUST_LOG` | Log level filter | `info` |

## Endpoints

When running in HTTP mode, the following endpoints are available:

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/mcp` | POST | MCP JSON-RPC 2.0 endpoint |
| `/mcp/sse` | POST | MCP JSON-RPC 2.0 via Server-Sent Events transport |
| `/healthz` | GET | Health check (returns `ok`) |
| `/metrics` | GET | Prometheus metrics (`mcp_requests_total`, `mcp_tool_calls_total`, `mcp_tool_call_duration_seconds`) |
| `/swagger-ui` | GET | OpenAPI / Swagger UI |
| `/openapi.json` | GET | OpenAPI JSON spec |

### Prometheus Metrics

The `/metrics` endpoint serves Prometheus-format metrics. Scrape it with your Prometheus instance to monitor request volume and tool call latency:

```yaml
# prometheus.yml scrape config
scrape_configs:
  - job_name: mcp-k8s
    static_configs:
      - targets: ['mcp-k8s:8080']
```

Exposed metrics:

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `mcp_requests_total` | Counter | `method` | Total MCP JSON-RPC requests by method |
| `mcp_tool_calls_total` | Counter | `tool` | Total tool calls by tool name |
| `mcp_tool_call_duration_seconds` | Histogram | `tool` | Tool call duration in seconds |
| `mcp_tool_call_errors_total` | Counter | `tool` | Total tool call errors by tool name |

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
