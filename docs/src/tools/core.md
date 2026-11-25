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

Create a Deployment whose pods are selected by `app=<name>` (or a custom `selector`). Preferred over `apply_manifest`.

It takes every [pod template argument](./pod-template.md): the single-container shorthand (`image`, `command`, `args`, `env`, `env_from`, `ports`, `resources`, `volume_mounts`, probes, `lifecycle`) or `containers`; `init_containers` (native sidecars); volumes; scheduling; and `security_context`. Plus:

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | Deployment name |
| `image` | string | Yes* | Container image (*or `containers`) |
| `replicas` | integer | No | Number of replicas (default: 1) |
| `port` | integer | No | Container port to expose (shorthand for `ports`) |
| `env` | object/array | No | Environment variables, including secret/configMap/field references |
| `env_from` | array | No | Load all keys of Secrets/ConfigMaps |
| `strategy` | object | No | `{type: RollingUpdate, max_surge, max_unavailable}` or `{type: Recreate}` |
| `min_ready_seconds` | integer | No | |
| `revision_history_limit` | integer | No | |
| `progress_deadline_seconds` | integer | No | |
| `selector` | object | No | Pod selector labels (default `{app: <name>}`) |
| `pod_annotations` | object | No | Pod template annotations |

Pods get the secure defaults (non-root, seccomp RuntimeDefault, no privilege escalation, drop ALL capabilities, no service account token). For images that must run as root, set `security_context: {"run_as_non_root": false}`. Privileged mode, host namespaces/paths/ports and extra capabilities are rejected unless the server allows them.

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
    "API_KEY": { "secret_key_ref": { "name": "my-app", "key": "api-key" } }
  },
  "readiness_probe": { "probe_type": "httpGet", "path": "/", "port": 80 },
  "resources": { "requests": { "cpu": "100m", "memory": "128Mi" } },
  "security_context": { "run_as_non_root": false }
}
```

### update_deployment

Update (strategic merge patch) an existing Deployment. Preferred over `apply_manifest`. It takes the same fields as `create_deployment`, all optional, plus `container` (which container the single-container fields change). Only the given fields change. See [Updates](./pod-template.md#updates).

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | Deployment name |
| `image` | string | No | New container image |
| `replicas` | integer | No | New replica count |
| `env` | object/array | No | Environment variables (merged by name) |
| `env_from` | array | No | envFrom sources |
| `liveness_probe` / `readiness_probe` / `startup_probe` | object | No | Probes |
| `container` | string | No | Target container (default: the one named after the Deployment, or the only one) |
| *any pod template field* | | No | `resources`, `volumes`, `volume_mounts`, `security_context`, `tolerations`, `affinity`, `init_containers`, … |
| `strategy`, `min_ready_seconds`, `revision_history_limit`, `progress_deadline_seconds` | | No | Rollout settings |

The patch is dry-run and its result checked against the guardrails before it is written. That means an update also can't modify a Deployment that already breaks the policy.

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

Rollback a deployment to a specific revision by restoring the pod template from the matching ReplicaSet. The restored template is checked against the guardrails like any update, so rolling back to a revision that predates them can be refused.

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

Create a standalone pod: debugging, one-shot tasks, or a quick container. Preferred over `apply_manifest`. It takes every [pod template argument](./pod-template.md). `restart_policy` is `Never` (default), `OnFailure` or `Always`.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | Pod name |
| `image` | string | Yes* | Container image (*or `containers`) |
| `command` | string[] | No | Command to run |
| `restart_policy` | string | No | Restart policy: `Never` (default), `Always`, `OnFailure` |
| *any pod template field* | | No | `args`, `env`, `ports`, `resources`, probes, `volumes`, `service_account_name`, `security_context`, `init_containers`, … |

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
| `timeout_secs` | integer | No | Seconds to wait for the command (default 60, max 300; larger values are capped) |

Returns `stdout`, `stderr`, `exit_code` and `timed_out`.

- **Timeout.** A command that hasn't finished after `timeout_secs` (for example `tail -f` or `sleep infinity`) returns the output collected so far with `timed_out: true` and a `note` saying so, and mcp-k8s closes the exec session. Without a TTY, whether the container runtime then kills the command is up to the runtime, so avoid commands that never end.
- **Output cap.** Each of `stdout` and `stderr` is capped at 1 MiB; anything beyond is discarded and the text ends with `[output truncated at 1 MiB]`.
- **Node-level access.** `exec_pod` first reads the pod and refuses if the [workload policy](../permissions.md#workload-guardrails) would reject it for a setting that gives node-level access: privileged containers (`--allow-privileged`), `hostNetwork`/`hostPID`/`hostIPC` (`--allow-host-namespaces`), `hostPath` volumes (`--allow-host-path`), added capabilities (`--allow-capabilities`), or Unconfined seccomp/AppArmor, custom SELinux options and `procMount: Unmasked` (`--allow-unconfined`). A command in such a pod runs with node-level access. The error names the flag(s); when the operator has set them, exec is allowed. Creation-only rules (registries, other volume types, host ports, `allowPrivilegeEscalation`) don't block exec. See [Permissions: exec_pod](../permissions.md#exec_pod-and-node-level-access).

`exec_pod` runs arbitrary commands, so it counts as an **update** (`pod-update`): read-only mode and `--disable-update` turn it off.

### port_forward_check

Check whether a pod port can be reached by opening a port-forward tunnel and closing it straight away. Gives up after 10 seconds.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | Pod name |
| `port` | integer | Yes | Container port |

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

Create a Service. Preferred over `apply_manifest`. ClusterIP by default, selecting pods labelled `app=<name>`.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | Service name (also the default `app` selector) |
| `port` | integer | No | Shorthand for a single port (default: 80) |
| `target_port` | integer/string | No | Target for the shorthand (default: `port`) |
| `ports` | array | No | `{port, target_port (number or container port name), name, protocol, app_protocol, node_port}` |
| `selector` | object | No | Pod selector (default `{app: <name>}`) |
| `type` | string | No | `ClusterIP` (default); `NodePort`, `LoadBalancer`, `ExternalName` are gated |
| `headless` / `cluster_ip` | boolean / string | No | `headless: true` = `clusterIP: None` |
| `session_affinity`, `session_affinity_timeout_seconds` | | No | `None` or `ClientIP` |
| `external_traffic_policy`, `internal_traffic_policy` | string | No | `Cluster` or `Local` |
| `traffic_distribution` | string | No | e.g. `PreferClose` |
| `ip_families`, `ip_family_policy` | | No | Dual-stack settings |
| `publish_not_ready_addresses` | boolean | No | |
| `load_balancer_source_ranges`, `load_balancer_class`, `allocate_load_balancer_node_ports` | | No | Need `--allow-loadbalancer` |
| `external_name`, `external_ips` | | No | Need `--allow-external-services` |
| `labels`, `annotations` | object | No | Cloud load-balancer annotations need `--allow-loadbalancer` |

### update_service

Patch a Service. Preferred over `apply_manifest`. It takes the same fields as `create_service` except the immutable ones (`headless`, `cluster_ip`, IP families). `ports` merge by port number. The result is dry-run and checked against the guardrails, so an update can't switch a Service to a disallowed type.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | Service name |
| `ports` | array | No | Port definitions (`{port, target_port, name, protocol, app_protocol, node_port}`) |
| `selector` | object | No | Label selector for pods |
| `type` | string | No | `ClusterIP`, `NodePort`, `LoadBalancer`, `ExternalName` (gated as on create) |
| *other create_service fields* | | No | Traffic policies, affinity, annotations, labels, … |

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

Create a new Kubernetes namespace. Labelling it `pod-security.kubernetes.io/{enforce,audit,warn}=privileged` turns off Pod Security Admission for the namespace and needs `--allow-privileged`.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `name` | string | Yes | Namespace name |
| `labels` | object | No | Labels to apply to the namespace |

### update_namespace

Update a namespace's labels and/or annotations. The result is dry-run and checked: the pod-security labels can't be set to `privileged` without `--allow-privileged`.

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
| `labels` | object | No | Labels (`app.kubernetes.io/managed-by: mcp-k8s` is added) |
| `annotations` | object | No | Annotations |
| `binary_data` | object | No | Base64-encoded binary values (`data` or `binary_data` is required) |
| `immutable` | boolean | No | Make the ConfigMap immutable |

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

Create a Kubernetes Secret. Accepts string_data as key-value pairs. `type: kubernetes.io/service-account-token` mints a long-lived token for a service account and needs `--allow-rbac-escalation`.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | Secret name |
| `type` | string | No | Secret type (default: `Opaque`) |
| `string_data` | object | Yes | Key-value pairs of secret data |
| `labels` | object | No | Labels (`app.kubernetes.io/managed-by: mcp-k8s` is added) |
| `annotations` | object | No | Annotations |
| `data` | object | No | Base64-encoded values (`string_data` or `data` is required) |
| `immutable` | boolean | No | Make the Secret immutable |

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
| `labels` | object | No | Labels |
| `image_pull_secrets` | string[] | No | Image pull Secrets for pods using this service account |

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
