# RBAC Setup

mcp-k8s needs a Kubernetes ServiceAccount with RBAC permissions to interact
with the cluster. The scope of permissions depends on which tools you want to
expose. This page lists the rules for full access, read-only access and
per-namespace access, with the tools behind each rule.

The Helm chart generates these rules (`helm/mcp-k8s/templates/_rbac.tpl`) and
the Kustomize manifests in `helm/charts/mcp-k8s/` carry the same rules; the
YAML below is the chart's output. None of them use wildcards.

**Start with read-only.** Grant only get/list/watch and start the server with
`--disable-create --disable-update --disable-delete`, so `tools/list` shows
only read tools and the ServiceAccount couldn't change anything even if the
server tried. In Helm that's one value, `rbac.readOnly: true`; with Kustomize,
the `read-only/` overlay. Widen to full access once you know you need writes.

## Secrets

Kubernetes RBAC can't grant "secret names and keys only": a token that can
`get` or `list` Secrets can read their values. The server redacts secret
values unless secret decoding is enabled, but that protects the MCP client
side, not the pod: anyone who gets hold of the pod's token can read every
Secret it can list. If you don't need the secret tools, leave Secrets out of
the role (Helm: `rbac.secrets: false`; the chart then also hides
`create_secret`/`update_secret`/`delete_secret`, and `list_secrets`/`get_secret`
return 403).

## Full Access ClusterRole

All permissions every built-in mcp-k8s tool needs (Helm default values).

```yaml
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRole
metadata:
  name: mcp-k8s
rules:
  # Core API, namespaced. Pods: list/get/create/delete_pod, evict_pod, drain_node
  # (deletes pods). Services, ConfigMaps, PVCs, ServiceAccounts, ResourceQuotas,
  # LimitRanges: their list/get/create/update/delete tools.
  - apiGroups: [""]
    resources:
      - pods
      - services
      - configmaps
      - persistentvolumeclaims
      - serviceaccounts
      - resourcequotas
      - limitranges
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]
  # Secrets: list_secrets/get_secret (+ create/update/delete_secret). get/list
  # return values to the token whatever the server's secret-decode setting.
  - apiGroups: [""]
    resources:
      - secrets
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]
  # Events: get_events. Endpoints: list/get_endpoints.
  - apiGroups: [""]
    resources:
      - events
      - endpoints
    verbs: ["get", "list", "watch"]
  # Pod logs: get_pod_logs, get_build_logs.
  - apiGroups: [""]
    resources:
      - pods/log
    verbs: ["get"]
  # exec_pod (pods/exec), port_forward_check (pods/portforward), evict_pod
  # (pods/eviction). The websocket upgrade needs create.
  - apiGroups: [""]
    resources:
      - pods/exec
      - pods/portforward
    verbs: ["get", "create"]
  - apiGroups: [""]
    resources:
      - pods/eviction
    verbs: ["create"]
  # Namespaces: list/get/create/update/delete_namespace.
  - apiGroups: [""]
    resources:
      - namespaces
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]
  # Nodes: list/get_node; cordon/uncordon/drain_node patch spec.unschedulable.
  - apiGroups: [""]
    resources:
      - nodes
    verbs: ["get", "list", "watch", "patch"]
  # PersistentVolumes: list/get/delete_pv.
  - apiGroups: [""]
    resources:
      - persistentvolumes
    verbs: ["get", "list", "watch", "delete"]
  # apps: Deployment (incl. scale/restart/rollback), StatefulSet, DaemonSet tools.
  - apiGroups: ["apps"]
    resources:
      - deployments
      - statefulsets
      - daemonsets
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]
  # ReplicaSets: list/get_replicaset, get_deployment_history, rollback_deployment.
  - apiGroups: ["apps"]
    resources:
      - replicasets
    verbs: ["get", "list", "watch"]
  # batch: Job and CronJob tools (incl. create_job_from_cronjob).
  - apiGroups: ["batch"]
    resources:
      - jobs
      - cronjobs
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]
  # networking.k8s.io: Ingress and NetworkPolicy tools.
  - apiGroups: ["networking.k8s.io"]
    resources:
      - ingresses
      - networkpolicies
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]
  # IngressClass tools.
  - apiGroups: ["networking.k8s.io"]
    resources:
      - ingressclasses
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]
  # IPAddress and ServiceCIDR (read-only tools).
  - apiGroups: ["networking.k8s.io"]
    resources:
      - ipaddresses
      - servicecidrs
    verbs: ["get", "list", "watch"]
  # rbac.authorization.k8s.io: Role and RoleBinding tools. Kubernetes still
  # refuses grants beyond the ServiceAccount's own (no escalate/bind here).
  - apiGroups: ["rbac.authorization.k8s.io"]
    resources:
      - roles
      - rolebindings
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]
  # ClusterRole and ClusterRoleBinding tools.
  - apiGroups: ["rbac.authorization.k8s.io"]
    resources:
      - clusterroles
      - clusterrolebindings
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]
  # autoscaling: HPA tools. policy: PDB tools.
  - apiGroups: ["autoscaling"]
    resources:
      - horizontalpodautoscalers
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]
  - apiGroups: ["policy"]
    resources:
      - poddisruptionbudgets
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]
  # storage.k8s.io, namespaced: list_csistoragecapacities.
  - apiGroups: ["storage.k8s.io"]
    resources:
      - csistoragecapacities
    verbs: ["get", "list", "watch"]
  # StorageClass tools.
  - apiGroups: ["storage.k8s.io"]
    resources:
      - storageclasses
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]
  # CSIDriver, CSINode, VolumeAttachment, VolumeAttributesClass (read-only tools).
  - apiGroups: ["storage.k8s.io"]
    resources:
      - csidrivers
      - csinodes
      - volumeattachments
      - volumeattributesclasses
    verbs: ["get", "list", "watch"]
  # coordination.k8s.io: list/get_lease, list/get_leasecandidate.
  - apiGroups: ["coordination.k8s.io"]
    resources:
      - leases
      - leasecandidates
    verbs: ["get", "list", "watch"]
  # discovery.k8s.io: list/get_endpointslice.
  - apiGroups: ["discovery.k8s.io"]
    resources:
      - endpointslices
    verbs: ["get", "list", "watch"]
  # resource.k8s.io (DRA): ResourceClaim and ResourceClaimTemplate tools.
  - apiGroups: ["resource.k8s.io"]
    resources:
      - resourceclaims
      - resourceclaimtemplates
    verbs: ["get", "list", "watch"]
  # ResourceSlice and DeviceClass tools.
  - apiGroups: ["resource.k8s.io"]
    resources:
      - resourceslices
      - deviceclasses
    verbs: ["get", "list", "watch"]
  # certificates.k8s.io: list/get_csr, list/get_clustertrustbundle.
  - apiGroups: ["certificates.k8s.io"]
    resources:
      - certificatesigningrequests
      - clustertrustbundles
    verbs: ["get", "list", "watch"]
  # approve_csr and deny_csr patch the approval subresource.
  - apiGroups: ["certificates.k8s.io"]
    resources:
      - certificatesigningrequests/approval
    verbs: ["update", "patch"]
  # admissionregistration.k8s.io: webhook configuration and admission policy
  # tools (read-only).
  - apiGroups: ["admissionregistration.k8s.io"]
    resources:
      - mutatingwebhookconfigurations
      - validatingwebhookconfigurations
      - validatingadmissionpolicies
      - validatingadmissionpolicybindings
      - mutatingadmissionpolicies
      - mutatingadmissionpolicybindings
    verbs: ["get", "list", "watch"]
  # apiextensions.k8s.io: list/get_crd, and discovery for the custom resource
  # tools. Custom resources themselves need rbac.extraRules.
  - apiGroups: ["apiextensions.k8s.io"]
    resources:
      - customresourcedefinitions
    verbs: ["get", "list", "watch"]
  # PriorityClass, RuntimeClass, FlowSchema, PriorityLevelConfiguration,
  # StorageVersion, StorageVersionMigration (read-only tools).
  - apiGroups: ["scheduling.k8s.io"]
    resources:
      - priorityclasses
    verbs: ["get", "list", "watch"]
  - apiGroups: ["node.k8s.io"]
    resources:
      - runtimeclasses
    verbs: ["get", "list", "watch"]
  - apiGroups: ["flowcontrol.apiserver.k8s.io"]
    resources:
      - flowschemas
      - prioritylevelconfigurations
    verbs: ["get", "list", "watch"]
  - apiGroups: ["internal.apiserver.k8s.io"]
    resources:
      - storageversions
    verbs: ["get", "list", "watch"]
  - apiGroups: ["storagemigration.k8s.io"]
    resources:
      - storageversionmigrations
    verbs: ["get", "list", "watch"]
  # whoami (SelfSubjectReview), can_i (SelfSubjectAccessReview),
  # list_my_permissions (SelfSubjectRulesReview). Kubernetes grants these to
  # every authenticated user already; listed for clarity.
  - apiGroups: ["authentication.k8s.io"]
    resources:
      - selfsubjectreviews
    verbs: ["create"]
  - apiGroups: ["authorization.k8s.io"]
    resources:
      - selfsubjectaccessreviews
      - selfsubjectrulesreviews
    verbs: ["create"]
  # metrics.k8s.io: get_node_metrics.
  - apiGroups: ["metrics.k8s.io"]
    resources:
      - nodes
    verbs: ["get", "list"]
  # metrics.k8s.io: get_metrics (pod metrics).
  - apiGroups: ["metrics.k8s.io"]
    resources:
      - pods
    verbs: ["get", "list"]
```

Not included, add them yourself if you need them (Helm: `rbac.extraRules`):

- **Custom resources.** `list_crds`/`get_crd` are covered, but the custom
  resource tools (`list_custom_resources`, `create_custom_resource`, ...),
  `watch_resource` and `get_resource_json` on custom kinds need a rule for each
  API group, e.g. `{apiGroups: ["cert-manager.io"], resources: ["certificates"], verbs: ["get", "list", "watch"]}`.
- **`apply_manifest`** (off by default) can target any kind; it needs rules for
  whatever you apply beyond the kinds above.
- **CSR approval.** Besides `certificatesigningrequests/approval`, Kubernetes
  requires the `approve` verb on the CSR's signer
  (`certificates.k8s.io/signers`, `resourceNames: [<signerName>]`) for both
  `approve_csr` and `deny_csr`. Helm: `rbac.csrSigners: ["example.com/my-signer"]`.
- **RBAC escalation.** `create_role`/`create_clusterrole` and the binding
  tools can only grant what the ServiceAccount itself holds; the role
  deliberately has no `escalate` or `bind` verbs.

## Read-Only ClusterRole

The recommended starting point. Use it with
`--disable-create --disable-update --disable-delete` (Helm `rbac.readOnly: true`
does both). The self-review `create` rules keep `whoami`, `can_i` and
`list_my_permissions` working; every authenticated user has those already.

`port_forward_check` is a read tool but needs `pods/portforward`, which this
role leaves out (it opens a connection into any pod), so it returns 403.

```yaml
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRole
metadata:
  name: mcp-k8s-readonly
rules:
  # Core API, namespaced. Pods: list/get/create/delete_pod, evict_pod, drain_node
  # (deletes pods). Services, ConfigMaps, PVCs, ServiceAccounts, ResourceQuotas,
  # LimitRanges: their list/get/create/update/delete tools.
  - apiGroups: [""]
    resources:
      - pods
      - services
      - configmaps
      - persistentvolumeclaims
      - serviceaccounts
      - resourcequotas
      - limitranges
    verbs: ["get", "list", "watch"]
  # Secrets: list_secrets/get_secret (+ create/update/delete_secret). get/list
  # return values to the token whatever the server's secret-decode setting.
  - apiGroups: [""]
    resources:
      - secrets
    verbs: ["get", "list", "watch"]
  # Events: get_events. Endpoints: list/get_endpoints.
  - apiGroups: [""]
    resources:
      - events
      - endpoints
    verbs: ["get", "list", "watch"]
  # Pod logs: get_pod_logs, get_build_logs.
  - apiGroups: [""]
    resources:
      - pods/log
    verbs: ["get"]
  # Namespaces: list/get/create/update/delete_namespace.
  - apiGroups: [""]
    resources:
      - namespaces
    verbs: ["get", "list", "watch"]
  # Nodes: list/get_node; cordon/uncordon/drain_node patch spec.unschedulable.
  - apiGroups: [""]
    resources:
      - nodes
    verbs: ["get", "list", "watch"]
  # PersistentVolumes: list/get/delete_pv.
  - apiGroups: [""]
    resources:
      - persistentvolumes
    verbs: ["get", "list", "watch"]
  # apps: Deployment (incl. scale/restart/rollback), StatefulSet, DaemonSet tools.
  - apiGroups: ["apps"]
    resources:
      - deployments
      - statefulsets
      - daemonsets
    verbs: ["get", "list", "watch"]
  # ReplicaSets: list/get_replicaset, get_deployment_history, rollback_deployment.
  - apiGroups: ["apps"]
    resources:
      - replicasets
    verbs: ["get", "list", "watch"]
  # batch: Job and CronJob tools (incl. create_job_from_cronjob).
  - apiGroups: ["batch"]
    resources:
      - jobs
      - cronjobs
    verbs: ["get", "list", "watch"]
  # networking.k8s.io: Ingress and NetworkPolicy tools.
  - apiGroups: ["networking.k8s.io"]
    resources:
      - ingresses
      - networkpolicies
    verbs: ["get", "list", "watch"]
  # IngressClass tools.
  - apiGroups: ["networking.k8s.io"]
    resources:
      - ingressclasses
    verbs: ["get", "list", "watch"]
  # IPAddress and ServiceCIDR (read-only tools).
  - apiGroups: ["networking.k8s.io"]
    resources:
      - ipaddresses
      - servicecidrs
    verbs: ["get", "list", "watch"]
  # rbac.authorization.k8s.io: Role and RoleBinding tools. Kubernetes still
  # refuses grants beyond the ServiceAccount's own (no escalate/bind here).
  - apiGroups: ["rbac.authorization.k8s.io"]
    resources:
      - roles
      - rolebindings
    verbs: ["get", "list", "watch"]
  # ClusterRole and ClusterRoleBinding tools.
  - apiGroups: ["rbac.authorization.k8s.io"]
    resources:
      - clusterroles
      - clusterrolebindings
    verbs: ["get", "list", "watch"]
  # autoscaling: HPA tools. policy: PDB tools.
  - apiGroups: ["autoscaling"]
    resources:
      - horizontalpodautoscalers
    verbs: ["get", "list", "watch"]
  - apiGroups: ["policy"]
    resources:
      - poddisruptionbudgets
    verbs: ["get", "list", "watch"]
  # storage.k8s.io, namespaced: list_csistoragecapacities.
  - apiGroups: ["storage.k8s.io"]
    resources:
      - csistoragecapacities
    verbs: ["get", "list", "watch"]
  # StorageClass tools.
  - apiGroups: ["storage.k8s.io"]
    resources:
      - storageclasses
    verbs: ["get", "list", "watch"]
  # CSIDriver, CSINode, VolumeAttachment, VolumeAttributesClass (read-only tools).
  - apiGroups: ["storage.k8s.io"]
    resources:
      - csidrivers
      - csinodes
      - volumeattachments
      - volumeattributesclasses
    verbs: ["get", "list", "watch"]
  # coordination.k8s.io: list/get_lease, list/get_leasecandidate.
  - apiGroups: ["coordination.k8s.io"]
    resources:
      - leases
      - leasecandidates
    verbs: ["get", "list", "watch"]
  # discovery.k8s.io: list/get_endpointslice.
  - apiGroups: ["discovery.k8s.io"]
    resources:
      - endpointslices
    verbs: ["get", "list", "watch"]
  # resource.k8s.io (DRA): ResourceClaim and ResourceClaimTemplate tools.
  - apiGroups: ["resource.k8s.io"]
    resources:
      - resourceclaims
      - resourceclaimtemplates
    verbs: ["get", "list", "watch"]
  # ResourceSlice and DeviceClass tools.
  - apiGroups: ["resource.k8s.io"]
    resources:
      - resourceslices
      - deviceclasses
    verbs: ["get", "list", "watch"]
  # certificates.k8s.io: list/get_csr, list/get_clustertrustbundle.
  - apiGroups: ["certificates.k8s.io"]
    resources:
      - certificatesigningrequests
      - clustertrustbundles
    verbs: ["get", "list", "watch"]
  # admissionregistration.k8s.io: webhook configuration and admission policy
  # tools (read-only).
  - apiGroups: ["admissionregistration.k8s.io"]
    resources:
      - mutatingwebhookconfigurations
      - validatingwebhookconfigurations
      - validatingadmissionpolicies
      - validatingadmissionpolicybindings
      - mutatingadmissionpolicies
      - mutatingadmissionpolicybindings
    verbs: ["get", "list", "watch"]
  # apiextensions.k8s.io: list/get_crd, and discovery for the custom resource
  # tools. Custom resources themselves need rbac.extraRules.
  - apiGroups: ["apiextensions.k8s.io"]
    resources:
      - customresourcedefinitions
    verbs: ["get", "list", "watch"]
  # PriorityClass, RuntimeClass, FlowSchema, PriorityLevelConfiguration,
  # StorageVersion, StorageVersionMigration (read-only tools).
  - apiGroups: ["scheduling.k8s.io"]
    resources:
      - priorityclasses
    verbs: ["get", "list", "watch"]
  - apiGroups: ["node.k8s.io"]
    resources:
      - runtimeclasses
    verbs: ["get", "list", "watch"]
  - apiGroups: ["flowcontrol.apiserver.k8s.io"]
    resources:
      - flowschemas
      - prioritylevelconfigurations
    verbs: ["get", "list", "watch"]
  - apiGroups: ["internal.apiserver.k8s.io"]
    resources:
      - storageversions
    verbs: ["get", "list", "watch"]
  - apiGroups: ["storagemigration.k8s.io"]
    resources:
      - storageversionmigrations
    verbs: ["get", "list", "watch"]
  # whoami (SelfSubjectReview), can_i (SelfSubjectAccessReview),
  # list_my_permissions (SelfSubjectRulesReview). Kubernetes grants these to
  # every authenticated user already; listed for clarity.
  - apiGroups: ["authentication.k8s.io"]
    resources:
      - selfsubjectreviews
    verbs: ["create"]
  - apiGroups: ["authorization.k8s.io"]
    resources:
      - selfsubjectaccessreviews
      - selfsubjectrulesreviews
    verbs: ["create"]
  # metrics.k8s.io: get_node_metrics.
  - apiGroups: ["metrics.k8s.io"]
    resources:
      - nodes
    verbs: ["get", "list"]
  # metrics.k8s.io: get_metrics (pod metrics).
  - apiGroups: ["metrics.k8s.io"]
    resources:
      - pods
    verbs: ["get", "list"]
```

## Per-Namespace Role

To restrict mcp-k8s to specific namespaces instead of granting cluster-wide
access, use a Role and RoleBinding in each namespace, and pass
`--namespaces ns1,ns2` so the server's tools stay inside them. The Helm chart
does this with `rbac.clusterWide: false`: one Role and RoleBinding per entry
in `namespaces` (or the release namespace when it's empty).

> **Note:** A Role can't grant cluster-scoped resources (nodes, namespaces,
> PersistentVolumes, ClusterRoles, StorageClasses, CRDs, ...), so the rules
> below leave them out and those tools return 403. If you need them, add a
> ClusterRole with a ClusterRoleBinding alongside the namespaced bindings.

```yaml
apiVersion: rbac.authorization.k8s.io/v1
kind: Role
metadata:
  name: mcp-k8s
  namespace: my-namespace
rules:
  # Core API, namespaced. Pods: list/get/create/delete_pod, evict_pod, drain_node
  # (deletes pods). Services, ConfigMaps, PVCs, ServiceAccounts, ResourceQuotas,
  # LimitRanges: their list/get/create/update/delete tools.
  - apiGroups: [""]
    resources:
      - pods
      - services
      - configmaps
      - persistentvolumeclaims
      - serviceaccounts
      - resourcequotas
      - limitranges
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]
  # Secrets: list_secrets/get_secret (+ create/update/delete_secret). get/list
  # return values to the token whatever the server's secret-decode setting.
  - apiGroups: [""]
    resources:
      - secrets
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]
  # Events: get_events. Endpoints: list/get_endpoints.
  - apiGroups: [""]
    resources:
      - events
      - endpoints
    verbs: ["get", "list", "watch"]
  # Pod logs: get_pod_logs, get_build_logs.
  - apiGroups: [""]
    resources:
      - pods/log
    verbs: ["get"]
  # exec_pod (pods/exec), port_forward_check (pods/portforward), evict_pod
  # (pods/eviction). The websocket upgrade needs create.
  - apiGroups: [""]
    resources:
      - pods/exec
      - pods/portforward
    verbs: ["get", "create"]
  - apiGroups: [""]
    resources:
      - pods/eviction
    verbs: ["create"]
  # apps: Deployment (incl. scale/restart/rollback), StatefulSet, DaemonSet tools.
  - apiGroups: ["apps"]
    resources:
      - deployments
      - statefulsets
      - daemonsets
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]
  # ReplicaSets: list/get_replicaset, get_deployment_history, rollback_deployment.
  - apiGroups: ["apps"]
    resources:
      - replicasets
    verbs: ["get", "list", "watch"]
  # batch: Job and CronJob tools (incl. create_job_from_cronjob).
  - apiGroups: ["batch"]
    resources:
      - jobs
      - cronjobs
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]
  # networking.k8s.io: Ingress and NetworkPolicy tools.
  - apiGroups: ["networking.k8s.io"]
    resources:
      - ingresses
      - networkpolicies
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]
  # rbac.authorization.k8s.io: Role and RoleBinding tools. Kubernetes still
  # refuses grants beyond the ServiceAccount's own (no escalate/bind here).
  - apiGroups: ["rbac.authorization.k8s.io"]
    resources:
      - roles
      - rolebindings
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]
  # autoscaling: HPA tools. policy: PDB tools.
  - apiGroups: ["autoscaling"]
    resources:
      - horizontalpodautoscalers
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]
  - apiGroups: ["policy"]
    resources:
      - poddisruptionbudgets
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]
  # storage.k8s.io, namespaced: list_csistoragecapacities.
  - apiGroups: ["storage.k8s.io"]
    resources:
      - csistoragecapacities
    verbs: ["get", "list", "watch"]
  # coordination.k8s.io: list/get_lease, list/get_leasecandidate.
  - apiGroups: ["coordination.k8s.io"]
    resources:
      - leases
      - leasecandidates
    verbs: ["get", "list", "watch"]
  # discovery.k8s.io: list/get_endpointslice.
  - apiGroups: ["discovery.k8s.io"]
    resources:
      - endpointslices
    verbs: ["get", "list", "watch"]
  # resource.k8s.io (DRA): ResourceClaim and ResourceClaimTemplate tools.
  - apiGroups: ["resource.k8s.io"]
    resources:
      - resourceclaims
      - resourceclaimtemplates
    verbs: ["get", "list", "watch"]
  # metrics.k8s.io: get_metrics (pod metrics).
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

Without Helm, the Kustomize manifests create the namespace, ServiceAccount,
ClusterRole and binding along with the Deployment:

```bash
# Read-only (recommended starting point)
kubectl apply -k helm/charts/mcp-k8s/read-only/

# Full access
kubectl apply -k helm/charts/mcp-k8s/

# Verify permissions
kubectl auth can-i --list --as=system:serviceaccount:mcp-k8s:mcp-k8s
```

To set up RBAC only (for example for a server running outside the cluster):

```bash
kubectl create namespace mcp-k8s
kubectl create serviceaccount mcp-k8s -n mcp-k8s
kubectl apply -f helm/charts/mcp-k8s/base/clusterrole.yaml
kubectl create clusterrolebinding mcp-k8s \
  --clusterrole=mcp-k8s \
  --serviceaccount=mcp-k8s:mcp-k8s
```

For read-only, apply the read-only ClusterRole above instead and start
mcp-k8s with:

```bash
mcp-k8s --http --disable-create --disable-update --disable-delete
```
