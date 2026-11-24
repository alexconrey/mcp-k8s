# Generic Tools

mcp-k8s provides generic tools that work with any Kubernetes resource type, including Custom Resource Definitions (CRDs). These tools use Kubernetes API discovery to resolve resource types at runtime.

## apply_manifest

**Last resort.** Apply an arbitrary Kubernetes manifest, given as **JSON**, using server-side apply. Accepts any resource type.

> Prefer the typed tools (`create_deployment`, `create_job`, `create_pod`, `create_service`, `create_ingress`, `update_*`, …). They validate their input and cover the common fields (see [Typed Tool Coverage](./coverage.md)). Use `apply_manifest` only for kinds or fields they don't cover.

`apply_manifest` enforces the **same guardrails** as the typed tools (see [Workload Guardrails](../permissions.md#workload-guardrails)):

- the pod rules on every pod template: Pod, PodTemplate, ReplicationController, ReplicaSet, Deployment, StatefulSet, DaemonSet, Job, CronJob;
- the Service, Ingress, RBAC, Secret and Namespace rules;
- the namespace allowlist;
- the same per-resource create/update switches. It looks the object up first: creating needs `<resource>-create`, updating needs `<resource>-update`.

RBAC objects, admission webhooks and policies, CRDs, APIServices, Endpoints/EndpointSlices, ControllerRevisions and every cluster-scoped kind are rejected unless the operator lists them in `--allow-manifest-kinds`. Custom resources need `--allow-custom-resource-groups`.

A manifest without a managed-by label gets `app.kubernetes.io/managed-by: mcp-k8s`. The tool can be removed entirely with `--disable-apply-manifest`.

It returns the applied object's metadata (name, namespace, kind, resource_version).

YAML is **not** accepted: convert it to JSON first (for example `yq -o=json manifest.yaml`). The output of [`get_resource_json`](#get_resource_json) can be edited and passed straight back.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `manifest` | string | Yes | The Kubernetes manifest as a JSON string (YAML is not accepted) |

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

- `apply_manifest` is classified as a **create** action. It is disabled when `--disable-create` is set. Updating an existing object additionally needs update permission for its kind.
- The manifest must be a single JSON object. YAML (including multi-document `---` files) is rejected with a parse error; convert it to JSON and apply one resource per call.
- If the resource already exists, it is updated (server-side apply merges fields).
- For CRDs, the API group must be installed in the cluster for discovery to succeed.
- The RBAC rules for the mcp-k8s service account must include the API groups and resources referenced in the manifest. See [RBAC Setup](../deployment/rbac.md) for details.

---

## get_resource_json

Get any Kubernetes resource as JSON, the object as the API server returns it. Accepts apiVersion, kind, name, and optional namespace. Uses API discovery to resolve the resource type. The output can be edited and passed back to `apply_manifest`; if a person needs YAML, convert it client-side (for example `yq -P`).

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

### `get_resource_yaml` (deprecated)

`get_resource_yaml` is a **deprecated alias** for `get_resource_json`. It takes the same arguments and returns the same **JSON**; despite its name it never returned YAML. It remains for compatibility in 0.4.x and will be removed in the next minor release. Switch to `get_resource_json`.
