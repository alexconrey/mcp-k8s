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
