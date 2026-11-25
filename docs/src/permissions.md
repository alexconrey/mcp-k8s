# Permissions

mcp-k8s includes a built-in permission system that controls which CRUD actions are available to MCP clients. This is separate from Kubernetes RBAC -- it controls which tools are exposed by the MCP server, while Kubernetes RBAC controls what the server's service account can actually do in the cluster.

## Design Principles

1. **Read is always allowed.** Read operations (list, get, logs, metrics, whoami, can_i) cannot be disabled through the permission system.
2. **Global flags are kill-switches.** `--disable-create`, `--disable-update`, and `--disable-delete` remove all tools of that action type from the MCP tool list.
3. **Per-resource overrides take precedence.** The `--disable` flag allows fine-grained control over specific resource-action combinations.
4. **Disabled tools are invisible.** When a tool is disabled by permissions, it is removed from the `tools/list` response entirely. MCP clients never see it.

## Action Classification

Every tool is classified into one of four CRUD actions:

| Action | Tool Prefixes | Examples |
|--------|--------------|----------|
| **Read** | `list_`, `get_`, `*_logs`, `*_metrics`, `can_i`, `whoami`, `list_my_permissions`, `get_resource_json` | `list_deployments`, `get_pod_logs`, `get_metrics` |
| **Create** | `create_`, `apply_manifest` | `create_deployment`, `create_pod`, `apply_manifest` |
| **Update** | `update_`, `exec_`, `scale_`, `restart_`, `rollback_`, `approve_`, `deny_`, `cordon_`, `uncordon_`, `drain_` | `update_deployment`, `exec_pod`, `scale_deployment`, `cordon_node` |
| **Delete** | `delete_`, `evict_` | `delete_pod`, `evict_pod` |

Unknown tool names default to **Read** as a safe fallback.

`exec_pod` runs arbitrary commands inside a container, so it counts as an **Update** on `pod`: read-only mode, `--disable-update` and `--disable pod-update` all turn it off. Before this change it fell through to Read and stayed available even in read-only mode.

## Resolution Logic

When checking whether a tool is allowed, mcp-k8s follows this resolution order:

1. **Extract the action** from the tool name (e.g. `delete_deployment` -> action=Delete, resource=deployment).
2. **If action is Read**, always allow.
3. **Check per-resource override** -- if `--disable` includes `<resource>-<action>`, deny.
4. **Check global flag** -- if the corresponding `--disable-<action>` flag is set, deny.
5. **Otherwise**, allow.

```
is_tool_allowed("delete_deployment")
  -> action = Delete, resource = "deployment"
  -> check override: "deployment-delete" in --disable? YES -> DENIED
  -> (would check global --disable-delete, but override already decided)
```

## Configuration Examples

### Read-Only Mode

Disable all mutating operations. Only list, get, and introspection tools are available.

```bash
mcp-k8s --disable-create --disable-update --disable-delete
```

Or with environment variables:

```bash
DISABLE_CREATE=true DISABLE_UPDATE=true DISABLE_DELETE=true mcp-k8s
```

Claude Code MCP config:

```json
{
  "mcpServers": {
    "mcp-k8s": {
      "command": "mcp-k8s",
      "args": ["--disable-create", "--disable-update", "--disable-delete"]
    }
  }
}
```

### Production Lockdown

Allow create and update but prevent accidental deletions:

```bash
mcp-k8s --disable-delete
```

### Selective Resource Restrictions

Prevent deployment deletions and secret creation, but allow everything else:

```bash
mcp-k8s --disable deployment-delete,secret-create
```

### Namespace + Permission Combo

Restrict to specific namespaces and disable dangerous operations:

```bash
mcp-k8s \
  --namespaces staging,production \
  --disable-delete \
  --disable deployment-create,secret-create
```

### Per-Resource Granularity

Only block specific resource-action combinations:

```bash
mcp-k8s --disable \
  namespace-delete,\
  node-delete,\
  clusterrole-create,\
  clusterrole-delete,\
  clusterrolebinding-create,\
  clusterrolebinding-delete
```

## Workload Guardrails

The permission switches above decide **which tools** run. The workload policy decides **what those tools may create**. The typed tools and the custom resource tools enforce it, with the same code:

- the typed `create_*` tools check the object they built;
- the typed `update_*` tools check the server's dry-run result of the patch;
- `create_custom_resource` and `update_custom_resource` check the raw manifest (or, for updates, the dry-run result).

`apply_manifest` is the one exception, on purpose: it's off by default, and when an operator enables it it's an unrestricted raw apply (see [`apply_manifest`](#apply_manifest)). The defaults are restrictive. Each relaxation is an explicit opt-in, and every rejection names the flag that allows it, for example:

```text
rejected by the mcp-k8s workload policy (1 violation):
- spec.template.spec.containers[0] (agent).securityContext: privileged: true gives the container full access to the node; an operator can allow this with --allow-privileged (env MCP_K8S_ALLOW_PRIVILEGED=true; library: WorkloadPolicy.allow_privileged)
These guardrails apply to every typed tool and the custom resource tools. Don't work around a rejection with another tool (including apply_manifest): report it to the user.
```

### Rules and flags

| Rule (default: rejected) | Applies to | Flag / env var | `WorkloadPolicy` field |
|--------------------------|------------|----------------|------------------------|
| `securityContext.privileged: true`, Windows `hostProcess`, namespaces labelled `pod-security.kubernetes.io/*=privileged` | every container (incl. init and ephemeral), Namespaces | `--allow-privileged` / `MCP_K8S_ALLOW_PRIVILEGED` | `allow_privileged` |
| `allowPrivilegeEscalation: true` | containers | `--allow-privilege-escalation` / `MCP_K8S_ALLOW_PRIVILEGE_ESCALATION` | `allow_privilege_escalation` |
| `capabilities.add` beyond the runtime's default set (AUDIT_WRITE, CHOWN, DAC_OVERRIDE, FOWNER, FSETID, KILL, MKNOD, NET_BIND_SERVICE, SETFCAP, SETGID, SETPCAP, SETUID, SYS_CHROOT) | containers | `--allow-capabilities NET_ADMIN,…` (`ALL` = any) / `MCP_K8S_ALLOW_CAPABILITIES` | `allowed_capabilities` |
| `hostNetwork`, `hostPID`, `hostIPC` | pods | `--allow-host-namespaces` / `MCP_K8S_ALLOW_HOST_NAMESPACES` | `allow_host_namespaces` |
| `hostPath` volumes | pods | `--allow-host-path` / `MCP_K8S_ALLOW_HOST_PATH` | `allow_host_path` |
| `hostPort` | container ports | `--allow-host-ports` / `MCP_K8S_ALLOW_HOST_PORTS` | `allow_host_ports` |
| Volume types other than configMap, secret, emptyDir, persistentVolumeClaim, projected, downwardAPI, ephemeral (e.g. `nfs`, inline `csi`, `gitRepo`, `iscsi`) | pods | `--allow-volume-types nfs,csi` (`*` = any but hostPath) / `MCP_K8S_ALLOW_VOLUME_TYPES` | `allowed_volume_types` |
| seccomp/AppArmor `Unconfined` (fields or legacy annotations), custom SELinux type/user/role, `procMount: Unmasked`, sysctls outside the safe set, probe/lifecycle `host` other than localhost | pods, containers | `--allow-unconfined` / `MCP_K8S_ALLOW_UNCONFINED` | `allow_unconfined` |
| Images outside the registry allowlist (only when one is set) | containers | `--allowed-registries ghcr.io/myorg,docker.io/library` / `MCP_K8S_ALLOWED_REGISTRIES` | `allowed_registries` |
| `type: NodePort`, explicit `nodePort` | Services | `--allow-nodeport` / `MCP_K8S_ALLOW_NODEPORT` | `allow_node_port` |
| `type: LoadBalancer`, `loadBalancerSourceRanges`/`Class`/`IP`, `allocateLoadBalancerNodePorts`, cloud load-balancer annotations (`service.beta.kubernetes.io/*`, GKE, MetalLB, kube-vip, Cilium LB-IPAM) | Services | `--allow-loadbalancer` / `MCP_K8S_ALLOW_LOADBALANCER` | `allow_load_balancer` |
| `type: ExternalName`, `spec.externalIPs` (CVE-2020-8554) | Services | `--allow-external-services` / `MCP_K8S_ALLOW_EXTERNAL_SERVICES` | `allow_external_services` |
| `*-snippet(s)` annotations (any controller); characters and words that inject nginx config (`;`, `{`, `}`, quotes, `\`, newlines, `_by_lua`, `load_module`, …) in ingress-nginx annotation values; `auth-url`/`auth-signin`/`mirror-target`/redirect values that aren't plain http(s) URLs; paths with injection characters (the IngressNightmare class: CVE-2025-1097, CVE-2025-1098, CVE-2025-24514) | Ingresses | `--allow-ingress-snippets` / `MCP_K8S_ALLOW_INGRESS_SNIPPETS` | `allow_ingress_snippets` |
| Annotations not in the allowlist (when set) / in the denylist; trailing `*` matches a prefix | Ingresses | `--ingress-annotation-allowlist`, `--ingress-annotation-denylist` / `MCP_K8S_INGRESS_ANNOTATION_ALLOWLIST`, `MCP_K8S_INGRESS_ANNOTATION_DENYLIST` | `ingress_annotation_allowlist`, `ingress_annotation_denylist` |
| RBAC escalation primitives: verbs `escalate`, `bind`, `impersonate`; wildcard verbs, resources (`*`, `x/*`), apiGroups or nonResourceURLs; aggregated ClusterRoles and `aggregate-to-*` labels; binding `cluster-admin`, or a role whose rules contain any of the above (fetched live); binding `system:anonymous`/`system:unauthenticated`; `kubernetes.io/service-account-token` Secrets | Roles, ClusterRoles, bindings, Secrets | `--allow-rbac-escalation` / `MCP_K8S_ALLOW_RBAC_ESCALATION` | `allow_rbac_escalation` |
| Approving a CSR for a privileged identity: subject group `system:masters` (any signer); any other `system:` user (CN) or group (O), such as `system:node:*`/`system:nodes`, `system:kube-*`, `system:serviceaccount:*`, `system:anonymous`, `system:unauthenticated`; requests whose PEM/PKCS#10 subject can't be parsed. Exception: a node's own serving certificate (signer `kubernetes.io/kubelet-serving`, `CN=system:node:<name>`, `O=system:nodes`, requested by that node; SANs aren't checked). See [CSR approval](#csr-approval). | `approve_csr` | `--allow-privileged-csr` / `MCP_K8S_ALLOW_PRIVILEGED_CSR` | `allow_privileged_csr` |
| Cluster-scoped custom resource kinds | custom resource tools | `--allow-manifest-kinds ClusterRole,rbac.authorization.k8s.io/RoleBinding` (`*` = all) / `MCP_K8S_ALLOW_MANIFEST_KINDS` | `allowed_manifest_kinds` |
| Custom resources (any non-built-in API group). Operators can turn them into pods or RBAC objects; pod specs, containers and manifests embedded in an allowlisted custom resource still get the rules above. | custom resource tools | `--allow-custom-resource-groups cert-manager.io` (`*` = any) / `MCP_K8S_ALLOW_CUSTOM_RESOURCE_GROUPS` | `allowed_custom_resource_groups` |

The namespace allowlist (`--namespaces`) applies to every tool, including `apply_manifest` (its `namespace` argument and the manifest's `metadata.namespace`), even though the workload policy doesn't. See [Namespace allowlist](#namespace-allowlist).

### CSR approval

Approving a CertificateSigningRequest lets its signer issue a certificate for the identity the request names: the subject's CN becomes the user and every O a group. A request for `O=system:masters` signed by `kubernetes.io/kube-apiserver-client` is a cluster-admin credential that RBAC can't take away, and `system:` names impersonate nodes, control-plane components or service accounts. So `approve_csr` parses `spec.request` and, by default, refuses:

- group `system:masters`, whatever the signer;
- any other user or group starting with `system:`, whatever the signer. This includes kubelet client certificates (`kubernetes.io/kube-apiserver-client-kubelet`): kube-controller-manager approves the legitimate ones itself, so a pending one that needs a human is suspect;
- requests that aren't exactly one parseable `CERTIFICATE REQUEST` PEM block.

A node's request for its own serving certificate is allowed: signer `kubernetes.io/kubelet-serving`, subject `CN=system:node:<name>, O=system:nodes`, and `spec.username` `system:node:<name>`. kube-controller-manager doesn't approve these, so clusters with `serverTLSBootstrap` approve them by hand. mcp-k8s doesn't check their SANs against the node's addresses; use a dedicated approver (such as kubelet-csr-approver) if you need that.

`get_csr` shows the parsed subject (`subject.common_names`, `subject.organizations`). `--allow-privileged-csr` (`MCP_K8S_ALLOW_PRIVILEGED_CSR`, `WorkloadPolicy.allow_privileged_csr`) turns the check off. `deny_csr` is never restricted.

### CSR approval

Approving a CertificateSigningRequest lets its signer issue a certificate for the identity the request names: the subject's CN becomes the user and every O a group. A request for `O=system:masters` signed by `kubernetes.io/kube-apiserver-client` is a cluster-admin credential that RBAC can't take away, and `system:` names impersonate nodes, control-plane components or service accounts. So `approve_csr` parses `spec.request` and, by default, refuses:

- group `system:masters`, whatever the signer;
- any other user or group starting with `system:`, whatever the signer. This includes kubelet client certificates (`kubernetes.io/kube-apiserver-client-kubelet`): kube-controller-manager approves the legitimate ones itself, so a pending one that needs a human is suspect;
- requests that aren't exactly one parseable `CERTIFICATE REQUEST` PEM block.

A node's request for its own serving certificate is allowed: signer `kubernetes.io/kubelet-serving`, subject `CN=system:node:<name>, O=system:nodes`, and `spec.username` `system:node:<name>`. kube-controller-manager doesn't approve these, so clusters with `serverTLSBootstrap` approve them by hand. mcp-k8s doesn't check their SANs against the node's addresses; use a dedicated approver (such as kubelet-csr-approver) if you need that.

`get_csr` shows the parsed subject (`subject.common_names`, `subject.organizations`). `--allow-privileged-csr` (`MCP_K8S_ALLOW_PRIVILEGED_CSR`, `WorkloadPolicy.allow_privileged_csr`) turns the check off. `deny_csr` is never restricted.

### Secure defaults

The typed create tools also fill in a restricted securityContext when the caller leaves it unset:

- `runAsNonRoot: true`;
- seccomp `RuntimeDefault`;
- `allowPrivilegeEscalation: false`;
- `capabilities.drop: [ALL]` for non-root containers;
- `automountServiceAccountToken: false` unless `service_account_name` is set.

They add the label `app.kubernetes.io/managed-by: mcp-k8s`; `apply_manifest` adds it too when the manifest has no managed-by label. These are defaults, not rules: `security_context.run_as_non_root: false` runs root images. `--no-secure-defaults` (`MCP_K8S_NO_SECURE_DEFAULTS`, `WorkloadPolicy.secure_defaults = false`) turns them off. See [Pod Template Arguments](./tools/pod-template.md#security-context-and-secure-defaults).

### Updates

Update tools patch with a server-side **dry run** first, check the resulting object, then write with the dry run's `resourceVersion` as a precondition. An update therefore can't add a disallowed setting. It also can't modify a workload that already breaks the policy, for example swapping the image of an existing privileged DaemonSet. That block is deliberate; allow the relevant rule to manage such workloads through mcp-k8s. `scale_deployment` and `restart_deployment` don't change pod content and aren't checked.

### Library users

`WorkloadPolicy` lives on `ActionPermissions`. `ActionPermissions::default()` and `ActionPermissions::new(...)` use the restrictive `WorkloadPolicy::default()`; replace it with `with_workload_policy`:

```rust
use mcp_k8s::permissions::ActionPermissions;
use mcp_k8s::policy::WorkloadPolicy;

let mut policy = WorkloadPolicy::default();
policy.allow_load_balancer = true;
policy.allowed_custom_resource_groups = vec!["cert-manager.io".into()];
let permissions = ActionPermissions::default().with_workload_policy(policy);
// WorkloadPolicy::permissive() restores the pre-0.5 behaviour (no guardrails).
```

`mcp_k8s::policy::check_object` is public, so an embedding application can run the same checks on objects it writes itself.

### Limits

The policy decides what mcp-k8s writes. It is not an admission controller:

- it doesn't stop other clients;
- it doesn't follow what an allowlisted operator does with a custom resource (for example a GitOps controller pulling from a repository);
- it can't see the user an image runs as.

Use Pod Security Admission, an admission policy engine and Kubernetes RBAC for cluster-wide enforcement; the guardrails keep the AI-facing tools inside those lines.

## Namespace allowlist

`--namespaces team-a,team-b` (library: the `allowed_namespaces` argument of `K8sClient::new`) restricts every namespaced tool to those namespaces. An empty list means every namespace. A call naming another namespace is refused with `Namespace '…' is not in the allowed list` before anything is sent to the Kubernetes API. `tests/namespace_allowlist.rs` checks this for every tool that takes a namespace, and fails for a new tool that takes none unless it is listed as exempt with a reason.

With an allowlist set:

- **An omitted namespace never means "all namespaces".** `watch_resource` (namespaced kinds), `list_custom_resources` (namespaced kinds) and `list_csistoragecapacities` refuse with `namespace is required` instead of acting across every namespace. Without an allowlist they still do.
- **Namespace objects are covered by name.** `get_namespace`, `create_namespace`, `update_namespace` and `delete_namespace` refuse a namespace outside the list; `list_namespaces` hides them.
- **`drain_node`** refuses, before cordoning, when the node runs non-DaemonSet pods in a namespace outside the list.
- **`can_i` and `list_my_permissions`** refuse a namespace outside the list.
- **`apply_manifest`** checks both its `namespace` argument and `metadata.namespace`, and refuses when they differ.
- Cluster-scoped resources (nodes, ClusterRoles, StorageClasses, …) aren't namespaced, so the allowlist doesn't apply to them; use the action switches to restrict them.

## `exec_pod` and node-level access

A command run with `exec_pod` in a privileged container, or in a pod that shares the node's namespaces or mounts its filesystem, has node-level access. So `exec_pod` reads the pod first and runs the workload policy's pod rules on it. It refuses if the pod breaks one of the rules that give node-level access, naming the flag that would allow it:

| Pod setting | Flag that allows exec |
|-------------|-----------------------|
| `privileged: true` (or Windows `hostProcess`) | `--allow-privileged` |
| `hostNetwork`, `hostPID`, `hostIPC` | `--allow-host-namespaces` |
| `hostPath` volumes | `--allow-host-path` |
| Capabilities beyond the runtime defaults | `--allow-capabilities <CAP>` |
| Unconfined seccomp/AppArmor, custom SELinux options, `procMount: Unmasked`, unsafe sysctls | `--allow-unconfined` |

Rules that only control what may be created (image registries, volume types other than `hostPath`, host ports, `allowPrivilegeEscalation`, probe or lifecycle-hook hosts) don't block exec. If the operator has allowed a setting (so the typed tools could create such a pod), exec into such pods is allowed too. `exec_pod` also has a timeout (`timeout_secs`, default 60, max 300) and a 1 MiB cap on each of stdout and stderr; see [exec_pod](./tools/core.md#exec_pod).

## `apply_manifest`

`apply_manifest` uses server-side apply to create **or** update an object from a raw JSON manifest. It's for users who know exactly what they're applying, and it's built so that the decision stays with them rather than with the agent:

- **Off by default.** It isn't listed in `tools/list` and calls are refused until the operator starts mcp-k8s with `--enable-apply-manifest` (`ENABLE_APPLY_MANIFEST=true`; Helm `permissions.enableApplyManifest`; library `ActionPermissions::with_apply_manifest(true)`). An agent can't turn it on. The old `--disable-apply-manifest` still works and wins over `--enable-apply-manifest`.
- **Unrestricted when enabled.** It skips the workload policy entirely: no pod rules, no Service/Ingress/RBAC rules, no kind gates, no custom resource group allowlist. It can create privileged pods, `hostPath` mounts, RBAC bindings, webhooks and CRDs.
- **Still scoped by the operator.** The namespace allowlist applies to the manifest's namespace and to the `namespace` argument (which fills in `metadata.namespace` when the manifest has none and must match it otherwise), and the per-resource switches apply: it looks the object up first, so creating it needs `<resource>-create` and updating an existing one needs `<resource>-update`, with the typed tools' keys (e.g. `deployment-update`). `--disable-create` removes it.
- **The human confirms each call.** Its tool definition carries the MCP annotation `destructiveHint: true`, so clients such as Claude Code ask the user before running it. Its description and the server instructions tell agents to use it only when the user explicitly asks for a manifest to be applied, to show the user the full manifest first, and never to use it to get around a rejection or a missing typed-tool field.
- A manifest without a managed-by label gets `app.kubernetes.io/managed-by: mcp-k8s`.

**Why not guard it like the other tools?** A guarded `apply_manifest` (as in early v0.6.0 drafts) is just a worse typed tool, and the risky case is an agent that's confident while the person behind it doesn't know what the manifest does. An operator opt-in plus per-call human confirmation addresses that directly; a policy only an operator can relax doesn't help a user who really does need a raw manifest.

**Recommendation.** Leave it off for shared or agent-driven deployments; agents get everything they need from the typed tools (see [Typed Tool Coverage](./tools/coverage.md)). Enable it for a person who manages the cluster and wants a raw escape hatch, ideally with a narrow `--namespaces` list.

## How It Works Internally

The `ActionPermissions` struct maintains:

- Three global flags: `global_create_enabled`, `global_update_enabled`, `global_delete_enabled`
- A `resource_overrides` map of `"<resource>-<action>"` -> `false` entries
- `secret_decode_enabled` and `apply_manifest_enabled`
- The `WorkloadPolicy` (see [Workload Guardrails](#workload-guardrails))

When `tool_definitions()` is called (in response to `tools/list`), each tool definition is passed through `permissions.is_tool_allowed(name)`. Tools that are not allowed are filtered out of the response.

The resource name is extracted from the tool name by stripping the action prefix:

| Tool Name | Resource | Action |
|-----------|----------|--------|
| `create_deployment` | `deployment` | Create |
| `delete_pod` | `pod` | Delete |
| `scale_deployment` | `deployment` | Update |
| `rollback_deployment` | `deployment` | Update |
| `evict_pod` | `pod` | Delete |
| `cordon_node` | `node` | Update |
| `exec_pod` | `pod` | Update |
| `apply_manifest` | `apply_manifest` | Create |

The override key format is `<resource>-<action>`, all lowercase. `apply_manifest` is off unless `--enable-apply-manifest` is set; when it's enabled, `--disable apply_manifest-create` also removes it. Inside `apply_manifest`, the object's own kind is checked against the same keys, e.g. `deployment-create` or `deployment-update`.

## Relationship to Kubernetes RBAC

The mcp-k8s permission system and Kubernetes RBAC serve complementary purposes:

| Layer | Controls | Scope |
|-------|----------|-------|
| **mcp-k8s permissions** | Which tools are visible to MCP clients | Application-level |
| **Kubernetes RBAC** | What API calls the service account can make | Cluster-level |

For production deployments, use both:

1. Set mcp-k8s permissions to restrict what the AI can attempt
2. Set Kubernetes RBAC to enforce least-privilege access at the API server level

Even if mcp-k8s permissions allow a tool, the Kubernetes API will reject the call if the service account lacks the required RBAC permissions. See [RBAC Setup](./deployment/rbac.md) for ClusterRole definitions.
