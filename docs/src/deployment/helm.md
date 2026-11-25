# Helm Chart

mcp-k8s includes a Helm chart for deploying to Kubernetes clusters. The chart creates a Deployment, Service, ServiceAccount, and RBAC resources.

The chart requires Kubernetes 1.33 or newer (`kubeVersion: ">=1.33.0-0"`, which also accepts pre-release server versions such as EKS's `v1.35.9-eks-…`).

## Installation

### Add the chart (from local source)

```bash
git clone https://github.com/alexconrey/mcp-k8s.git
cd mcp-k8s
```

### Install read-only (recommended starting point)

```bash
helm install mcp-k8s ./helm/mcp-k8s -n mcp-k8s --create-namespace \
  --set rbac.readOnly=true
```

The ServiceAccount gets only get/list/watch, and the server hides every write
tool. See [RBAC](#rbac) and [RBAC Setup](rbac.md).

### Install with default values

```bash
helm install mcp-k8s ./helm/mcp-k8s -n mcp-k8s --create-namespace
```

### Install with custom values

```bash
helm install mcp-k8s ./helm/mcp-k8s \
  -n mcp-k8s --create-namespace \
  --set namespaces='{default,production}' \
  --set permissions.disableDelete=true
```

### Install with a values file

```bash
helm install mcp-k8s ./helm/mcp-k8s \
  -n mcp-k8s --create-namespace \
  -f my-values.yaml
```

## Values Reference

### Image

| Key | Default | Description |
|-----|---------|-------------|
| `image.repository` | `ghcr.io/alexconrey/mcp-k8s` | Container image repository |
| `image.tag` | `""` (the chart's `appVersion`, as `v<appVersion>`) | Image tag. Leave empty to run the image released with the chart; set `main` for unreleased builds |
| `image.pullPolicy` | `IfNotPresent` | Image pull policy |

### Replicas

| Key | Default | Description |
|-----|---------|-------------|
| `replicaCount` | `1` | Number of replicas |

### Service

| Key | Default | Description |
|-----|---------|-------------|
| `service.type` | `ClusterIP` | Service type |
| `service.port` | `8080` | Service port |

### Namespaces

| Key | Default | Description |
|-----|---------|-------------|
| `namespaces` | `[]` | List of allowed namespaces. Empty means all namespaces. |

### Permissions

| Key | Default | Description |
|-----|---------|-------------|
| `permissions.disableCreate` | `false` | Globally disable create operations |
| `permissions.disableUpdate` | `false` | Globally disable update operations |
| `permissions.disableDelete` | `false` | Globally disable delete operations |
| `permissions.disableActions` | `[]` | List of resource-action strings to disable (e.g. `["pods-delete", "secrets-create"]`) |
| `permissions.enableApplyManifest` | `false` | Enable `apply_manifest`, an unrestricted raw apply that skips the workload guardrails (see [Permissions](../permissions.md#apply_manifest)) |
| `permissions.disableApplyManifest` | `false` | Deprecated (it's off by default); still honoured and wins over `enableApplyManifest` |
| `extraEnv` | `[]` | Extra container environment variables, e.g. `MCP_K8S_ALLOW_LOADBALANCER=true` to relax a [workload guardrail](../permissions.md#workload-guardrails). Quote values in a values file, or use `--set-string` on the command line |

### Resources

| Key | Default | Description |
|-----|---------|-------------|
| `resources.requests.cpu` | `50m` | CPU request |
| `resources.requests.memory` | `64Mi` | Memory request |
| `resources.limits.cpu` | `200m` | CPU limit |
| `resources.limits.memory` | `256Mi` | Memory limit |

The server is a small static binary and idles at around 20Mi. Raise memory if
you list very large collections or enable the response cache.

### Security context

The image is a `scratch` image holding one static binary that runs as user
65534 and never writes to disk, so the defaults follow the Kubernetes
"restricted" Pod Security Standard. The ServiceAccount token stays mounted
(`automountServiceAccountToken: true`): the server needs it to call the API.

| Key | Default | Description |
|-----|---------|-------------|
| `podSecurityContext` | `runAsNonRoot: true`, `runAsUser`/`runAsGroup: 65534`, `seccompProfile: RuntimeDefault` | Pod-level `securityContext`. Replaces the default as a whole when set |
| `securityContext` | the same, plus `allowPrivilegeEscalation: false`, `readOnlyRootFilesystem: true`, `capabilities.drop: [ALL]` | Container-level `securityContext` |

### RBAC

The chart grants only what the tools need, without wildcards. The rules, and
the tool behind each one, are on the [RBAC Setup](rbac.md) page.

| Key | Default | Description |
|-----|---------|-------------|
| `rbac.create` | `true` | Create RBAC resources |
| `rbac.clusterWide` | `true` | `true`: a ClusterRole and ClusterRoleBinding. `false`: a Role and RoleBinding in each namespace of `namespaces` (the release namespace when empty), without cluster-scoped resources |
| `rbac.readOnly` | `false` | **Recommended starting point.** Grant only get/list/watch (plus the self-review creates `whoami`/`can_i` use) and set `DISABLE_CREATE`/`DISABLE_UPDATE`/`DISABLE_DELETE`, so `tools/list` matches. |
| `rbac.secrets` | `true` | Grant access to Secrets. RBAC can't grant "keys only", so any read access lets the pod's token read secret values even while the server redacts them. `false` leaves Secrets out and hides the secret write tools |
| `rbac.csrSigners` | `[]` | Signer names `approve_csr`/`deny_csr` may act for (`approve` on `certificates.k8s.io/signers`). Kubernetes refuses approvals without it |
| `rbac.extraRules` | `[]` | More rules, e.g. for custom resources the custom resource tools or `watch_resource` should reach |
| `rbac.allowWildcards` | `false` | Allow `"*"` in `extraRules` (rendering fails otherwise) |

Secret value decoding is becoming opt-in on the server (`--enable-secret-decode`);
its chart value arrives with that change. If you leave decoding off, consider
whether the secret tools are needed at all, and set `rbac.secrets: false` if not.

### ServiceAccount

| Key | Default | Description |
|-----|---------|-------------|
| `serviceAccount.create` | `true` | Create a ServiceAccount |
| `serviceAccount.name` | `""` | ServiceAccount name (auto-generated if empty) |

### Ingress

| Key | Default | Description |
|-----|---------|-------------|
| `ingress.enabled` | `false` | Enable Ingress resource creation |
| `ingress.className` | `""` | IngressClass name |
| `ingress.host` | `""` | Ingress hostname |

### PodDisruptionBudget

| Key | Default | Description |
|-----|---------|-------------|
| `pdb.enabled` | `false` | Create a PodDisruptionBudget for the mcp-k8s pods |
| `pdb.minAvailable` | *(unset)* | Minimum number of pods that must remain available during disruption (number or percentage) |
| `pdb.maxUnavailable` | *(unset)* | Maximum number of pods that can be unavailable during disruption (number or percentage) |

Specify either `minAvailable` or `maxUnavailable`, not both.

### Autoscaling (HPA)

| Key | Default | Description |
|-----|---------|-------------|
| `autoscaling.enabled` | `false` | Create a HorizontalPodAutoscaler |
| `autoscaling.minReplicas` | `1` | Minimum replica count |
| `autoscaling.maxReplicas` | `3` | Maximum replica count |
| `autoscaling.targetCPUUtilizationPercentage` | `80` | Target CPU utilization percentage |
| `autoscaling.targetMemoryUtilizationPercentage` | *(unset)* | Target memory utilization percentage (optional) |

When autoscaling is enabled, the HPA manages the replica count instead of `replicaCount`.

### NetworkPolicy

Off by default. When enabled, the pod gets a default-deny policy for ingress
and (unless `egress.enabled` is false) egress, with these exceptions:

- **Ingress**: the MCP port only (the container's `http` port), from the
  sources below. With no sources set, the MCP port is open to any source.
- **Egress**: the Kubernetes API server, cluster DNS and, when `otel.endpoint`
  is set, the OTLP collector (port taken from the endpoint, default 4318).

| Key | Default | Description |
|-----|---------|-------------|
| `networkPolicy.enabled` | `false` | Create the NetworkPolicy |
| `networkPolicy.allowedCIDRs` | `[]` | CIDR blocks allowed to reach the MCP port |
| `networkPolicy.allowedNamespaces` | `[]` | Namespaces (by name) whose pods may reach the MCP port |
| `networkPolicy.extraIngressFrom` | `[]` | More ingress peers (`podSelector`/`namespaceSelector`/`ipBlock`) |
| `networkPolicy.egress.enabled` | `true` | Restrict egress as well |
| `networkPolicy.egress.apiServer.cidrs` | `[]` | The API server's endpoint addresses (`kubectl get endpointslices -n default -l kubernetes.io/service-name=kubernetes`). Empty allows the ports to any address |
| `networkPolicy.egress.apiServer.ports` | `[443, 6443]` | API server endpoint ports |
| `networkPolicy.egress.dns.enabled` | `true` | Allow DNS (UDP/TCP 53) |
| `networkPolicy.egress.dns.to` | kube-system pods labelled `k8s-app: kube-dns` | DNS peers. Add an `ipBlock` for NodeLocal DNSCache |
| `networkPolicy.egress.otlp.to` | `[]` | OTLP collector peers. Empty allows the collector port to any address |
| `networkPolicy.egress.extra` | `[]` | More egress rules |

Most CNIs evaluate NetworkPolicy after the `kubernetes` Service's DNAT, so
pin the API server with its endpoint IPs and ports (6443 on kubeadm and k3s,
443 on most managed clusters), not the Service ClusterIP.

### OpenTelemetry

OTLP export of traces, the `mcp.server.operation.duration` metric and logs
(see [Telemetry](../telemetry.md)). Nothing is exported while `otel.endpoint`
is empty.

| Key | Default | Description |
|-----|---------|-------------|
| `otel.endpoint` | `""` | `OTEL_EXPORTER_OTLP_ENDPOINT`: the collector's OTLP http/protobuf URL, e.g. `http://otel-collector.observability:4318` (gRPC is not supported) |
| `otel.serviceName` | `""` | `OTEL_SERVICE_NAME` (the binary defaults to `mcp-k8s`) |
| `otel.clusterName` | `""` | Added to `OTEL_RESOURCE_ATTRIBUTES` as `k8s.cluster.name` |
| `otel.resourceAttributes` | `{}` | More `OTEL_RESOURCE_ATTRIBUTES`, e.g. `{deployment.environment.name: prod}` |
| `otel.sampler` | `""` | `OTEL_TRACES_SAMPLER`, e.g. `parentbased_traceidratio` |
| `otel.samplerArg` | `""` | `OTEL_TRACES_SAMPLER_ARG`, e.g. `"0.25"` |
| `otel.traces` / `otel.metrics` / `otel.logs` | `true` | `false` stops exporting that signal |
| `otel.headersSecret.name` / `.key` | `""` | A Secret key holding `OTEL_EXPORTER_OTLP_HEADERS` (e.g. `authorization=Bearer ...`) |
| `otel.filter` | `""` | `MCP_K8S_OTEL_FILTER`: tracing filter for exported spans and logs (default `info`) |
| `otel.extraEnv` | `[]` | More environment variables (rendered even without `endpoint`), e.g. `OTEL_EXPORTER_OTLP_TIMEOUT` |

```yaml
otel:
  endpoint: http://otel-collector.observability:4318
  clusterName: prod-east
  sampler: parentbased_traceidratio
  samplerArg: "0.25"
```

## Example Values Files

### Read-only deployment

```yaml
# values-readonly.yaml
rbac:
  readOnly: true   # get/list/watch only, and the server hides write tools
  secrets: false   # optional: keep Secret values out of the pod's reach
```

```bash
helm install mcp-k8s ./helm/mcp-k8s \
  -n mcp-k8s --create-namespace \
  -f values-readonly.yaml
```

### Production deployment with restrictions

```yaml
# values-production.yaml
image:
  tag: v0.5.0

replicaCount: 2

namespaces:
  - default
  - production
  - staging

permissions:
  disableDelete: true
  disableActions:
    - namespace-create
    - clusterrole-create
    - clusterrolebinding-create

resources:
  requests:
    cpu: 100m
    memory: 128Mi
  limits:
    cpu: 500m
    memory: 512Mi

ingress:
  enabled: true
  className: nginx
  host: mcp-k8s.internal.example.com
```

### Production deployment with autoscaling and network policy

```yaml
# values-production-hardened.yaml
image:
  tag: v0.5.0

replicaCount: 2

namespaces:
  - production

permissions:
  disableDelete: true

autoscaling:
  enabled: true
  minReplicas: 2
  maxReplicas: 5
  targetCPUUtilizationPercentage: 70

pdb:
  enabled: true
  minAvailable: 1

networkPolicy:
  enabled: true
  allowedNamespaces:
    - monitoring
    - ai-agents
  egress:
    apiServer:
      cidrs: ["10.0.0.10/32"]   # the kubernetes endpoint IP(s)
```

### Namespace-scoped deployment

```yaml
# values-namespaced.yaml
namespaces:
  - my-namespace

rbac:
  create: true
  clusterWide: false
```

## Upgrading

```bash
helm upgrade mcp-k8s ./helm/mcp-k8s \
  -n mcp-k8s \
  --set image.tag=v0.6.0
```

## Uninstalling

```bash
helm uninstall mcp-k8s -n mcp-k8s
kubectl delete namespace mcp-k8s
```
