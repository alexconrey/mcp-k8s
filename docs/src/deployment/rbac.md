# RBAC Setup

mcp-k8s needs a Kubernetes ServiceAccount with appropriate RBAC permissions to interact with the cluster. The scope of permissions depends on which tools you want to expose. This page provides ClusterRole definitions for full access, read-only access, and per-namespace access.

## Full Access ClusterRole

This ClusterRole grants all permissions required by every mcp-k8s tool. Apply this when running mcp-k8s with full create/update/delete capabilities.

```yaml
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRole
metadata:
  name: mcp-k8s
rules:
  # --------------------------------------------------------------------------
  # Core API ("")
  # --------------------------------------------------------------------------

  # Pods: list_pods, get_pod, create_pod, delete_pod, get_pod_logs, drain_node
  # Services: list_services, get_service, create_service, update_service, delete_service
  # ConfigMaps: list_configmaps, get_configmap, create_configmap, update_configmap, delete_configmap
  # Secrets: list_secrets, get_secret, create_secret, update_secret, delete_secret
  # Namespaces: get_namespaces, get_namespace, create_namespace, update_namespace, delete_namespace
  # Events: get_events
  # Endpoints: list_endpoints, get_endpoints
  # PersistentVolumes: list_pvs, get_pv, delete_pv
  # PersistentVolumeClaims: list_pvcs, get_pvc, create_pvc, update_pvc, delete_pvc
  # ServiceAccounts: list_serviceaccounts, get_serviceaccount, create_serviceaccount, delete_serviceaccount
  # Nodes: list_nodes, get_node, cordon_node, uncordon_node, drain_node
  # ResourceQuotas: list_resourcequotas, get_resourcequota, create_resourcequota, update_resourcequota, delete_resourcequota
  # LimitRanges: list_limitranges, get_limitrange, create_limitrange, delete_limitrange
  - apiGroups: [""]
    resources:
      - pods
      - services
      - configmaps
      - secrets
      - namespaces
      - events
      - endpoints
      - persistentvolumes
      - persistentvolumeclaims
      - serviceaccounts
      - nodes
      - resourcequotas
      - limitranges
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]

  # Pod logs: get_pod_logs, get_build_logs
  - apiGroups: [""]
    resources:
      - pods/log
    verbs: ["get"]

  # Pod eviction: drain_node evicts pods via the eviction subresource
  - apiGroups: [""]
    resources:
      - pods/eviction
    verbs: ["create"]

  # Node status: cordon_node and uncordon_node patch spec.unschedulable
  - apiGroups: [""]
    resources:
      - nodes/status
    verbs: ["patch"]

  # --------------------------------------------------------------------------
  # apps
  # --------------------------------------------------------------------------

  # Deployments: list_deployments, get_deployment, create_deployment, update_deployment,
  #   delete_deployment, restart_deployment, scale_deployment, rollback_deployment,
  #   get_deployment_history
  # StatefulSets: list_statefulsets, get_statefulset, create_statefulset, update_statefulset, delete_statefulset
  # DaemonSets: list_daemonsets, get_daemonset, create_daemonset, update_daemonset, delete_daemonset
  # ReplicaSets: list_replicasets, get_replicaset (also used by rollback_deployment)
  - apiGroups: ["apps"]
    resources:
      - deployments
      - statefulsets
      - daemonsets
      - replicasets
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]

  # --------------------------------------------------------------------------
  # batch
  # --------------------------------------------------------------------------

  # Jobs: list_jobs, get_job, create_job, delete_job, get_build_logs
  # CronJobs: list_cronjobs, get_cronjob, create_cronjob, update_cronjob, delete_cronjob
  - apiGroups: ["batch"]
    resources:
      - jobs
      - cronjobs
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]

  # --------------------------------------------------------------------------
  # networking.k8s.io
  # --------------------------------------------------------------------------

  # Ingresses: list_ingresses, get_ingress, create_ingress, update_ingress, delete_ingress
  # IngressClasses: list_ingressclasses, get_ingressclass
  # NetworkPolicies: list_networkpolicies, get_networkpolicy, create_networkpolicy,
  #   update_networkpolicy, delete_networkpolicy
  - apiGroups: ["networking.k8s.io"]
    resources:
      - ingresses
      - ingressclasses
      - networkpolicies
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]

  # --------------------------------------------------------------------------
  # rbac.authorization.k8s.io
  # --------------------------------------------------------------------------

  # Roles: list_roles, get_role, create_role, update_role, delete_role
  # RoleBindings: list_rolebindings, get_rolebinding, create_rolebinding, delete_rolebinding
  # ClusterRoles: list_clusterroles, get_clusterrole, create_clusterrole, update_clusterrole, delete_clusterrole
  # ClusterRoleBindings: list_clusterrolebindings, get_clusterrolebinding,
  #   create_clusterrolebinding, delete_clusterrolebinding
  - apiGroups: ["rbac.authorization.k8s.io"]
    resources:
      - roles
      - rolebindings
      - clusterroles
      - clusterrolebindings
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]

  # --------------------------------------------------------------------------
  # autoscaling
  # --------------------------------------------------------------------------

  # HPAs: list_hpas, get_hpa, create_hpa, update_hpa, delete_hpa
  - apiGroups: ["autoscaling"]
    resources:
      - horizontalpodautoscalers
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]

  # --------------------------------------------------------------------------
  # policy
  # --------------------------------------------------------------------------

  # PDBs: list_pdbs, get_pdb, create_pdb, update_pdb, delete_pdb
  - apiGroups: ["policy"]
    resources:
      - poddisruptionbudgets
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]

  # --------------------------------------------------------------------------
  # storage.k8s.io
  # --------------------------------------------------------------------------

  # StorageClasses: list_storageclasses, get_storageclass, create_storageclass, delete_storageclass
  # CSIDrivers: list_csidrivers, get_csidriver
  # CSINodes: list_csinodes
  # CSIStorageCapacities: list_csistoragecapacities
  # VolumeAttachments: list_volumeattachments, get_volumeattachment
  - apiGroups: ["storage.k8s.io"]
    resources:
      - storageclasses
      - csidrivers
      - csinodes
      - csistoragecapacities
      - volumeattachments
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]

  # --------------------------------------------------------------------------
  # coordination.k8s.io
  # --------------------------------------------------------------------------

  # Leases: list_leases, get_lease
  - apiGroups: ["coordination.k8s.io"]
    resources:
      - leases
    verbs: ["get", "list", "watch"]

  # --------------------------------------------------------------------------
  # discovery.k8s.io
  # --------------------------------------------------------------------------

  # EndpointSlices: list_endpointslices, get_endpointslice
  - apiGroups: ["discovery.k8s.io"]
    resources:
      - endpointslices
    verbs: ["get", "list", "watch"]

  # --------------------------------------------------------------------------
  # certificates.k8s.io
  # --------------------------------------------------------------------------

  # CSRs: list_csrs, get_csr, approve_csr, deny_csr
  - apiGroups: ["certificates.k8s.io"]
    resources:
      - certificatesigningrequests
    verbs: ["get", "list", "watch"]

  # approve_csr and deny_csr update the approval subresource
  - apiGroups: ["certificates.k8s.io"]
    resources:
      - certificatesigningrequests/approval
    verbs: ["update"]

  # approve_csr and deny_csr also update status conditions
  - apiGroups: ["certificates.k8s.io"]
    resources:
      - certificatesigningrequests/status
    verbs: ["update", "patch"]

  # --------------------------------------------------------------------------
  # admissionregistration.k8s.io
  # --------------------------------------------------------------------------

  # Admission webhooks: list_mutatingwebhookconfigs, get_mutatingwebhookconfig,
  #   list_validatingwebhookconfigs, get_validatingwebhookconfig,
  #   list_validatingadmissionpolicies, get_validatingadmissionpolicy
  - apiGroups: ["admissionregistration.k8s.io"]
    resources:
      - mutatingwebhookconfigurations
      - validatingwebhookconfigurations
      - validatingadmissionpolicies
      - validatingadmissionpolicybindings
    verbs: ["get", "list", "watch"]

  # --------------------------------------------------------------------------
  # scheduling.k8s.io
  # --------------------------------------------------------------------------

  # PriorityClasses: list_priorityclasses, get_priorityclass
  - apiGroups: ["scheduling.k8s.io"]
    resources:
      - priorityclasses
    verbs: ["get", "list", "watch"]

  # --------------------------------------------------------------------------
  # node.k8s.io
  # --------------------------------------------------------------------------

  # RuntimeClasses: list_runtimeclasses, get_runtimeclass
  - apiGroups: ["node.k8s.io"]
    resources:
      - runtimeclasses
    verbs: ["get", "list", "watch"]

  # --------------------------------------------------------------------------
  # flowcontrol.apiserver.k8s.io
  # --------------------------------------------------------------------------

  # FlowSchemas: list_flowschemas, get_flowschema
  # PriorityLevelConfigurations: list_prioritylevelconfigs, get_prioritylevelconfig
  - apiGroups: ["flowcontrol.apiserver.k8s.io"]
    resources:
      - flowschemas
      - prioritylevelconfigurations
    verbs: ["get", "list", "watch"]

  # --------------------------------------------------------------------------
  # authentication.k8s.io
  # --------------------------------------------------------------------------

  # whoami tool creates a SelfSubjectReview
  - apiGroups: ["authentication.k8s.io"]
    resources:
      - selfsubjectreviews
    verbs: ["create"]

  # --------------------------------------------------------------------------
  # authorization.k8s.io
  # --------------------------------------------------------------------------

  # can_i tool creates a SelfSubjectAccessReview
  # list_my_permissions tool creates a SelfSubjectRulesReview
  - apiGroups: ["authorization.k8s.io"]
    resources:
      - selfsubjectaccessreviews
      - selfsubjectrulesreviews
    verbs: ["create"]

  # --------------------------------------------------------------------------
  # metrics.k8s.io
  # --------------------------------------------------------------------------

  # get_metrics (pod metrics), get_node_metrics (node metrics)
  - apiGroups: ["metrics.k8s.io"]
    resources:
      - pods
      - nodes
    verbs: ["get", "list"]

  # --------------------------------------------------------------------------
  # Generic / apply_manifest
  # --------------------------------------------------------------------------
  # The apply_manifest and get_resource_yaml tools use API discovery and can
  # operate on any resource type. The rules above cover all built-in tools.
  # If you use apply_manifest to manage custom resources, add rules for those
  # API groups here.
```

---

## Read-Only ClusterRole

A minimal ClusterRole for read-only mode. Use this when running mcp-k8s with
`--disable-create --disable-update --disable-delete`, which restricts all tools
to read operations only (get, list, watch).

```yaml
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRole
metadata:
  name: mcp-k8s-readonly
rules:
  # Core API
  - apiGroups: [""]
    resources:
      - pods
      - pods/log
      - services
      - configmaps
      - secrets
      - namespaces
      - events
      - endpoints
      - persistentvolumes
      - persistentvolumeclaims
      - serviceaccounts
      - nodes
      - resourcequotas
      - limitranges
    verbs: ["get", "list", "watch"]

  # apps
  - apiGroups: ["apps"]
    resources:
      - deployments
      - statefulsets
      - daemonsets
      - replicasets
    verbs: ["get", "list", "watch"]

  # batch
  - apiGroups: ["batch"]
    resources:
      - jobs
      - cronjobs
    verbs: ["get", "list", "watch"]

  # networking.k8s.io
  - apiGroups: ["networking.k8s.io"]
    resources:
      - ingresses
      - ingressclasses
      - networkpolicies
    verbs: ["get", "list", "watch"]

  # rbac.authorization.k8s.io
  - apiGroups: ["rbac.authorization.k8s.io"]
    resources:
      - roles
      - rolebindings
      - clusterroles
      - clusterrolebindings
    verbs: ["get", "list", "watch"]

  # autoscaling
  - apiGroups: ["autoscaling"]
    resources:
      - horizontalpodautoscalers
    verbs: ["get", "list", "watch"]

  # policy
  - apiGroups: ["policy"]
    resources:
      - poddisruptionbudgets
    verbs: ["get", "list", "watch"]

  # storage.k8s.io
  - apiGroups: ["storage.k8s.io"]
    resources:
      - storageclasses
      - csidrivers
      - csinodes
      - csistoragecapacities
      - volumeattachments
    verbs: ["get", "list", "watch"]

  # coordination.k8s.io
  - apiGroups: ["coordination.k8s.io"]
    resources:
      - leases
    verbs: ["get", "list", "watch"]

  # discovery.k8s.io
  - apiGroups: ["discovery.k8s.io"]
    resources:
      - endpointslices
    verbs: ["get", "list", "watch"]

  # certificates.k8s.io
  - apiGroups: ["certificates.k8s.io"]
    resources:
      - certificatesigningrequests
    verbs: ["get", "list", "watch"]

  # admissionregistration.k8s.io
  - apiGroups: ["admissionregistration.k8s.io"]
    resources:
      - mutatingwebhookconfigurations
      - validatingwebhookconfigurations
      - validatingadmissionpolicies
      - validatingadmissionpolicybindings
    verbs: ["get", "list", "watch"]

  # scheduling.k8s.io
  - apiGroups: ["scheduling.k8s.io"]
    resources:
      - priorityclasses
    verbs: ["get", "list", "watch"]

  # node.k8s.io
  - apiGroups: ["node.k8s.io"]
    resources:
      - runtimeclasses
    verbs: ["get", "list", "watch"]

  # flowcontrol.apiserver.k8s.io
  - apiGroups: ["flowcontrol.apiserver.k8s.io"]
    resources:
      - flowschemas
      - prioritylevelconfigurations
    verbs: ["get", "list", "watch"]

  # authentication.k8s.io -- whoami still works in read-only mode
  - apiGroups: ["authentication.k8s.io"]
    resources:
      - selfsubjectreviews
    verbs: ["create"]

  # authorization.k8s.io -- can_i and list_my_permissions still work in read-only mode
  - apiGroups: ["authorization.k8s.io"]
    resources:
      - selfsubjectaccessreviews
      - selfsubjectrulesreviews
    verbs: ["create"]

  # metrics.k8s.io
  - apiGroups: ["metrics.k8s.io"]
    resources:
      - pods
      - nodes
    verbs: ["get", "list"]
```

---

## Per-Namespace Role

To restrict mcp-k8s to specific namespaces instead of granting cluster-wide access, use a namespaced Role and RoleBinding. Pass `--namespaces ns1,ns2` to mcp-k8s to restrict its tool operations to those namespaces.

> **Note:** Cluster-scoped resources (nodes, namespaces, clusterroles, storageclasses, etc.) are not accessible with a namespaced Role. If you need those tools, use a ClusterRole with a ClusterRoleBinding alongside the namespaced bindings.

```yaml
apiVersion: rbac.authorization.k8s.io/v1
kind: Role
metadata:
  name: mcp-k8s
  namespace: my-namespace
rules:
  # Core namespaced resources
  - apiGroups: [""]
    resources:
      - pods
      - pods/log
      - pods/eviction
      - services
      - configmaps
      - secrets
      - events
      - endpoints
      - persistentvolumeclaims
      - serviceaccounts
      - resourcequotas
      - limitranges
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]

  # apps
  - apiGroups: ["apps"]
    resources:
      - deployments
      - statefulsets
      - daemonsets
      - replicasets
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]

  # batch
  - apiGroups: ["batch"]
    resources:
      - jobs
      - cronjobs
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]

  # networking.k8s.io
  - apiGroups: ["networking.k8s.io"]
    resources:
      - ingresses
      - networkpolicies
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]

  # rbac.authorization.k8s.io (namespaced)
  - apiGroups: ["rbac.authorization.k8s.io"]
    resources:
      - roles
      - rolebindings
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]

  # autoscaling
  - apiGroups: ["autoscaling"]
    resources:
      - horizontalpodautoscalers
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]

  # policy
  - apiGroups: ["policy"]
    resources:
      - poddisruptionbudgets
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]

  # coordination.k8s.io
  - apiGroups: ["coordination.k8s.io"]
    resources:
      - leases
    verbs: ["get", "list", "watch"]

  # discovery.k8s.io
  - apiGroups: ["discovery.k8s.io"]
    resources:
      - endpointslices
    verbs: ["get", "list", "watch"]

  # storage.k8s.io (namespaced)
  - apiGroups: ["storage.k8s.io"]
    resources:
      - csistoragecapacities
    verbs: ["get", "list", "watch"]

  # metrics.k8s.io
  - apiGroups: ["metrics.k8s.io"]
    resources:
      - pods
    verbs: ["get", "list"]
---
apiVersion: rbac.authorization.k8s.io/v1
kind: RoleBinding
metadata:
  name: mcp-k8s
  namespace: my-namespace
subjects:
  - kind: ServiceAccount
    name: mcp-k8s
    namespace: mcp-k8s
roleRef:
  kind: Role
  name: mcp-k8s
  apiGroup: rbac.authorization.k8s.io
```

Repeat the Role and RoleBinding for each namespace you want mcp-k8s to access.

---

## Quick Start

Create a ServiceAccount, apply the full-access ClusterRole, and bind them:

```bash
# Create a namespace for mcp-k8s (optional, can use an existing one)
kubectl create namespace mcp-k8s

# Create the ServiceAccount
kubectl create serviceaccount mcp-k8s -n mcp-k8s

# Apply the full-access ClusterRole
kubectl apply -f - <<'EOF'
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRole
metadata:
  name: mcp-k8s
rules:
  - apiGroups: [""]
    resources: [pods, pods/log, pods/eviction, services, configmaps, secrets, namespaces, events, endpoints, persistentvolumes, persistentvolumeclaims, serviceaccounts, nodes, nodes/status, resourcequotas, limitranges]
    verbs: [get, list, watch, create, update, patch, delete]
  - apiGroups: ["apps"]
    resources: [deployments, statefulsets, daemonsets, replicasets]
    verbs: [get, list, watch, create, update, patch, delete]
  - apiGroups: ["batch"]
    resources: [jobs, cronjobs]
    verbs: [get, list, watch, create, update, patch, delete]
  - apiGroups: ["networking.k8s.io"]
    resources: [ingresses, ingressclasses, networkpolicies]
    verbs: [get, list, watch, create, update, patch, delete]
  - apiGroups: ["rbac.authorization.k8s.io"]
    resources: [roles, rolebindings, clusterroles, clusterrolebindings]
    verbs: [get, list, watch, create, update, patch, delete]
  - apiGroups: ["autoscaling"]
    resources: [horizontalpodautoscalers]
    verbs: [get, list, watch, create, update, patch, delete]
  - apiGroups: ["policy"]
    resources: [poddisruptionbudgets]
    verbs: [get, list, watch, create, update, patch, delete]
  - apiGroups: ["storage.k8s.io"]
    resources: [storageclasses, csidrivers, csinodes, csistoragecapacities, volumeattachments]
    verbs: [get, list, watch, create, update, patch, delete]
  - apiGroups: ["coordination.k8s.io"]
    resources: [leases]
    verbs: [get, list, watch]
  - apiGroups: ["discovery.k8s.io"]
    resources: [endpointslices]
    verbs: [get, list, watch]
  - apiGroups: ["certificates.k8s.io"]
    resources: [certificatesigningrequests]
    verbs: [get, list, watch]
  - apiGroups: ["certificates.k8s.io"]
    resources: [certificatesigningrequests/approval, certificatesigningrequests/status]
    verbs: [update, patch]
  - apiGroups: ["admissionregistration.k8s.io"]
    resources: [mutatingwebhookconfigurations, validatingwebhookconfigurations, validatingadmissionpolicies, validatingadmissionpolicybindings]
    verbs: [get, list, watch]
  - apiGroups: ["scheduling.k8s.io"]
    resources: [priorityclasses]
    verbs: [get, list, watch]
  - apiGroups: ["node.k8s.io"]
    resources: [runtimeclasses]
    verbs: [get, list, watch]
  - apiGroups: ["flowcontrol.apiserver.k8s.io"]
    resources: [flowschemas, prioritylevelconfigurations]
    verbs: [get, list, watch]
  - apiGroups: ["authentication.k8s.io"]
    resources: [selfsubjectreviews]
    verbs: [create]
  - apiGroups: ["authorization.k8s.io"]
    resources: [selfsubjectaccessreviews, selfsubjectrulesreviews]
    verbs: [create]
  - apiGroups: ["metrics.k8s.io"]
    resources: [pods, nodes]
    verbs: [get, list]
EOF

# Bind the ClusterRole to the ServiceAccount
kubectl create clusterrolebinding mcp-k8s \
  --clusterrole=mcp-k8s \
  --serviceaccount=mcp-k8s:mcp-k8s

# Verify permissions
kubectl auth can-i --list --as=system:serviceaccount:mcp-k8s:mcp-k8s
```

For read-only mode, replace `mcp-k8s` with `mcp-k8s-readonly` in the ClusterRole name and binding, and start mcp-k8s with:

```bash
mcp-k8s --http --disable-create --disable-update --disable-delete
```
