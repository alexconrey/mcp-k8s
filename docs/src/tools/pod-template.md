# Pod Template Arguments

`create_pod`, `create_job`, `create_cronjob`, `create_deployment`, `create_statefulset` and `create_daemonset` take the same container and pod arguments. One module (`src/resources/podspec.rs`) parses them, so a field means the same thing in every tool. The matching `update_*` tools take the same fields, all optional.

Use these tools rather than `apply_manifest`. They validate input, fill in secure defaults, and run the [workload guardrails](../permissions.md#workload-guardrails). `apply_manifest` runs the same guardrails, so it can't do anything the typed tools refuse.

## Conventions

- **Names:** arguments are snake_case. Kubernetes' camelCase spelling is accepted as an alias at every level (`serviceAccountName`, `securityContext.runAsUser`, `volumeMounts[].mountPath`, …).
- **Unknown fields are rejected.** The error lists the valid fields, so a typo can't silently drop a setting.
- **Pass-through fields:** `affinity`, `topology_spread_constraints`, `dns_config`, and the Job `pod_failure_policy`/`success_policy` take the Kubernetes object as written in the API. snake_case keys are converted.

## One container or several

For a single container, put the container fields at the top level: `image`, `command`, `args`, `env`, …. The container is named after the workload, or `container_name`.

For several containers, use `containers`: a list of container objects, each with `name` and `image`. Don't mix the two forms.

`init_containers` run to completion before the main containers start. An init container with `restart_policy: "Always"` is a **native sidecar**: it starts first and keeps running alongside the main containers.

## Container fields

| Field | Type | Notes |
|-------|------|-------|
| `image` | string | Required. |
| `command`, `args` | string[] | Entrypoint and arguments. |
| `working_dir` | string | |
| `image_pull_policy` | `Always` \| `IfNotPresent` \| `Never` | |
| `env` | object or array | `{NAME: "value"}`, where a value may also be a source object such as `{"secret_key_ref": {"name": "db", "key": "password"}}`. The array form is `[{name, value}]` or `[{name, value_from: {...}}]`. Sources: `secret_key_ref`, `config_map_key_ref` (each `{name, key, optional}`), `field_ref` (`{field_path}`, e.g. `status.podIP`), `resource_field_ref` (`{resource, container_name, divisor}`). |
| `env_from` | array | `[{secret_ref: "name"}]` or `[{config_map_ref: "name"}]`, with optional `prefix` and `optional`. |
| `port` | integer | Top-level shorthand for one port. |
| `ports` | array | Numbers, or `{container_port, name, protocol}`. `host_port` is [gated](#guardrails). |
| `resources` | object | `{requests: {...}, limits: {...}}`. Any resource name works: `cpu`, `memory`, `ephemeral-storage`, `nvidia.com/gpu`, …. |
| `volume_mounts` | array | `{name, mount_path, sub_path, sub_path_expr, read_only, mount_propagation}`. |
| `liveness_probe`, `readiness_probe`, `startup_probe` | object | `probe_type`: `httpGet` (`path`, `port`, `scheme`, `http_headers`), `tcpSocket` (`port`), `exec` (`command`) or `grpc` (`port`, `service`). Timing fields: `initial_delay_seconds`, `period_seconds`, `timeout_seconds`, `failure_threshold`, `success_threshold`, `termination_grace_period_seconds`. `port` is a number or a named container port; the default is 80. |
| `lifecycle` | object | `post_start` and `pre_stop` hooks, each `{handler_type: exec\|httpGet\|sleep, command, path, port, scheme, seconds}`. |
| `security_context` | object | Only inside a `containers[]` entry. Container-level settings, see below. |
| `restart_policy` | `Always` | Only on `init_containers[]` entries; makes a native sidecar. |

## Pod fields

| Field | Type | Notes |
|-------|------|-------|
| `volumes` | array | `name` plus exactly one source: `config_map`, `secret` (`secret_name`), `empty_dir` (`medium`: `""`/`Memory`, `size_limit`), `persistent_volume_claim` (`claim_name`, `read_only`), `projected` (`sources`: `config_map`, `secret`, `service_account_token` with `{path, audience, expiration_seconds}`, `downward_api`), `downward_api`. `host_path`, `nfs` and inline `csi` are [gated](#guardrails). |
| `service_account_name` | string | `service_account` is accepted as an alias. |
| `automount_service_account_token` | boolean | Defaults to `false` unless `service_account_name` is set (see below). |
| `image_pull_secrets` | string[] | Secret names. |
| `node_selector` | object | |
| `tolerations` | array | `{key, operator, value, effect, toleration_seconds}`. |
| `affinity` | object | A Kubernetes `Affinity`: `nodeAffinity`, `podAffinity` or `podAntiAffinity`. |
| `topology_spread_constraints` | array | Kubernetes `TopologySpreadConstraint` objects. |
| `priority_class_name`, `runtime_class_name` | string | |
| `termination_grace_period_seconds` | integer | |
| `dns_policy` | string | `ClusterFirst`, `ClusterFirstWithHostNet`, `Default` or `None`. |
| `dns_config` | object | `{nameservers, searches, options}`. |
| `host_aliases` | array | `{ip, hostnames}` entries for `/etc/hosts`. |
| `hostname`, `subdomain` | string | |
| `share_process_namespace`, `enable_service_links` | boolean | |
| `security_context` | object | Pod-wide settings, see below. |
| `host_network`, `host_pid`, `host_ipc` | boolean | [Gated](#guardrails). |
| `restart_policy` | string | `create_pod`: `Never` (default), `OnFailure`, `Always`. Jobs and CronJobs: `Never` or `OnFailure`; the default is `Never` for Jobs and `OnFailure` for CronJobs. |
| `labels` | object | Applied to the object and its pods. `app: <name>` and `app.kubernetes.io/managed-by: mcp-k8s` are added. |
| `annotations` | object | Object annotations (for `create_pod`, the pod's). |
| `pod_annotations` | object | Pod template annotations. Every workload tool except `create_pod`. |
| `selector` | object | Deployment, StatefulSet and DaemonSet only. Pod selector labels, added to the pod labels. Default: `{app: <name>}`. |

## Security context and secure defaults

The **top-level** `security_context` covers the whole pod:

- **Pod securityContext:** `run_as_non_root`, `run_as_user`, `run_as_group`, `fs_group`, `fs_group_change_policy`, `supplemental_groups`, `seccomp_profile`, `app_armor_profile`, `sysctls`.
- **Default for every container:** `privileged`, `allow_privilege_escalation`, `capabilities` (`add`, `drop`), `read_only_root_filesystem`.

A `containers[]` entry's own `security_context` takes container-level fields and overrides the top-level defaults.

With secure defaults on (the default), any field the caller leaves unset gets:

| Setting | Default | Notes |
|---------|---------|-------|
| `runAsNonRoot` | `true` | Not set when `run_as_user` is 0. |
| `seccompProfile` | `RuntimeDefault` | |
| `allowPrivilegeEscalation` | `false` | Not set for privileged containers or ones that add `SYS_ADMIN`; Kubernetes rejects that combination. |
| `capabilities.drop` | `["ALL"]` | Only for containers that don't run as root. |
| `automountServiceAccountToken` | `false` | Unless `service_account_name` is set. |

**Images that must run as root** (official nginx, redis, postgres, …) need `security_context: {"run_as_non_root": false}`. They then keep the container runtime's default capabilities and still have no privilege escalation. An operator can turn these defaults off with `--no-secure-defaults`.

## Guardrails

The [workload policy](../permissions.md#workload-guardrails) checks every object these tools build. By default it rejects:

- `privileged`, `allowPrivilegeEscalation: true`, and capabilities beyond the runtime's defaults;
- `hostNetwork`/`hostPID`/`hostIPC`, `hostPath`, `hostPort`, and volume types outside the safe set;
- `Unconfined` seccomp and AppArmor profiles, and unsafe sysctls;
- images outside the registry allowlist, when one is configured.

The fields are still part of the schema, so when an operator enables a rule (for example `--allow-privileged` for a node agent), the typed tool can express the workload and there's no need to fall back to `apply_manifest`. Every rejection names the flag that would allow it.

## Updates

`update_deployment`, `update_statefulset`, `update_daemonset` and `update_cronjob` take the same fields. Only the fields you give change; the server applies a strategic merge patch:

- containers, `env`, `ports`, `volumes` and `volume_mounts` merge by name or key;
- `tolerations` and similar lists are replaced.

Container fields go to `container` (by default, the container named after the workload, or the only container). The top-level `security_context` splits as on create: pod fields to the pod, container fields to that container. No defaults are added on update.

Before writing, the update is sent as a **server-side dry run** and the resulting object is checked against the guardrails. The real write carries the dry run's `resourceVersion` as a precondition, so a concurrent change can't slip in between the check and the write.

Because the whole result is checked, an update also refuses to modify a workload that **already** breaks the policy, for example swapping the image of an existing privileged DaemonSet. `rollback_deployment`, `update_deployment_labels`, `update_deployment_annotations` and `update_deployment_affinity` use the same check.

## Example

```json
{
  "namespace": "shop",
  "name": "api",
  "replicas": 3,
  "image": "ghcr.io/acme/api:1.4.2",
  "args": ["--port", "8080"],
  "ports": [{ "container_port": 8080, "name": "http" }],
  "env": {
    "LOG_LEVEL": "info",
    "DB_PASSWORD": { "secret_key_ref": { "name": "db", "key": "password" } },
    "POD_IP": { "field_ref": { "field_path": "status.podIP" } }
  },
  "env_from": [{ "config_map_ref": "api-settings" }],
  "resources": {
    "requests": { "cpu": "250m", "memory": "256Mi", "ephemeral-storage": "1Gi" },
    "limits": { "memory": "512Mi" }
  },
  "readiness_probe": { "probe_type": "httpGet", "path": "/ready", "port": "http" },
  "liveness_probe": { "probe_type": "httpGet", "path": "/healthz", "port": "http", "period_seconds": 20 },
  "lifecycle": { "pre_stop": { "handler_type": "sleep", "seconds": 5 } },
  "volume_mounts": [{ "name": "cache", "mount_path": "/cache" }],
  "volumes": [{ "name": "cache", "empty_dir": { "size_limit": "1Gi" } }],
  "init_containers": [
    { "name": "migrate", "image": "ghcr.io/acme/api:1.4.2", "command": ["api", "migrate"] },
    { "name": "proxy", "image": "ghcr.io/acme/proxy:2", "restart_policy": "Always" }
  ],
  "service_account_name": "api",
  "topology_spread_constraints": [
    { "maxSkew": 1, "topologyKey": "topology.kubernetes.io/zone", "whenUnsatisfiable": "ScheduleAnyway", "labelSelector": { "matchLabels": { "app": "api" } } }
  ],
  "security_context": { "run_as_user": 10001, "fs_group": 10001 },
  "strategy": { "type": "RollingUpdate", "max_surge": "25%", "max_unavailable": 0 }
}
```
