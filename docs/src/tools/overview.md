# Tools Overview

mcp-k8s exposes 202 tools covering every GA Kubernetes resource type plus CRDs, multi-cluster management, and resource watching. Each tool corresponds to a specific Kubernetes API operation. Tools are categorized by action type:

- **Read** -- List, get, and inspect resources. Always available regardless of permission configuration.
- **Create** -- Create new resources. Disabled by `--disable-create`.
- **Update** -- Modify existing resources (patch, scale, restart, rollback, cordon, drain). Disabled by `--disable-update`.
- **Delete** -- Remove resources (delete, evict). Disabled by `--disable-delete`.

## Tools by Resource Category

### Core Resources (58 tools)

| Tool | Action | Description |
|------|--------|-------------|
| `get_namespaces` | Read | List all namespaces visible to the server |
| `get_namespace` | Read | Get a namespace by name |
| `create_namespace` | Create | Create a new namespace |
| `update_namespace` | Update | Update a namespace's labels/annotations |
| `delete_namespace` | Delete | Delete a namespace |
| `list_deployments` | Read | List deployments in a namespace |
| `get_deployment` | Read | Get deployment detail with pods and ingresses |
| `get_deployment_history` | Read | List ReplicaSet revision history |
| `create_deployment` | Create | Create a new deployment |
| `update_deployment` | Update | Patch a deployment (image, replicas, env) |
| `delete_deployment` | Delete | Delete a deployment |
| `restart_deployment` | Update | Trigger a rolling restart |
| `scale_deployment` | Update | Scale replica count |
| `rollback_deployment` | Update | Rollback to a specific revision |
| `list_pods` | Read | List pods in a namespace |
| `get_pod` | Read | Get pod detail |
| `get_pod_logs` | Read | Fetch pod logs |
| `create_pod` | Create | Create a standalone pod |
| `delete_pod` | Delete | Delete a pod |
| `evict_pod` | Delete | Evict a pod respecting PDBs |
| `exec_pod` | Read | Execute a command in a pod |
| `list_services` | Read | List services in a namespace |
| `get_service` | Read | Get service detail |
| `create_service` | Create | Create a ClusterIP Service |
| `update_service` | Update | Patch a service |
| `delete_service` | Delete | Delete a service |
| `get_events` | Read | List events in a namespace |
| `get_build_logs` | Read | Fetch logs from a Job's pod |
| `get_metrics` | Read | Get pod CPU/memory metrics |
| `list_configmaps` | Read | List configmaps |
| `get_configmap` | Read | Get configmap with data |
| `create_configmap` | Create | Create a configmap |
| `update_configmap` | Update | Merge-patch configmap data |
| `delete_configmap` | Delete | Delete a configmap |
| `list_secrets` | Read | List secrets (values redacted) |
| `get_secret` | Read | Get secret metadata (optionally decode values) |
| `create_secret` | Create | Create a secret |
| `update_secret` | Update | Merge-patch secret data |
| `delete_secret` | Delete | Delete a secret |
| `list_endpoints` | Read | List endpoints |
| `get_endpoints` | Read | Get endpoints detail |
| `list_pvs` | Read | List PersistentVolumes |
| `get_pv` | Read | Get PV detail |
| `delete_pv` | Delete | Delete a PV |
| `list_pvcs` | Read | List PersistentVolumeClaims |
| `get_pvc` | Read | Get PVC detail |
| `create_pvc` | Create | Create a PVC |
| `update_pvc` | Update | Patch a PVC |
| `delete_pvc` | Delete | Delete a PVC |
| `list_serviceaccounts` | Read | List service accounts |
| `get_serviceaccount` | Read | Get service account detail |
| `create_serviceaccount` | Create | Create a service account |
| `delete_serviceaccount` | Delete | Delete a service account |
| `list_resourcequotas` | Read | List resource quotas |
| `get_resourcequota` | Read | Get resource quota detail |
| `create_resourcequota` | Create | Create a resource quota |
| `update_resourcequota` | Update | Update a resource quota |
| `delete_resourcequota` | Delete | Delete a resource quota |

See [Core Resources](./core.md) for full parameter details.

### Workloads (25 tools)

| Tool | Action | Description |
|------|--------|-------------|
| `list_statefulsets` | Read | List StatefulSets |
| `get_statefulset` | Read | Get StatefulSet detail |
| `create_statefulset` | Create | Create a StatefulSet |
| `update_statefulset` | Update | Patch a StatefulSet |
| `delete_statefulset` | Delete | Delete a StatefulSet |
| `list_daemonsets` | Read | List DaemonSets |
| `get_daemonset` | Read | Get DaemonSet detail |
| `create_daemonset` | Create | Create a DaemonSet |
| `update_daemonset` | Update | Patch a DaemonSet |
| `delete_daemonset` | Delete | Delete a DaemonSet |
| `list_cronjobs` | Read | List CronJobs |
| `get_cronjob` | Read | Get CronJob detail |
| `create_cronjob` | Create | Create a CronJob |
| `update_cronjob` | Update | Patch a CronJob |
| `delete_cronjob` | Delete | Delete a CronJob |
| `list_jobs` | Read | List Jobs |
| `get_job` | Read | Get Job detail |
| `create_job` | Create | Create a Job |
| `delete_job` | Delete | Delete a Job |
| `list_hpas` | Read | List HPAs |
| `get_hpa` | Read | Get HPA detail |
| `create_hpa` | Create | Create an HPA |
| `update_hpa` | Update | Patch an HPA |
| `delete_hpa` | Delete | Delete an HPA |
| `list_replicasets` | Read | List ReplicaSets |
| `get_replicaset` | Read | Get ReplicaSet detail |

See [Workloads](./workloads.md) for full parameter details.

### Networking (16 tools)

| Tool | Action | Description |
|------|--------|-------------|
| `list_ingresses` | Read | List Ingresses |
| `get_ingress` | Read | Get Ingress detail |
| `create_ingress` | Create | Create an Ingress (auto-creates backing Service) |
| `update_ingress` | Update | Patch an Ingress |
| `delete_ingress` | Delete | Delete an Ingress |
| `list_ingressclasses` | Read | List IngressClasses |
| `get_ingressclass` | Read | Get IngressClass detail |
| `list_networkpolicies` | Read | List NetworkPolicies |
| `get_networkpolicy` | Read | Get NetworkPolicy detail |
| `create_networkpolicy` | Create | Create a NetworkPolicy |
| `update_networkpolicy` | Update | Patch a NetworkPolicy |
| `delete_networkpolicy` | Delete | Delete a NetworkPolicy |
| `list_endpointslices` | Read | List EndpointSlices |
| `get_endpointslice` | Read | Get EndpointSlice detail |
| `list_endpoints` | Read | List Endpoints |
| `get_endpoints` | Read | Get Endpoints detail |

See [Networking](./networking.md) for full parameter details.

### Storage (16 tools)

| Tool | Action | Description |
|------|--------|-------------|
| `list_pvs` | Read | List PersistentVolumes |
| `get_pv` | Read | Get PV detail |
| `delete_pv` | Delete | Delete a PV |
| `list_pvcs` | Read | List PersistentVolumeClaims |
| `get_pvc` | Read | Get PVC detail |
| `create_pvc` | Create | Create a PVC |
| `update_pvc` | Update | Patch a PVC |
| `delete_pvc` | Delete | Delete a PVC |
| `list_storageclasses` | Read | List StorageClasses |
| `get_storageclass` | Read | Get StorageClass detail |
| `create_storageclass` | Create | Create a StorageClass |
| `delete_storageclass` | Delete | Delete a StorageClass |
| `list_csidrivers` | Read | List CSI Drivers |
| `get_csidriver` | Read | Get CSI Driver detail |
| `list_csinodes` | Read | List CSI Nodes |
| `list_csistoragecapacities` | Read | List CSI Storage Capacities |
| `list_volumeattachments` | Read | List VolumeAttachments |
| `get_volumeattachment` | Read | Get VolumeAttachment detail |

See [Storage](./storage.md) for full parameter details.

### RBAC & Auth (18 tools)

| Tool | Action | Description |
|------|--------|-------------|
| `can_i` | Read | Check if current user can perform an action |
| `whoami` | Read | Identify the current authenticated user |
| `list_my_permissions` | Read | List current user's permissions in a namespace |
| `list_roles` | Read | List Roles |
| `get_role` | Read | Get Role detail |
| `create_role` | Create | Create a Role |
| `update_role` | Update | Update a Role |
| `delete_role` | Delete | Delete a Role |
| `list_rolebindings` | Read | List RoleBindings |
| `get_rolebinding` | Read | Get RoleBinding detail |
| `create_rolebinding` | Create | Create a RoleBinding |
| `delete_rolebinding` | Delete | Delete a RoleBinding |
| `list_clusterroles` | Read | List ClusterRoles |
| `get_clusterrole` | Read | Get ClusterRole detail |
| `create_clusterrole` | Create | Create a ClusterRole |
| `update_clusterrole` | Update | Update a ClusterRole |
| `delete_clusterrole` | Delete | Delete a ClusterRole |
| `list_clusterrolebindings` | Read | List ClusterRoleBindings |
| `get_clusterrolebinding` | Read | Get ClusterRoleBinding detail |
| `create_clusterrolebinding` | Create | Create a ClusterRoleBinding |
| `delete_clusterrolebinding` | Delete | Delete a ClusterRoleBinding |

See [RBAC & Auth](./rbac.md) for full parameter details.

### Cluster Resources (31 tools)

| Tool | Action | Description |
|------|--------|-------------|
| `list_nodes` | Read | List all cluster nodes |
| `get_node` | Read | Get node detail |
| `get_node_metrics` | Read | Get node CPU/memory metrics |
| `cordon_node` | Update | Mark a node as unschedulable |
| `uncordon_node` | Update | Mark a node as schedulable |
| `drain_node` | Update | Drain pods from a node |
| `list_leases` | Read | List Leases |
| `get_lease` | Read | Get Lease detail |
| `list_pdbs` | Read | List PodDisruptionBudgets |
| `get_pdb` | Read | Get PDB detail |
| `create_pdb` | Create | Create a PDB |
| `update_pdb` | Update | Update a PDB |
| `delete_pdb` | Delete | Delete a PDB |
| `list_limitranges` | Read | List LimitRanges |
| `get_limitrange` | Read | Get LimitRange detail |
| `create_limitrange` | Create | Create a LimitRange |
| `delete_limitrange` | Delete | Delete a LimitRange |
| `list_priorityclasses` | Read | List PriorityClasses |
| `get_priorityclass` | Read | Get PriorityClass detail |
| `list_runtimeclasses` | Read | List RuntimeClasses |
| `get_runtimeclass` | Read | Get RuntimeClass detail |
| `list_flowschemas` | Read | List FlowSchemas |
| `get_flowschema` | Read | Get FlowSchema detail |
| `list_prioritylevelconfigs` | Read | List PriorityLevelConfigurations |
| `get_prioritylevelconfig` | Read | Get PriorityLevelConfiguration detail |
| `list_mutatingwebhookconfigs` | Read | List MutatingWebhookConfigurations |
| `get_mutatingwebhookconfig` | Read | Get MutatingWebhookConfiguration detail |
| `list_validatingwebhookconfigs` | Read | List ValidatingWebhookConfigurations |
| `get_validatingwebhookconfig` | Read | Get ValidatingWebhookConfiguration detail |
| `list_validatingadmissionpolicies` | Read | List ValidatingAdmissionPolicies |
| `get_validatingadmissionpolicy` | Read | Get ValidatingAdmissionPolicy detail |
| `list_csrs` | Read | List CertificateSigningRequests |
| `get_csr` | Read | Get CSR detail |
| `approve_csr` | Update | Approve a CSR |
| `deny_csr` | Update | Deny a CSR |

See [Cluster Resources](./cluster.md) for full parameter details.

### Cluster Management (3 tools)

| Tool | Action | Description |
|------|--------|-------------|
| `list_clusters` | Read | List all configured cluster contexts and show which is active |
| `switch_cluster` | Update | Switch the active cluster context |
| `get_active_cluster` | Read | Get the name of the currently active cluster context |

See [Cluster Resources](./cluster.md) for full parameter details.

### Watch (1 tool)

| Tool | Action | Description |
|------|--------|-------------|
| `watch_resource` | Read | Watch a resource type for ADDED/MODIFIED/DELETED events over a time window |

See [Cluster Resources](./cluster.md) for full parameter details.

### CRD Discovery (7 tools)

| Tool | Action | Description |
|------|--------|-------------|
| `list_crds` | Read | List all CustomResourceDefinitions installed in the cluster |
| `get_crd` | Read | Get a CRD by name with full spec and schema info |
| `list_custom_resources` | Read | List instances of a custom resource via dynamic API discovery |
| `get_custom_resource` | Read | Get a single custom resource by name |
| `create_custom_resource` | Create | Create a custom resource from a JSON manifest |
| `update_custom_resource` | Update | Merge-patch a custom resource |
| `delete_custom_resource` | Delete | Delete a custom resource by name |

See [Cluster Resources](./cluster.md) for full parameter details.

### Generic Tools (2 tools)

| Tool | Action | Description |
|------|--------|-------------|
| `apply_manifest` | Create | Apply any YAML/JSON manifest via server-side apply |
| `get_resource_yaml` | Read | Get any resource as raw JSON via API discovery |

See [Generic Tools](./generic.md) for full parameter details.
