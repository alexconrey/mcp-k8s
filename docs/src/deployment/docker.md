# Docker Deployment

mcp-k8s provides a pre-built container image for running as an HTTP server. This is the foundation for both standalone Docker deployments and Kubernetes deployments.

## Image

```
ghcr.io/alexconrey/mcp-k8s:latest
```

Tags:

| Tag | Meaning |
|-----|---------|
| `vX.Y.Z` | A release. Pin this in production. |
| `vX.Y` | The newest patch release of `X.Y`. |
| `latest` | The newest release. |
| `main`, `<commit sha>` | Builds of the `main` branch. Unreleased; for testing only. |

The image is a static musl binary in a `scratch` image, running as UID 65534. It runs `mcp-k8s --http` on port 8080 by default.

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

When `AUTH_TOKEN` is set, all requests to `/mcp` and `/mcp/sse` must include an `Authorization: Bearer my-secret-token` header. The `/healthz`, `/metrics`, `/swagger-ui`, and `/openapi.json` endpoints remain unauthenticated. To keep the token out of the container's environment, mount it as a file and pass `AUTH_TOKEN_FILE=/path/to/token`. Set `MCP_K8S_ALLOWED_HOSTS` to the host names clients use (see [Configuration](../getting-started/configuration.md#browser-and-dns-rebinding-protection)).

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

### OpenTelemetry export

```bash
docker run --rm \
  -v ~/.kube/config:/home/nonroot/.kube/config:ro \
  -p 8080:8080 \
  -e OTEL_EXPORTER_OTLP_ENDPOINT=http://otel-collector:4318 \
  -e OTEL_RESOURCE_ATTRIBUTES=k8s.cluster.name=dev \
  ghcr.io/alexconrey/mcp-k8s:latest
```

Exports traces, the `mcp.server.operation.duration` metric and logs over OTLP
http/protobuf. See [Telemetry](../telemetry.md).

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
| `AUTH_TOKEN_FILE` | Read the bearer token from this file | *(none)* |
| `MCP_K8S_ALLOWED_HOSTS` | Host names the server answers to (DNS-rebinding protection) | loopback names on a loopback listen address, otherwise any |
| `MCP_K8S_CORS_ALLOWED_ORIGINS` | Origins browser-based clients may call from (CORS) | *(none)* |
| `MCP_K8S_MAX_REQUEST_BYTES` | Maximum request body size | `4194304` |
| `MCP_K8S_REQUEST_TIMEOUT_SECS` | Request timeout | `360` |
| `TLS_CERT` | Path to TLS certificate PEM file | *(none)* |
| `TLS_KEY` | Path to TLS private key PEM file | *(none)* |
| `ENABLE_SECRET_DECODE` | Let tools return Secret values (redacted otherwise) | `false` |
| `LOG_FORMAT` | Log output format (`text` or `json`) | `text` |
| `KUBECONFIG` | Path to kubeconfig file | *(auto-detected)* |
| `RUST_LOG` | Console log level filter | `info` |
| `OTEL_EXPORTER_OTLP_ENDPOINT` | OTLP http/protobuf collector URL; export is off without it | *(none)* |
| `OTEL_SERVICE_NAME` | `service.name` for exported telemetry | `mcp-k8s` |
| `OTEL_RESOURCE_ATTRIBUTES` | Extra resource attributes | *(none)* |
| `OTEL_TRACES_SAMPLER` | Trace sampler | `parentbased_always_on` |
| `MCP_K8S_OTEL_FILTER` | Filter for exported spans and logs | `info` |

## Endpoints

When running in HTTP mode, the following endpoints are available:

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/mcp` | POST | MCP JSON-RPC 2.0 endpoint |
| `/mcp/sse` | POST | MCP JSON-RPC 2.0 via Server-Sent Events transport |
| `/healthz` | GET | Health check (returns `ok`) |
| `/metrics` | GET | Prometheus metrics (`mcp_requests_total`, `mcp_k8s_tool_calls_total`, `mcp_k8s_tool_call_duration_seconds`) |
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
| `mcp_k8s_tool_calls_total` | Counter | `tool`, `status` | Total tool calls by tool name. `status` is `ok`, `error` or `denied` (blocked by permissions) |
| `mcp_k8s_tool_call_duration_seconds` | Histogram | `tool` | Tool call duration in seconds |

These names will be deprecated in a later release in favour of the
OpenTelemetry metric `mcp.server.operation.duration`, exported over OTLP (see
[Telemetry](../telemetry.md#old-metric-names)).

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
