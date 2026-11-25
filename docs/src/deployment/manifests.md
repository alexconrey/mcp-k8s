# Kubernetes Manifests

mcp-k8s includes raw Kubernetes manifests for deploying without Helm, using either Kustomize or direct `kubectl apply`.

## Kustomize

The manifests are organized as a Kustomize base with a read-only overlay
and a NetworkPolicy overlay.

### Directory Structure

```
helm/charts/mcp-k8s/
+-- kustomization.yaml          # Full access (points at base/)
+-- base/
|   +-- kustomization.yaml
|   +-- namespace.yaml           # mcp-k8s namespace
|   +-- serviceaccount.yaml      # ServiceAccount
|   +-- clusterrole.yaml         # Full-access ClusterRole
|   +-- clusterrolebinding.yaml  # ClusterRoleBinding
|   +-- deployment.yaml          # Deployment (HTTP mode, hardened securityContext)
|   +-- service.yaml             # ClusterIP Service on port 8080
+-- read-only/
|   +-- kustomization.yaml       # Read-only overlay
|   +-- clusterrole-patch.yaml   # get/list/watch-only rules
+-- network-policy/
    +-- kustomization.yaml       # Base + NetworkPolicy
    +-- networkpolicy.yaml
```

The ClusterRole rules are the Helm chart's (see [RBAC Setup](rbac.md) for the
tool behind each rule).

### Deploy with full access

```bash
kubectl apply -k helm/charts/mcp-k8s/
```

This creates:

- Namespace `mcp-k8s`
- ServiceAccount `mcp-k8s`
- ClusterRole `mcp-k8s` with full CRUD permissions
- ClusterRoleBinding `mcp-k8s`
- Deployment `mcp-k8s` running `ghcr.io/alexconrey/mcp-k8s:latest`
- Service `mcp-k8s` on port 8080

### Deploy in read-only mode (recommended starting point)

```bash
kubectl apply -k helm/charts/mcp-k8s/read-only/
```

The read-only overlay replaces the ClusterRole's rules with get/list/watch-only
ones and sets `DISABLE_CREATE=true`, `DISABLE_UPDATE=true` and
`DISABLE_DELETE=true` on the Deployment, so `tools/list` shows only read tools.

### Add a NetworkPolicy

```bash
kubectl apply -k helm/charts/mcp-k8s/network-policy/
```

Default-deny for the mcp-k8s pod, except ingress to port 8080 from the
`mcp-k8s` namespace and egress to the API server (ports 443/6443) and cluster
DNS. Edit `network-policy/networkpolicy.yaml` first: who may call the server,
the API server's endpoint IPs, and the OTLP collector if you export telemetry.
To combine it with read-only mode, point its `resources` at `../read-only`.

### Secrets

The ClusterRole grants Secrets access for the secret tools. RBAC can't grant
"keys only", so the pod's token can read secret values even though the server
redacts them. Remove the `secrets` rule from `base/clusterrole.yaml` (or
`read-only/clusterrole-patch.yaml`) if you don't need those tools.

## Manual kubectl apply

If you prefer not to use Kustomize, apply the manifests individually:

```bash
# Create namespace
kubectl apply -f helm/charts/mcp-k8s/base/namespace.yaml

# Create ServiceAccount
kubectl apply -f helm/charts/mcp-k8s/base/serviceaccount.yaml

# Create RBAC
kubectl apply -f helm/charts/mcp-k8s/base/clusterrole.yaml
kubectl apply -f helm/charts/mcp-k8s/base/clusterrolebinding.yaml

# Deploy
kubectl apply -f helm/charts/mcp-k8s/base/deployment.yaml
kubectl apply -f helm/charts/mcp-k8s/base/service.yaml
```

## Deployment Manifest

The default deployment manifest runs mcp-k8s in HTTP mode with health checks,
as a non-root user with a read-only root filesystem, no capabilities and the
runtime's default seccomp profile:

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: mcp-k8s
  namespace: mcp-k8s
spec:
  replicas: 1
  selector:
    matchLabels:
      app.kubernetes.io/name: mcp-k8s
  template:
    metadata:
      labels:
        app.kubernetes.io/name: mcp-k8s
    spec:
      serviceAccountName: mcp-k8s
      automountServiceAccountToken: true
      securityContext:
        runAsNonRoot: true
        runAsUser: 65534
        runAsGroup: 65534
        seccompProfile:
          type: RuntimeDefault
      containers:
        - name: mcp-k8s
          image: ghcr.io/alexconrey/mcp-k8s:latest
          securityContext:
            runAsNonRoot: true
            runAsUser: 65534
            runAsGroup: 65534
            allowPrivilegeEscalation: false
            readOnlyRootFilesystem: true
            capabilities:
              drop: ["ALL"]
            seccompProfile:
              type: RuntimeDefault
          command: ["/mcp-k8s", "--http"]
          ports:
            - name: http
              containerPort: 8080
              protocol: TCP
          env:
            - name: MCP_K8S_LISTEN
              value: "0.0.0.0:8080"
          livenessProbe:
            httpGet:
              path: /healthz
              port: http
            periodSeconds: 30
          readinessProbe:
            httpGet:
              path: /healthz
              port: http
            periodSeconds: 10
          resources:
            requests:
              cpu: 50m
              memory: 64Mi
            limits:
              cpu: 200m
              memory: 256Mi
```

## Customizing the Deployment

### Restrict to specific namespaces

Add the `MCP_K8S_NAMESPACES` environment variable:

```yaml
env:
  - name: MCP_K8S_NAMESPACES
    value: "default,production,staging"
```

### Disable mutating operations

Add disable flags:

```yaml
env:
  - name: DISABLE_CREATE
    value: "true"
  - name: DISABLE_UPDATE
    value: "true"
  - name: DISABLE_DELETE
    value: "true"
```

### Disable specific resource actions

```yaml
env:
  - name: MCP_K8S_DISABLE
    value: "deployment-delete,secret-create,namespace-delete"
```

### Set logging level

```yaml
env:
  - name: RUST_LOG
    value: "mcp_k8s=debug"
```

## Verifying the Deployment

```bash
# Check the deployment is running
kubectl get deployment mcp-k8s -n mcp-k8s

# Check the pod is ready
kubectl get pods -n mcp-k8s

# Check the health endpoint
kubectl port-forward svc/mcp-k8s 8080:8080 -n mcp-k8s
curl http://localhost:8080/healthz
# => ok

# Browse the API docs
open http://localhost:8080/swagger-ui
```
