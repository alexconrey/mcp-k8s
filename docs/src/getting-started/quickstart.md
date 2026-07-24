# Quick Start

This guide walks you through setting up mcp-k8s with Claude Code and using it to interact with a Kubernetes cluster.

## Prerequisites

- A running Kubernetes cluster (local or remote)
- `kubectl` configured and working (`kubectl get nodes` succeeds)
- [Claude Code](https://docs.anthropic.com/en/docs/claude-code) installed
- mcp-k8s binary installed (see [Installation](./installation.md))

## Step 1: Configure Claude Code

Add mcp-k8s to your Claude Code MCP settings. Create or edit `.claude/settings.json` in your project directory:

```json
{
  "mcpServers": {
    "mcp-k8s": {
      "command": "mcp-k8s",
      "args": []
    }
  }
}
```

Claude Code will automatically start the mcp-k8s server when you begin a conversation.

## Step 2: Verify the Connection

Start Claude Code and ask it to list your namespaces:

> "List the Kubernetes namespaces in my cluster."

Claude Code will call the `get_namespaces` tool and return something like:

```json
[
  { "name": "default", "status": "Active" },
  { "name": "kube-system", "status": "Active" },
  { "name": "kube-public", "status": "Active" }
]
```

## Step 3: Explore Resources

Ask Claude Code to list deployments in a namespace:

> "Show me all deployments in the default namespace."

This calls the `list_deployments` tool:

```json
[
  {
    "name": "nginx",
    "namespace": "default",
    "image": "nginx:1.25",
    "replicas": {
      "desired": 3,
      "ready": 3,
      "available": 3,
      "updated": 3
    }
  }
]
```

## Step 4: Get Detailed Information

Ask for details about a specific deployment:

> "Get details for the nginx deployment in default."

This calls `get_deployment` and returns detailed info including pods, ingresses, and replica sets.

## Step 5: View Logs

Ask to see logs from a pod:

> "Show me the last 50 lines of logs from the nginx pod."

This calls `get_pod_logs` with `tail_lines: 50`.

## Step 6: Create a Resource

Ask Claude Code to create a deployment:

> "Create a deployment called hello-world in the default namespace using the nginx:latest image with 2 replicas."

This calls `create_deployment`:

```json
{
  "name": "hello-world",
  "namespace": "default",
  "image": "nginx:latest",
  "replicas": {
    "desired": 2,
    "ready": 0,
    "available": 0,
    "updated": 2
  }
}
```

## Step 7: Scale and Manage

You can ask Claude Code to perform management operations:

> "Scale the hello-world deployment to 5 replicas."

This calls `scale_deployment` with `replicas: 5`.

> "Restart the hello-world deployment."

This calls `restart_deployment`, which patches the pod template with a restart annotation to trigger a rolling restart.

## Step 8: Check Permissions

Ask what you can do:

> "What Kubernetes permissions do I have in the default namespace?"

This calls `list_my_permissions` and returns the RBAC rules that apply to the current user.

## Step 9: Clean Up

> "Delete the hello-world deployment."

This calls `delete_deployment` and removes the deployment.

## Example Tool Calls

Here are some commonly used tools and their parameters:

### List pods with a label selector

```json
{
  "name": "list_pods",
  "arguments": {
    "namespace": "default",
    "label_selector": "app=nginx"
  }
}
```

### Get pod logs

```json
{
  "name": "get_pod_logs",
  "arguments": {
    "namespace": "default",
    "pod_name": "nginx-abc123",
    "tail_lines": 100,
    "container": "nginx"
  }
}
```

### Apply an arbitrary manifest

```json
{
  "name": "apply_manifest",
  "arguments": {
    "manifest": "apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: my-config\n  namespace: default\ndata:\n  key: value"
  }
}
```

### Check a specific permission

```json
{
  "name": "can_i",
  "arguments": {
    "verb": "delete",
    "resource": "pods",
    "namespace": "production"
  }
}
```

## SSE Transport

In HTTP mode, mcp-k8s exposes an SSE (Server-Sent Events) endpoint at `POST /mcp/sse` alongside the standard `POST /mcp` JSON-RPC endpoint. The SSE endpoint accepts the same JSON-RPC request body but returns the response as an SSE event stream instead of a plain JSON response. This is useful for clients that prefer streaming transport.

```bash
curl -X POST http://localhost:8080/mcp/sse \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}'
```

The response arrives as an SSE `data:` event containing the JSON-RPC response.

## MCP Resources and Prompts

Beyond tools, mcp-k8s supports the MCP `resources` and `prompts` capabilities.

### Resources

Use `resources/list` to discover available `k8s://` resource URIs, then `resources/read` to fetch a specific resource by URI:

```json
{
  "jsonrpc": "2.0", "id": 1,
  "method": "resources/read",
  "params": { "uri": "k8s://default/pods/nginx-abc123" }
}
```

Supported URI patterns:

- `k8s://{namespace}/pods/{name}`
- `k8s://{namespace}/deployments/{name}`
- `k8s://{namespace}/services/{name}`
- `k8s://{namespace}/configmaps/{name}`
- `k8s://{namespace}/secrets/{name}`
- `k8s://{namespace}/statefulsets/{name}`
- `k8s://{namespace}/daemonsets/{name}`
- `k8s://{namespace}/jobs/{name}`
- `k8s://{namespace}/cronjobs/{name}`
- `k8s://{namespace}/ingresses/{name}`
- `k8s://cluster/nodes/{name}`
- `k8s://cluster/namespaces/{name}`

### Prompts

Use `prompts/list` to discover built-in diagnostic prompts, then `prompts/get` to retrieve the prompt messages. Available prompts:

- **`diagnose-pod`** -- Guides the AI through pod diagnosis (status, logs, events)
- **`review-namespace-rbac`** -- Reviews RBAC configuration for a namespace
- **`cluster-health-check`** -- Checks overall cluster health (nodes, system pods, resource pressure)
- **`resource-usage-report`** -- Summarizes CPU/memory usage in a namespace

Example:

```json
{
  "jsonrpc": "2.0", "id": 1,
  "method": "prompts/get",
  "params": { "name": "diagnose-pod", "arguments": { "namespace": "default", "pod_name": "nginx-abc123" } }
}
```

## Multi-Cluster Switching

When mcp-k8s is started with `--contexts`, you can switch between clusters at runtime:

> "List the available Kubernetes clusters."

This calls `list_clusters` and returns:

```
* staging (active)
  production
```

> "Switch to the production cluster."

This calls `switch_cluster` with `name: "production"`. All subsequent tool calls will target the production cluster.

> "Which cluster am I connected to?"

This calls `get_active_cluster` and returns `production`.

## Next Steps

- [Configuration](./configuration.md) -- Fine-tune namespace restrictions and permission controls
- [Tools Reference](../tools/overview.md) -- Browse all 202 available tools
- [Permissions](../permissions.md) -- Lock down operations for production environments
