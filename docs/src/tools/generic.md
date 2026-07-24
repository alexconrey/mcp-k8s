# Generic Tools

mcp-k8s provides two generic tools that work with any Kubernetes resource type, including Custom Resource Definitions (CRDs). These tools use Kubernetes API discovery to resolve resource types at runtime.

## apply_manifest

Apply an arbitrary Kubernetes YAML or JSON manifest using server-side apply. Accepts any resource type. Returns the applied object's metadata (name, namespace, kind, resource_version).

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `manifest` | string | Yes | The Kubernetes manifest as a YAML or JSON string |

The manifest string is parsed, the resource type is resolved via API discovery, and the object is applied using server-side apply with the field manager `mcp-k8s`.

### Example: Apply a ConfigMap

```json
{
  "manifest": "apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: my-config\n  namespace: default\ndata:\n  database_url: postgres://localhost:5432/mydb\n  log_level: info"
}
```

### Example: Apply a Deployment

```json
{
  "manifest": "apiVersion: apps/v1\nkind: Deployment\nmetadata:\n  name: web\n  namespace: default\nspec:\n  replicas: 3\n  selector:\n    matchLabels:\n      app: web\n  template:\n    metadata:\n      labels:\n        app: web\n    spec:\n      containers:\n      - name: web\n        image: nginx:1.25\n        ports:\n        - containerPort: 80"
}
```

### Example: Apply a CRD Instance

```json
{
  "manifest": "apiVersion: monitoring.coreos.com/v1\nkind: ServiceMonitor\nmetadata:\n  name: my-app\n  namespace: monitoring\nspec:\n  selector:\n    matchLabels:\n      app: my-app\n  endpoints:\n  - port: metrics\n    interval: 30s"
}
```

### Notes

- `apply_manifest` is classified as a **create** action. It is disabled when `--disable-create` is set.
- The manifest can be YAML or JSON. Multi-document YAML (separated by `---`) is not supported; apply one resource per call.
- If the resource already exists, it is updated (server-side apply merges fields).
- For CRDs, the API group must be installed in the cluster for discovery to succeed.
- The RBAC rules for the mcp-k8s service account must include the API groups and resources referenced in the manifest. See [RBAC Setup](../deployment/rbac.md) for details.

---

## get_resource_yaml

Get any Kubernetes resource as raw JSON output. Accepts apiVersion, kind, name, and optional namespace. Uses API discovery to resolve the resource type.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `api_version` | string | Yes | API version (e.g. `v1`, `apps/v1`, `networking.k8s.io/v1`) |
| `kind` | string | Yes | Resource kind (e.g. `Pod`, `Deployment`, `Service`) |
| `name` | string | Yes | Resource name |
| `namespace` | string | No | Kubernetes namespace (omit for cluster-scoped resources) |

### Example: Get a Deployment

```json
{
  "api_version": "apps/v1",
  "kind": "Deployment",
  "name": "nginx",
  "namespace": "default"
}
```

### Example: Get a Cluster-Scoped Resource

```json
{
  "api_version": "rbac.authorization.k8s.io/v1",
  "kind": "ClusterRole",
  "name": "admin"
}
```

### Example: Get a CRD Instance

```json
{
  "api_version": "monitoring.coreos.com/v1",
  "kind": "ServiceMonitor",
  "name": "my-app",
  "namespace": "monitoring"
}
```

### Notes

- `get_resource_yaml` is classified as a **read** action and is always available regardless of permission configuration.
- Despite the name, the output is JSON (not YAML). The name is kept for compatibility.
- This tool returns the full Kubernetes object, unlike the typed tools which return focused summaries. Use it when you need complete object details or when working with resource types that do not have dedicated tools.
