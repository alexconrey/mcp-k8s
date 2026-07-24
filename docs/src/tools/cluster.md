# Cluster Resources

This page documents tools for cluster-level resources: Nodes, Leases, PodDisruptionBudgets, LimitRanges, PriorityClasses, RuntimeClasses, FlowControl, Admission webhooks, CertificateSigningRequests, Cluster Management, Watch, and CRD Discovery.

## Nodes

### list_nodes

List all nodes in the cluster with status, roles, capacity, and version info.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `label_selector` | string | No | Label selector (e.g. `kubernetes.io/os=linux`) |
| `field_selector` | string | No | Field selector (e.g. `metadata.name=node1`) |

### get_node

Get detailed information for a single node including conditions, addresses, allocatable resources, taints, labels, and annotations.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `name` | string | Yes | Node name |

### get_node_metrics

Get node-level CPU and memory usage metrics from metrics-server.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `name` | string | No | Specific node name (omit to list all) |

### cordon_node

Mark a node as unschedulable. No new pods will be scheduled on it, but existing pods continue running.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `name` | string | Yes | Node name |

### uncordon_node

Mark a node as schedulable again.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `name` | string | Yes | Node name |

### drain_node

Drain pods from a node. Evicts pods respecting PodDisruptionBudgets and cordons the node. DaemonSet pods are skipped by default.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `name` | string | Yes | Node name |
| `grace_period_seconds` | integer | No | Grace period for pod termination |
| `ignore_daemonsets` | boolean | No | Ignore DaemonSet pods (default: true) |

---

## Leases

### list_leases

List Leases in a namespace. Leases are used for leader election and node heartbeats.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |

### get_lease

Get detailed info for a single Lease.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | Lease name |

---

## PodDisruptionBudgets (PDBs)

### list_pdbs

List PodDisruptionBudgets in a namespace.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |

### get_pdb

Get detailed info for a single PDB.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | PDB name |

### create_pdb

Create a PodDisruptionBudget.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | PDB name |
| `selector` | object | Yes | Label selector for target pods |
| `min_available` | string | No | Minimum available pods (number or percentage) |
| `max_unavailable` | string | No | Maximum unavailable pods (number or percentage) |

### update_pdb

Update a PodDisruptionBudget.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | PDB name |
| `min_available` | string | No | Updated minimum available |
| `max_unavailable` | string | No | Updated maximum unavailable |

### delete_pdb

Delete a PodDisruptionBudget.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | PDB name |

---

## LimitRanges

### list_limitranges

List LimitRanges in a namespace.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |

### get_limitrange

Get detailed info for a single LimitRange.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | LimitRange name |

### create_limitrange

Create a LimitRange.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | LimitRange name |
| `limits` | array | Yes | Array of limit items with type, default, defaultRequest, max, min |

### delete_limitrange

Delete a LimitRange.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | LimitRange name |

---

## PriorityClasses

### list_priorityclasses

List PriorityClasses in the cluster.

No required parameters.

### get_priorityclass

Get detailed info for a single PriorityClass.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `name` | string | Yes | PriorityClass name |

---

## RuntimeClasses

### list_runtimeclasses

List RuntimeClasses in the cluster.

No required parameters.

### get_runtimeclass

Get detailed info for a single RuntimeClass.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `name` | string | Yes | RuntimeClass name |

---

## FlowControl

### list_flowschemas

List FlowSchemas. FlowSchemas define how API requests are classified into flow categories for priority-based throttling.

No required parameters.

### get_flowschema

Get detailed info for a single FlowSchema.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `name` | string | Yes | FlowSchema name |

### list_prioritylevelconfigs

List PriorityLevelConfigurations. These define the priority and queuing behavior for different flow categories.

No required parameters.

### get_prioritylevelconfig

Get detailed info for a single PriorityLevelConfiguration.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `name` | string | Yes | PriorityLevelConfiguration name |

---

## Admission Webhooks

### list_mutatingwebhookconfigs

List MutatingWebhookConfigurations.

No required parameters.

### get_mutatingwebhookconfig

Get detailed info for a single MutatingWebhookConfiguration.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `name` | string | Yes | MutatingWebhookConfiguration name |

### list_validatingwebhookconfigs

List ValidatingWebhookConfigurations.

No required parameters.

### get_validatingwebhookconfig

Get detailed info for a single ValidatingWebhookConfiguration.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `name` | string | Yes | ValidatingWebhookConfiguration name |

### list_validatingadmissionpolicies

List ValidatingAdmissionPolicies.

No required parameters.

### get_validatingadmissionpolicy

Get detailed info for a single ValidatingAdmissionPolicy.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `name` | string | Yes | ValidatingAdmissionPolicy name |

---

## CertificateSigningRequests (CSRs)

### list_csrs

List CertificateSigningRequests in the cluster.

No required parameters.

### get_csr

Get detailed info for a single CSR.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `name` | string | Yes | CSR name |

### approve_csr

Approve a CertificateSigningRequest.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `name` | string | Yes | CSR name |

### deny_csr

Deny a CertificateSigningRequest.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `name` | string | Yes | CSR name |

---

## Cluster Management

These tools allow switching between multiple Kubernetes cluster contexts at runtime. Multi-cluster support is enabled by passing `--contexts` (or `MCP_K8S_CONTEXTS`) with a comma-separated list of kubeconfig context names. See [Configuration](../getting-started/configuration.md) for details.

### list_clusters

List all configured cluster contexts and indicate which is currently active.

No required parameters.

Example response:

```
* staging (active)
  production
```

### switch_cluster

Switch the active cluster context. All subsequent tool calls will operate against the new cluster.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `name` | string | Yes | Cluster context name to switch to |

Example:

```json
{
  "name": "switch_cluster",
  "arguments": { "name": "production" }
}
```

### get_active_cluster

Get the name of the currently active cluster context.

No required parameters.

Returns the context name as a plain string (e.g. `staging`).

---

## Watch

### watch_resource

Watch a Kubernetes resource type for changes over a specified duration. Since MCP tools are request/response, this collects all ADDED, MODIFIED, and DELETED events during the watch window and returns them as a batch.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `api_version` | string | Yes | API version (e.g. `v1`, `apps/v1`, `networking.k8s.io/v1`) |
| `kind` | string | Yes | Resource kind (e.g. `Pod`, `Deployment`, `Service`) |
| `namespace` | string | No | Kubernetes namespace. Omit for cluster-scoped resources or to watch across all namespaces. |
| `label_selector` | string | No | Label selector to filter watched resources (e.g. `app=nginx`) |
| `duration_seconds` | integer | No | How long to watch in seconds (default: 10, max: 60) |

Example:

```json
{
  "name": "watch_resource",
  "arguments": {
    "api_version": "apps/v1",
    "kind": "Deployment",
    "namespace": "default",
    "duration_seconds": 15
  }
}
```

Example response:

```json
{
  "api_version": "apps/v1",
  "kind": "Deployment",
  "namespace": "default",
  "duration_seconds": 15,
  "events_count": 2,
  "events": [
    { "type": "MODIFIED", "name": "nginx", "namespace": "default" },
    { "type": "ADDED", "name": "hello-world", "namespace": "default" }
  ]
}
```

---

## CRD Discovery

These tools provide full lifecycle management of CustomResourceDefinitions and their instances. CRD discovery uses the Kubernetes API discovery mechanism to dynamically resolve resource types, so any installed CRD is supported without code changes.

### list_crds

List all CustomResourceDefinitions installed in the cluster. Returns name, group, kind, scope, versions, and creation timestamp for each CRD.

No required parameters.

### get_crd

Get a CustomResourceDefinition by name. Returns the full spec including group, names, scope, and versions with schema information.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `name` | string | Yes | CRD name (e.g. `crontabs.stable.example.com`) |

### list_custom_resources

List instances of a custom resource. Uses dynamic API discovery to resolve the resource type from group/version/kind.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `group` | string | Yes | API group of the custom resource (e.g. `stable.example.com`) |
| `version` | string | Yes | API version (e.g. `v1`, `v1alpha1`) |
| `kind` | string | Yes | Resource kind (e.g. `CronTab`) |
| `namespace` | string | No | Kubernetes namespace. Omit for cluster-scoped custom resources. |

Example:

```json
{
  "name": "list_custom_resources",
  "arguments": {
    "group": "stable.example.com",
    "version": "v1",
    "kind": "CronTab",
    "namespace": "default"
  }
}
```

### get_custom_resource

Get a single custom resource by name.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `group` | string | Yes | API group of the custom resource |
| `version` | string | Yes | API version |
| `kind` | string | Yes | Resource kind |
| `name` | string | Yes | Resource name |
| `namespace` | string | No | Kubernetes namespace. Omit for cluster-scoped custom resources. |

### create_custom_resource

Create a custom resource from a JSON manifest.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `group` | string | Yes | API group of the custom resource |
| `version` | string | Yes | API version |
| `kind` | string | Yes | Resource kind |
| `manifest` | object | Yes | Full resource manifest (must include apiVersion, kind, metadata, and spec) |
| `namespace` | string | No | Kubernetes namespace. Omit for cluster-scoped custom resources. |

### update_custom_resource

Merge-patch a custom resource.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `group` | string | Yes | API group of the custom resource |
| `version` | string | Yes | API version |
| `kind` | string | Yes | Resource kind |
| `name` | string | Yes | Resource name |
| `patch` | object | Yes | JSON merge-patch object to apply to the resource |
| `namespace` | string | No | Kubernetes namespace. Omit for cluster-scoped custom resources. |

Example:

```json
{
  "name": "update_custom_resource",
  "arguments": {
    "group": "stable.example.com",
    "version": "v1",
    "kind": "CronTab",
    "name": "my-crontab",
    "namespace": "default",
    "patch": { "spec": { "cronSpec": "*/10 * * * *" } }
  }
}
```

### delete_custom_resource

Delete a custom resource by name.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `group` | string | Yes | API group of the custom resource |
| `version` | string | Yes | API version |
| `kind` | string | Yes | Resource kind |
| `name` | string | Yes | Resource name |
| `namespace` | string | No | Kubernetes namespace. Omit for cluster-scoped custom resources. |
