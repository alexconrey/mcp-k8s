# Workloads

This page documents tools for workload resources: StatefulSets, DaemonSets, CronJobs, Jobs, HorizontalPodAutoscalers, and ReplicaSets.

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

Create a new StatefulSet in the specified namespace.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | StatefulSet name |
| `image` | string | Yes | Container image |
| `service_name` | string | Yes | Governing headless service name |
| `replicas` | integer | No | Number of replicas (default: 1) |
| `port` | integer | No | Container port to expose |

Example:

```json
{
  "namespace": "default",
  "name": "postgres",
  "image": "postgres:16",
  "service_name": "postgres-headless",
  "replicas": 3,
  "port": 5432
}
```

### update_statefulset

Update (patch) an existing StatefulSet. Supports changing image and replica count.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | StatefulSet name |
| `image` | string | No | New container image |
| `replicas` | integer | No | New replica count |

At least one of `image` or `replicas` must be provided.

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

Create a DaemonSet that runs a pod on all (or selected) nodes.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | DaemonSet name |
| `image` | string | Yes | Container image |
| `port` | integer | No | Container port |

### update_daemonset

Patch a DaemonSet. Supports updating the container image.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | DaemonSet name |
| `image` | string | No | New container image |

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

Create a CronJob.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | CronJob name |
| `image` | string | Yes | Container image |
| `schedule` | string | Yes | Cron schedule expression (e.g. `*/5 * * * *`) |
| `command` | string[] | No | Command to run |

### update_cronjob

Patch a CronJob. Supports updating image, schedule, and suspend state.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | CronJob name |
| `image` | string | No | New container image |
| `schedule` | string | No | New cron schedule |
| `suspend` | boolean | No | Suspend or resume the CronJob |

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

Create a Job.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | Job name |
| `image` | string | Yes | Container image |
| `command` | string[] | No | Command to run |

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
