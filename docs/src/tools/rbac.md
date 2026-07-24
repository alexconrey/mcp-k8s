# RBAC & Auth

This page documents tools for RBAC resources (Roles, RoleBindings, ClusterRoles, ClusterRoleBindings, ServiceAccounts) and authentication/authorization introspection tools.

## Auth Introspection

These tools allow the AI to understand its own access level within the cluster.

### can_i

Check if the current user can perform a specific action on a resource. Creates a SelfSubjectAccessReview. Returns `allowed` (bool), `reason`, and `evaluation_error`.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `verb` | string | Yes | Kubernetes API verb (e.g. `get`, `list`, `create`, `update`, `delete`, `watch`) |
| `resource` | string | Yes | Resource type (e.g. `pods`, `deployments`, `services`) |
| `namespace` | string | No | Namespace to check (omit for cluster-scoped) |
| `subresource` | string | No | Subresource (e.g. `status`, `log`) |

Example:

```json
{
  "verb": "delete",
  "resource": "pods",
  "namespace": "production"
}
```

Response:

```json
{
  "allowed": true,
  "reason": "RBAC: allowed by ClusterRoleBinding \"mcp-k8s\" ...",
  "evaluation_error": null
}
```

### whoami

Identify the current authenticated user. Creates a SelfSubjectReview. Returns username, UID, groups, and extra attributes. Takes no parameters.

### list_my_permissions

List what the current user can do in a namespace. Creates a SelfSubjectRulesReview. Returns resource_rules and non_resource_rules.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Namespace to evaluate rules for |

---

## Roles

### list_roles

List Roles in a namespace.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `label_selector` | string | No | Label selector |

### get_role

Get detailed info for a single Role including its policy rules.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | Role name |

### create_role

Create a Role.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | Role name |
| `rules` | array | Yes | Policy rules (array of `{apiGroups, resources, verbs}`) |

### update_role

Update a Role's policy rules.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | Role name |
| `rules` | array | Yes | Updated policy rules |

### delete_role

Delete a Role.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | Role name |

---

## RoleBindings

### list_rolebindings

List RoleBindings in a namespace.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `label_selector` | string | No | Label selector |

### get_rolebinding

Get detailed info for a single RoleBinding.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | RoleBinding name |

### create_rolebinding

Create a RoleBinding.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | RoleBinding name |
| `role_name` | string | Yes | Name of the Role to bind |
| `subjects` | array | Yes | Subjects (users, groups, or service accounts) |

### delete_rolebinding

Delete a RoleBinding.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | RoleBinding name |

---

## ClusterRoles

### list_clusterroles

List ClusterRoles in the cluster.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `label_selector` | string | No | Label selector |

### get_clusterrole

Get detailed info for a single ClusterRole including its policy rules.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `name` | string | Yes | ClusterRole name |

### create_clusterrole

Create a ClusterRole.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `name` | string | Yes | ClusterRole name |
| `rules` | array | Yes | Policy rules (array of `{apiGroups, resources, verbs}`) |

### update_clusterrole

Update a ClusterRole's policy rules.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `name` | string | Yes | ClusterRole name |
| `rules` | array | Yes | Updated policy rules |

### delete_clusterrole

Delete a ClusterRole.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `name` | string | Yes | ClusterRole name |

---

## ClusterRoleBindings

### list_clusterrolebindings

List ClusterRoleBindings in the cluster.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `label_selector` | string | No | Label selector |

### get_clusterrolebinding

Get detailed info for a single ClusterRoleBinding.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `name` | string | Yes | ClusterRoleBinding name |

### create_clusterrolebinding

Create a ClusterRoleBinding.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `name` | string | Yes | ClusterRoleBinding name |
| `role_name` | string | Yes | Name of the ClusterRole to bind |
| `subjects` | array | Yes | Subjects (users, groups, or service accounts) |

### delete_clusterrolebinding

Delete a ClusterRoleBinding.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `name` | string | Yes | ClusterRoleBinding name |
