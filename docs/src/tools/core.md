# Core Resources

This page documents tools for the most commonly used Kubernetes resources: Deployments, Pods, Services, Events, Namespaces, ConfigMaps, Secrets, Endpoints, ServiceAccounts, and ResourceQuotas.

## Deployments

### list_deployments

List deployments in a namespace with replica counts and status.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `label_selector` | string | No | Label selector (e.g. `app=nginx`) |
| `field_selector` | string | No | Field selector (e.g. `metadata.name=foo`) |

### get_deployment

Get detailed info for a single deployment including pods and ingresses.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | Deployment name |

### get_deployment_history

List revision history (ReplicaSets) for a deployment.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | Deployment name |

### create_deployment

Create a new deployment in the specified namespace.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | Deployment name |
| `image` | string | Yes | Container image |
| `replicas` | integer | No | Number of replicas (default: 1) |
| `port` | integer | No | Container port to expose |
| `env` | object | No | Environment variables as key-value string pairs |

Example:

```json
{
  "namespace": "default",
  "name": "my-app",
  "image": "nginx:1.25",
  "replicas": 3,
  "port": 80,
  "env": {
    "ENV": "production",
    "LOG_LEVEL": "info"
  }
}
```

### update_deployment

Update (merge patch) an existing deployment. Supports changing image, replicas, and env vars.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | Deployment name |
| `image` | string | No | New container image |
| `replicas` | integer | No | New replica count |
| `env` | object | No | Environment variables (replaces all env vars) |

At least one of `image`, `replicas`, or `env` must be provided.

### delete_deployment

Delete a deployment by name from a namespace.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | Deployment name |

### restart_deployment

Trigger a rolling restart by patching the pod template with a `kubectl.kubernetes.io/restartedAt` annotation.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | Deployment name |

### scale_deployment

Scale a deployment to the specified number of replicas.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | Deployment name |
| `replicas` | integer | Yes | Desired replica count |

### rollback_deployment

Rollback a deployment to a specific revision by restoring the pod template from the matching ReplicaSet.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | Deployment name |
| `revision` | integer | Yes | Revision number to rollback to |

---

## Pods

### list_pods

List pods in a namespace. Returns name, phase, readiness, restart count, node, and container statuses.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `label_selector` | string | No | Label selector (e.g. `app=nginx`) |
| `field_selector` | string | No | Field selector (e.g. `status.phase=Running`, `spec.nodeName=node1`) |

### get_pod

Get a pod by name. Returns detailed information including phase, containers, volumes, service account, node selector, tolerations, and annotations.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | Pod name |

### get_pod_logs

Fetch logs from a pod. Optionally scope to a container and limit line count.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `pod_name` | string | Yes | Pod name |
| `tail_lines` | integer | No | Number of recent lines to return |
| `container` | string | No | Container name (defaults to first) |

### create_pod

Create a standalone pod. Useful for debugging, one-shot tasks, or running a quick container.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | Pod name |
| `image` | string | Yes | Container image |
| `command` | string[] | No | Command to run |
| `restart_policy` | string | No | Restart policy: `Never` (default), `Always`, `OnFailure` |

### delete_pod

Delete a pod by name from a namespace.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | Pod name |
| `grace_period_seconds` | integer | No | Grace period before force-killing |

### evict_pod

Evict a pod respecting PodDisruptionBudgets. Safer than delete for production pods.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | Pod name |
| `grace_period_seconds` | integer | No | Grace period in seconds |

### exec_pod

Execute a command in a pod container and return the output.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | Pod name |
| `command` | string[] | Yes | Command to execute (e.g. `["ls", "-la"]`) |
| `container` | string | No | Container name (defaults to first) |

Returns `stdout`, `stderr`, and `exit_code`.

---

## Services

### list_services

List services in a namespace. Returns name, type, cluster IP, external IPs, and ports.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `label_selector` | string | No | Label selector |
| `field_selector` | string | No | Field selector |

### get_service

Get detailed info for a single service including selector, session affinity, and annotations.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | Service name |

### create_service

Create a Kubernetes ClusterIP Service pointing to pods with a matching app label.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | Service name (also used as the app label selector) |
| `port` | integer | No | Service port (default: 80) |
| `target_port` | integer | No | Container port to forward to (defaults to same as port) |

### update_service

Patch a service. Accepts optional fields: ports, selector, type.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | Service name |
| `ports` | array | No | Array of port definitions (`{port, target_port, protocol}`) |
| `selector` | object | No | Label selector for pods |
| `type` | string | No | Service type: `ClusterIP`, `NodePort`, `LoadBalancer`, `ExternalName` |

### delete_service

Delete a service by name from a namespace.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | Service name |

---

## Events

### get_events

List Kubernetes events in a namespace, optionally filtered by resource name and/or label selector.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `resource_name` | string | No | Filter to events involving this resource name |
| `label_selector` | string | No | Label selector |
| `field_selector` | string | No | Field selector (e.g. `involvedObject.kind=Pod`, `reason=Killing`) |

---

## Namespaces

### get_namespaces

List all Kubernetes namespaces visible to the server. Takes no parameters. If a namespace allowlist is configured, only allowed namespaces are returned.

### get_namespace

Get a namespace by name. Returns name, status (Active/Terminating), labels, annotations, created_at, and finalizers.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `name` | string | Yes | Namespace name |

### create_namespace

Create a new Kubernetes namespace.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `name` | string | Yes | Namespace name |
| `labels` | object | No | Labels to apply to the namespace |

### update_namespace

Update a namespace's labels and/or annotations.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `name` | string | Yes | Namespace name |
| `labels` | object | No | Labels to set/update |
| `annotations` | object | No | Annotations to set/update |

### delete_namespace

Delete a namespace by name.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `name` | string | Yes | Namespace name |

---

## ConfigMaps

### list_configmaps

List configmaps in a namespace. Returns name, namespace, data key count, and created_at.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `label_selector` | string | No | Label selector |
| `field_selector` | string | No | Field selector |

### get_configmap

Get a configmap by name. Returns name, namespace, labels, annotations, and data keys with values.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | ConfigMap name |

### create_configmap

Create a configmap in a namespace with the given data key-value pairs.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | ConfigMap name |
| `data` | object | Yes | Key-value string pairs for the configmap data |

### update_configmap

Update (merge patch) a configmap's data. Provided keys are added or overwritten; existing keys not in the patch are preserved.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | ConfigMap name |
| `data` | object | Yes | Key-value string pairs to merge |

### delete_configmap

Delete a configmap by name from a namespace.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | ConfigMap name |

---

## Secrets

> **Note:** Secret values are never returned by default. The `list_secrets` tool only returns metadata and key counts. The `get_secret` tool returns key names by default; pass `decode: true` to include decoded values.

### list_secrets

List secrets in a namespace. Returns name, namespace, type, data key count, and created_at. Secret values are never returned.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `label_selector` | string | No | Label selector |
| `field_selector` | string | No | Field selector (e.g. `type=kubernetes.io/tls`) |

### get_secret

Get a secret by name. Returns metadata and data keys only by default.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | Secret name |
| `decode` | boolean | No | If true, include decoded secret values. Defaults to false. |

### create_secret

Create a Kubernetes Secret. Accepts string_data as key-value pairs.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | Secret name |
| `type` | string | No | Secret type (default: `Opaque`) |
| `string_data` | object | Yes | Key-value pairs of secret data |

### update_secret

Update (merge-patch) a secret's data.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | Secret name |
| `string_data` | object | Yes | Key-value pairs to merge into the secret |

### delete_secret

Delete a Kubernetes Secret by name.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | Secret name |

---

## Metrics

### get_metrics

Get pod resource usage metrics (CPU/memory) from metrics-server.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `label_selector` | string | No | Label selector to scope pods (e.g. `app=foo`) |

### get_build_logs

Fetch logs from a Kubernetes Job's pod (e.g. a build job).

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `job_name` | string | Yes | Job name |

---

## Endpoints

### list_endpoints

List endpoints in a namespace.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `label_selector` | string | No | Label selector |

### get_endpoints

Get endpoints detail by name.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | Endpoints name |

---

## ServiceAccounts

### list_serviceaccounts

List service accounts in a namespace.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `label_selector` | string | No | Label selector |

### get_serviceaccount

Get a service account by name.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | ServiceAccount name |

### create_serviceaccount

Create a service account.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | ServiceAccount name |

### delete_serviceaccount

Delete a service account.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | ServiceAccount name |

---

## ResourceQuotas

### list_resourcequotas

List resource quotas in a namespace.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |

### get_resourcequota

Get a resource quota by name.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | ResourceQuota name |

### create_resourcequota

Create a resource quota.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | ResourceQuota name |
| `hard` | object | Yes | Hard limits (e.g. `{"pods": "10", "requests.cpu": "4"}`) |

### update_resourcequota

Update a resource quota.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | ResourceQuota name |
| `hard` | object | Yes | Updated hard limits |

### delete_resourcequota

Delete a resource quota.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | ResourceQuota name |
