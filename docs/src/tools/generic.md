# Generic Tools

mcp-k8s provides generic tools that work with any Kubernetes resource type, including Custom Resource Definitions (CRDs). These tools use Kubernetes API discovery to resolve resource types at runtime.

## apply_manifest

**Off by default; unrestricted when enabled.** Apply an arbitrary Kubernetes manifest, given as **JSON**, using server-side apply. Accepts any resource type.

> This tool is for people who know exactly what they're applying. It's only listed once the operator starts mcp-k8s with `--enable-apply-manifest`, and when it is, it **skips the [workload guardrails](../permissions.md#workload-guardrails)**: it can create privileged pods, `hostPath` mounts, RBAC bindings, webhooks, CRDs or anything else. Agents should use the typed tools (`create_deployment`, `create_job`, `create_pod`, `create_service`, `create_ingress`, `update_*`, …) and call `apply_manifest` only when the user explicitly asks for a manifest to be applied, after showing them the full manifest.

What still applies when it's enabled:

- the namespace allowlist, on the manifest's `metadata.namespace` and on the `namespace` argument;
- the per-resource create/update switches. It looks the object up first: creating needs `<resource>-create`, updating needs `<resource>-update`.

Its tool definition carries the MCP annotation `destructiveHint: true`, so MCP clients (Claude Code, for example) ask the user before each call. A manifest without a managed-by label gets `app.kubernetes.io/managed-by: mcp-k8s`. See [Permissions: apply_manifest](../permissions.md#apply_manifest) for the reasoning.

It returns the applied object's metadata (name, namespace, kind, resource_version).

YAML is **not** accepted: convert it to JSON first (for example `yq -o=json manifest.yaml`).

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `manifest` | string | Yes | The Kubernetes manifest as a JSON string (YAML is not accepted) |
| `namespace` | string | No | Target namespace for a namespaced kind. Fills `metadata.namespace` when the manifest has none; if both are set they must match, or the call is refused. Checked against the namespace allowlist. |

A namespaced kind needs a namespace from one of the two. Cluster-scoped kinds ignore it (but a namespace outside the allowlist is still refused).

The manifest string is parsed, the resource type is resolved via API discovery, and the object is applied using server-side apply with the field manager `mcp-k8s`.

### Example: Apply a ConfigMap

```json
{
  "manifest": "{\"apiVersion\":\"v1\",\"kind\":\"ConfigMap\",\"metadata\":{\"name\":\"my-config\",\"namespace\":\"default\"},\"data\":{\"database_url\":\"postgres://localhost:5432/mydb\",\"log_level\":\"info\"}}"
}
```

### Example: Apply a Deployment

```json
{
  "manifest": "{\"apiVersion\":\"apps/v1\",\"kind\":\"Deployment\",\"metadata\":{\"name\":\"web\",\"namespace\":\"default\"},\"spec\":{\"replicas\":3,\"selector\":{\"matchLabels\":{\"app\":\"web\"}},\"template\":{\"metadata\":{\"labels\":{\"app\":\"web\"}},\"spec\":{\"containers\":[{\"name\":\"web\",\"image\":\"nginx:1.25\",\"ports\":[{\"containerPort\":80}]}]}}}}"
}
```

### Example: Apply a CRD Instance

Needs `--allow-custom-resource-groups monitoring.coreos.com`.

```json
{
  "manifest": "{\"apiVersion\":\"monitoring.coreos.com/v1\",\"kind\":\"ServiceMonitor\",\"metadata\":{\"name\":\"my-app\",\"namespace\":\"monitoring\"},\"spec\":{\"selector\":{\"matchLabels\":{\"app\":\"my-app\"}},\"endpoints\":[{\"port\":\"metrics\",\"interval\":\"30s\"}]}}"
}
```

### Notes

- `apply_manifest` is off unless `--enable-apply-manifest` is set. It's classified as a **create** action, so `--disable-create` also removes it. Updating an existing object additionally needs update permission for its kind.
- The manifest must be a single JSON object. YAML (including multi-document `---` files) is rejected with a parse error; convert it to JSON and apply one resource per call.
- If the resource already exists, it is updated (server-side apply merges fields).
- For CRDs, the API group must be installed in the cluster for discovery to succeed.
- The RBAC rules for the mcp-k8s service account must include the API groups and resources referenced in the manifest. See [RBAC Setup](../deployment/rbac.md) for details.

---

## get_resource_json

Get any Kubernetes resource as JSON, the object as the API server returns it. Accepts apiVersion, kind, name, and optional namespace. Uses API discovery to resolve the resource type. Use it to inspect an object before changing it with a typed `update_*` tool; if a person needs YAML, convert it client-side (for example `yq -P`).

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

- `get_resource_json` is classified as a **read** action and is always available regardless of permission configuration.
- This tool returns the full Kubernetes object, unlike the typed tools which return focused summaries. Use it when you need complete object details or when working with resource types that do not have dedicated tools.

