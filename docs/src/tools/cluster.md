# Cluster Resources

This page documents tools for cluster-level resources: Nodes, Leases, PodDisruptionBudgets, LimitRanges, PriorityClasses, RuntimeClasses, FlowControl, Admission webhooks, and CertificateSigningRequests.

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
