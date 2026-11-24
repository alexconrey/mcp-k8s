# Workloads

This page documents tools for workload resources: StatefulSets, DaemonSets, CronJobs, Jobs, HorizontalPodAutoscalers, and ReplicaSets. Deployments and Pods are on [Core Resources](./core.md).

Every workload create/update tool takes the shared [pod template arguments](./pod-template.md): containers, init containers and native sidecars, env with secret/configMap/field references, probes, resources, volumes, scheduling and security settings. Each section below lists only the tool's own fields. Prefer these tools over `apply_manifest`; both enforce the same [guardrails](../permissions.md#workload-guardrails).

## StatefulSets

### list_statefulsets

List StatefulSets in a namespace with replica counts and status.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `label_selector` | string | No | Label selector (e.g. `app=postgres`) |

### get_statefulset

Get detailed info for a single StatefulSet including conditions, service name, update strategy, and volume claim templates.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | StatefulSet name |

### create_statefulset

Create a StatefulSet (pods selected by `app=<name>` or a custom `selector`). It takes the [pod template arguments](./pod-template.md) plus:

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | StatefulSet name |
| `image` | string | Yes* | Container image (*or `containers`) |
| `service_name` | string | Yes | Governing headless service name |
| `replicas` | integer | No | Number of replicas (default: 1) |
| `port` | integer | No | Container port to expose |
| `volume_claim_templates` | array | No | Per-pod PVCs: `{name, storage, storage_class_name, access_modes (default [ReadWriteOnce]), volume_mode, labels, annotations}`. Mount them by name in `volume_mounts`. |
| `pod_management_policy` | string | No | `OrderedReady` (default) or `Parallel` |
| `update_strategy` | object | No | `{type: RollingUpdate, partition, max_unavailable}` or `{type: OnDelete}` |
| `persistent_volume_claim_retention_policy` | object | No | `{when_deleted, when_scaled}`: `Retain` or `Delete` |
| `min_ready_seconds`, `revision_history_limit` | integer | No | |
| `selector`, `pod_annotations` | object | No | |

Example:

```json
{
  "namespace": "default",
  "name": "postgres",
  "image": "postgres:16",
  "service_name": "postgres-headless",
  "replicas": 3,
  "port": 5432,
  "env": { "POSTGRES_PASSWORD": { "secret_key_ref": { "name": "pg", "key": "password" } } },
  "volume_claim_templates": [{ "name": "data", "storage": "10Gi" }],
  "volume_mounts": [{ "name": "data", "mount_path": "/var/lib/postgresql/data" }],
  "security_context": { "run_as_non_root": false, "fs_group": 999 }
}
```

### update_statefulset

Update (patch) an existing StatefulSet: `replicas`, `update_strategy`, `persistent_volume_claim_retention_policy`, `min_ready_seconds`, `revision_history_limit`, and any pod template field (`image`, `env`, `resources`, probes, …). `container` picks the target container. The result is dry-run and checked against the guardrails before it is written (see [Updates](./pod-template.md#updates)).

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | StatefulSet name |
| `image` | string | No | New container image |
| `replicas` | integer | No | New replica count |
| *other fields* | | No | As above |

### delete_statefulset

Delete a StatefulSet by name.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | StatefulSet name |

---

## DaemonSets

### list_daemonsets

List DaemonSets in a namespace with image, node counts, and labels.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `label_selector` | string | No | Label selector |

### get_daemonset

Get detailed info for a single DaemonSet including conditions, update strategy, node selector, tolerations, and annotations.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | DaemonSet name |

### create_daemonset

Create a DaemonSet that runs a pod on all (or selected) nodes. It takes the [pod template arguments](./pod-template.md); use `node_selector`, `affinity` and `tolerations` to pick nodes. Plus:

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | DaemonSet name |
| `image` | string | Yes* | Container image (*or `containers`) |
| `port` | integer | No | Container port |
| `update_strategy` | object | No | `{type: RollingUpdate, max_unavailable, max_surge}` or `{type: OnDelete}` |
| `min_ready_seconds`, `revision_history_limit` | integer | No | |
| `selector`, `pod_annotations` | object | No | |

Node agents often need `hostPath`, `hostNetwork` or privileged mode. Those are rejected unless the operator enables `--allow-host-path`, `--allow-host-namespaces` or `--allow-privileged`. With the flag set, this tool can express them.

### update_daemonset

Patch a DaemonSet: `update_strategy`, `min_ready_seconds`, `revision_history_limit`, and any pod template field. The result is dry-run and checked against the guardrails, so an image change on an existing privileged DaemonSet is refused unless the operator allows privileged workloads.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | DaemonSet name |
| `image` | string | No | New container image |
| *other fields* | | No | As above |

### delete_daemonset

Delete a DaemonSet by name.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | DaemonSet name |

---

## CronJobs

### list_cronjobs

List CronJobs in a namespace.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `label_selector` | string | No | Label selector |

### get_cronjob

Get detailed info for a single CronJob including schedule, last schedule time, and active jobs.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | CronJob name |

### create_cronjob

Create a CronJob. It takes the [pod template arguments](./pod-template.md) (`restart_policy` defaults to `OnFailure`), every Job setting from [`create_job`](#create_job) for the job template, plus:

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | CronJob name |
| `schedule` | string | Yes | Cron schedule expression (e.g. `*/5 * * * *`) |
| `image` | string | Yes* | Container image (*or `containers`) |
| `command` | string[] | No | Command to run |
| `restart_policy` | string | No | `OnFailure` (default) or `Never` |
| `time_zone` | string | No | IANA time zone, e.g. `Europe/Berlin` |
| `concurrency_policy` | string | No | `Allow` (default), `Forbid`, `Replace` |
| `suspend` | boolean | No | Suspend future runs |
| `starting_deadline_seconds` | integer | No | |
| `successful_jobs_history_limit`, `failed_jobs_history_limit` | integer | No | |

### update_cronjob

Update (patch) an existing CronJob: `schedule`, `suspend`, `time_zone`, `concurrency_policy`, history limits, job settings, and any pod template field (`image`, `env`, …). The result is dry-run and checked against the guardrails before it is written.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | CronJob name |
| `schedule` | string | No | New cron schedule expression |
| `suspend` | boolean | No | Whether to suspend the cronjob |
| `image` | string | No | New container image |
| *other fields* | | No | As above |

### delete_cronjob

Delete a CronJob.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | CronJob name |

---

## Jobs

### list_jobs

List Jobs in a namespace.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `label_selector` | string | No | Label selector |

### get_job

Get detailed info for a single Job including status, completions, and conditions.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | Job name |

### create_job

Create a Job. It takes the [pod template arguments](./pod-template.md) (`restart_policy`: `Never` by default, or `OnFailure`) plus:

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | Job name |
| `image` | string | Yes* | Container image (*or `containers`) |
| `command` | string[] | No | Command to run |
| `restart_policy` | string | No | `Never` (default) or `OnFailure` |
| `backoff_limit` | integer | No | Retries before failure (default: 6) |
| `ttl_seconds_after_finished` | integer | No | Delete the Job this long after it finishes |
| `active_deadline_seconds` | integer | No | Fail the Job after this long |
| `completions`, `parallelism` | integer | No | |
| `completion_mode` | string | No | `NonIndexed` or `Indexed` |
| `backoff_limit_per_index`, `max_failed_indexes` | integer | No | Indexed Jobs |
| `pod_failure_policy` | object | No | Kubernetes PodFailurePolicy (`{rules: [{action, onExitCodes \| onPodConditions}]}`) |
| `success_policy` | object | No | Kubernetes SuccessPolicy for Indexed Jobs |
| `pod_replacement_policy` | string | No | `TerminatingOrFailed` or `Failed` |
| `suspend` | boolean | No | Create the Job suspended |

### create_job_from_cronjob

Run a CronJob now: create a Job from its job template, like `kubectl create job --from=cronjob/NAME`. The Job is owned by the CronJob and annotated `cronjob.kubernetes.io/instantiate: manual`. The template is checked against the guardrails like any workload.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `cronjob` | string | Yes | CronJob name |
| `name` | string | No | Job name (default: `<cronjob>-manual-<timestamp>`) |

### delete_job

Delete a Job.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | Job name |

---

## HorizontalPodAutoscalers (HPAs)

### list_hpas

List HPAs in a namespace.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `label_selector` | string | No | Label selector |

### get_hpa

Get detailed info for a single HPA including current/target metrics and scaling status.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | HPA name |

### create_hpa

Create an HPA targeting a deployment.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | HPA name |
| `target_deployment` | string | Yes | Name of the deployment to scale |
| `min_replicas` | integer | No | Minimum replicas (default: 1) |
| `max_replicas` | integer | Yes | Maximum replicas |
| `cpu_target_percent` | integer | No | Target CPU utilization percentage |

### update_hpa

Patch an HPA. Supports updating min/max replicas and CPU target.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | HPA name |
| `min_replicas` | integer | No | New minimum replicas |
| `max_replicas` | integer | No | New maximum replicas |
| `cpu_target_percent` | integer | No | New CPU target percentage |

### delete_hpa

Delete an HPA.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | HPA name |

---

## ReplicaSets

ReplicaSets are typically managed by Deployments. These tools are useful for inspecting deployment history and revision details.

### list_replicasets

List ReplicaSets in a namespace.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `label_selector` | string | No | Label selector |

### get_replicaset

Get detailed info for a single ReplicaSet.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | ReplicaSet name |
