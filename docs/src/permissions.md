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
| **Read** | `list_`, `get_`, `*_logs`, `*_metrics`, `can_i`, `whoami`, `list_my_permissions`, `get_resource_yaml` | `list_deployments`, `get_pod_logs`, `get_metrics` |
| **Create** | `create_`, `apply_manifest` | `create_deployment`, `create_pod`, `apply_manifest` |
| **Update** | `update_`, `scale_`, `restart_`, `rollback_`, `approve_`, `deny_`, `cordon_`, `uncordon_`, `drain_` | `update_deployment`, `scale_deployment`, `cordon_node` |
| **Delete** | `delete_`, `evict_` | `delete_pod`, `evict_pod` |

Unknown tool names default to **Read** as a safe fallback.

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

## How It Works Internally

The `ActionPermissions` struct maintains:

- Three global flags: `global_create_enabled`, `global_update_enabled`, `global_delete_enabled`
- A `resource_overrides` map of `"<resource>-<action>"` -> `false` entries

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
| `apply_manifest` | `apply_manifest` | Create |

The override key format is `<resource>-<action>`, all lowercase. So to disable `apply_manifest`, you would use `--disable apply_manifest-create`.

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
