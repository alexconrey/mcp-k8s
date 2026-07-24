# Helm Chart

mcp-k8s includes a Helm chart for deploying to Kubernetes clusters. The chart creates a Deployment, Service, ServiceAccount, and RBAC resources.

## Installation

### Add the chart (from local source)

```bash
git clone https://github.com/alexconrey/mcp-k8s.git
cd mcp-k8s
```

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
| `image.tag` | `latest` | Image tag |
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

### Resources

| Key | Default | Description |
|-----|---------|-------------|
| `resources.requests.cpu` | `50m` | CPU request |
| `resources.requests.memory` | `64Mi` | Memory request |
| `resources.limits.cpu` | `200m` | CPU limit |
| `resources.limits.memory` | `256Mi` | Memory limit |

### RBAC

| Key | Default | Description |
|-----|---------|-------------|
| `rbac.create` | `true` | Create RBAC resources |
| `rbac.clusterWide` | `true` | If true, creates ClusterRole/ClusterRoleBinding. If false, creates namespaced Role/RoleBinding. |

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

| Key | Default | Description |
|-----|---------|-------------|
| `networkPolicy.enabled` | `false` | Create a NetworkPolicy restricting ingress to mcp-k8s pods |
| `networkPolicy.allowedCIDRs` | `[]` | List of CIDR blocks allowed to reach the service port |
| `networkPolicy.allowedNamespaces` | `[]` | List of namespace names whose pods are allowed to reach the service port |

When enabled, the NetworkPolicy restricts ingress to the service port (`service.port`) and only from the specified CIDRs and/or namespaces. If neither `allowedCIDRs` nor `allowedNamespaces` is set, all ingress traffic on the service port is allowed.

## Example Values Files

### Read-only deployment

```yaml
# values-readonly.yaml
permissions:
  disableCreate: true
  disableUpdate: true
  disableDelete: true
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
