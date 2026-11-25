{{/*
RBAC rules for the mcp-k8s ServiceAccount, scoped to what the tools need.
See docs/src/deployment/rbac.md for the tool behind each rule; keep the two in
sync.

Input: dict with "Values" (the chart values) and "clusterWide" (bool; false
leaves out cluster-scoped resources, which a Role can't grant).

Resources with write tools get get/list/watch, plus create/update/patch/delete
unless rbac.readOnly. Everything else is get/list/watch only. No wildcards;
rbac.extraRules may add them only with rbac.allowWildcards.
*/}}
{{- define "mcp-k8s.rbacRules" -}}
{{- $rbac := .Values.rbac -}}
{{- $cw := .clusterWide -}}
{{- $ro := $rbac.readOnly -}}
{{- $read := list "get" "list" "watch" -}}
{{- $rw := list "get" "list" "watch" "create" "update" "patch" "delete" -}}
{{- $verbs := ternary $read $rw $ro -}}
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
  verbs: {{ toJson $verbs | replace "\",\"" "\", \"" }}
{{- if $rbac.secrets }}
# Secrets: list_secrets/get_secret (+ create/update/delete_secret). get/list
# return values to the token whatever the server's secret-decode setting.
- apiGroups: [""]
  resources:
    - secrets
  verbs: {{ toJson $verbs | replace "\",\"" "\", \"" }}
{{- end }}
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
{{- if not $ro }}
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
{{- end }}
{{- if $cw }}
# Namespaces: list/get/create/update/delete_namespace.
- apiGroups: [""]
  resources:
    - namespaces
  verbs: {{ toJson $verbs | replace "\",\"" "\", \"" }}
# Nodes: list/get_node; cordon/uncordon/drain_node patch spec.unschedulable.
- apiGroups: [""]
  resources:
    - nodes
  verbs: {{ toJson (ternary $read (list "get" "list" "watch" "patch") $ro) | replace "\",\"" "\", \"" }}
# PersistentVolumes: list/get/delete_pv.
- apiGroups: [""]
  resources:
    - persistentvolumes
  verbs: {{ toJson (ternary $read (list "get" "list" "watch" "delete") $ro) | replace "\",\"" "\", \"" }}
{{- end }}
# apps: Deployment (incl. scale/restart/rollback), StatefulSet, DaemonSet tools.
- apiGroups: ["apps"]
  resources:
    - deployments
    - statefulsets
    - daemonsets
  verbs: {{ toJson $verbs | replace "\",\"" "\", \"" }}
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
  verbs: {{ toJson $verbs | replace "\",\"" "\", \"" }}
# networking.k8s.io: Ingress and NetworkPolicy tools.
- apiGroups: ["networking.k8s.io"]
  resources:
    - ingresses
    - networkpolicies
  verbs: {{ toJson $verbs | replace "\",\"" "\", \"" }}
{{- if $cw }}
# IngressClass tools.
- apiGroups: ["networking.k8s.io"]
  resources:
    - ingressclasses
  verbs: {{ toJson $verbs | replace "\",\"" "\", \"" }}
# IPAddress and ServiceCIDR (read-only tools).
- apiGroups: ["networking.k8s.io"]
  resources:
    - ipaddresses
    - servicecidrs
  verbs: ["get", "list", "watch"]
{{- end }}
# rbac.authorization.k8s.io: Role and RoleBinding tools. Kubernetes still
# refuses grants beyond the ServiceAccount's own (no escalate/bind here).
- apiGroups: ["rbac.authorization.k8s.io"]
  resources:
    - roles
    - rolebindings
  verbs: {{ toJson $verbs | replace "\",\"" "\", \"" }}
{{- if $cw }}
# ClusterRole and ClusterRoleBinding tools.
- apiGroups: ["rbac.authorization.k8s.io"]
  resources:
    - clusterroles
    - clusterrolebindings
  verbs: {{ toJson $verbs | replace "\",\"" "\", \"" }}
{{- end }}
# autoscaling: HPA tools. policy: PDB tools.
- apiGroups: ["autoscaling"]
  resources:
    - horizontalpodautoscalers
  verbs: {{ toJson $verbs | replace "\",\"" "\", \"" }}
- apiGroups: ["policy"]
  resources:
    - poddisruptionbudgets
  verbs: {{ toJson $verbs | replace "\",\"" "\", \"" }}
# storage.k8s.io, namespaced: list_csistoragecapacities.
- apiGroups: ["storage.k8s.io"]
  resources:
    - csistoragecapacities
  verbs: ["get", "list", "watch"]
{{- if $cw }}
# StorageClass tools.
- apiGroups: ["storage.k8s.io"]
  resources:
    - storageclasses
  verbs: {{ toJson $verbs | replace "\",\"" "\", \"" }}
# CSIDriver, CSINode, VolumeAttachment, VolumeAttributesClass (read-only tools).
- apiGroups: ["storage.k8s.io"]
  resources:
    - csidrivers
    - csinodes
    - volumeattachments
    - volumeattributesclasses
  verbs: ["get", "list", "watch"]
{{- end }}
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
{{- if $cw }}
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
{{- if not $ro }}
# approve_csr and deny_csr patch the approval subresource.
- apiGroups: ["certificates.k8s.io"]
  resources:
    - certificatesigningrequests/approval
  verbs: ["update", "patch"]
{{- with $rbac.csrSigners }}
# ...and Kubernetes also requires `approve` on the CSR's signer.
- apiGroups: ["certificates.k8s.io"]
  resources:
    - signers
  resourceNames: {{ toJson . | replace "\",\"" "\", \"" }}
  verbs: ["approve"]
{{- end }}
{{- end }}
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
{{- end }}
# metrics.k8s.io: get_metrics (pod metrics).
- apiGroups: ["metrics.k8s.io"]
  resources:
    - pods
  verbs: ["get", "list"]
{{- range $rbac.extraRules }}
{{- if not $rbac.allowWildcards }}
{{- if or (has "*" (.apiGroups | default list)) (has "*" (.resources | default list)) (has "*" (.verbs | default list)) }}
{{- fail "rbac.extraRules contains a \"*\" wildcard; set rbac.allowWildcards=true if you mean it" }}
{{- end }}
{{- end }}
- {{ toYaml . | nindent 2 | trim }}
{{- end }}
{{- end }}
