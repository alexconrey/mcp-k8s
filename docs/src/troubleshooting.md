# Troubleshooting

Common issues and solutions when running mcp-k8s.

## Connection refused

**Symptom**: mcp-k8s fails to start or tools return connection errors like `"connection refused"` or `"Failed to create Kubernetes client"`.

**Causes and fixes**:

- **KUBECONFIG not set**: Ensure the `KUBECONFIG` environment variable points to a valid kubeconfig file, or that `~/.kube/config` exists. In stdio mode, the kubeconfig is read from the environment the process runs in.
- **Cluster unreachable**: Verify the cluster API server is reachable from the machine running mcp-k8s. Test with `kubectl cluster-info`.
- **Wrong context**: If your kubeconfig has multiple contexts, ensure the active context points to the correct cluster. Check with `kubectl config current-context` and switch with `kubectl config use-context <name>`.
- **In-cluster mode**: When running as a pod, mcp-k8s uses the in-cluster service account token automatically. Ensure the pod has a valid service account with the necessary RBAC permissions.

## Namespace not allowed

**Symptom**: Tools return `"Namespace 'foo' is not in the allowed list"`.

**Cause**: mcp-k8s was started with `--namespaces` (or `MCP_K8S_NAMESPACES`) set to a list that does not include the requested namespace.

**Fix**: Either add the namespace to the allowlist or remove the `--namespaces` flag entirely to allow all namespaces.

```bash
# Allow specific namespaces
mcp-k8s --namespaces default,production,staging

# Allow all namespaces (omit the flag)
mcp-k8s
```

To check the current configuration, look at the server's startup logs or the Claude Code MCP server configuration in `.claude.json` / `settings.json`.

## Action not allowed

**Symptom**: A tool call returns an error like `"Action 'delete' is not allowed for resource 'deployment'"` or the tool does not appear in the `tools/list` response.

**Cause**: Permission controls are blocking the operation. This can be a global disable flag or a per-resource override.

**How to check**:

1. Review the server startup flags:
   - `--disable-create` / `DISABLE_CREATE` -- blocks all create operations
   - `--disable-update` / `DISABLE_UPDATE` -- blocks all update operations
   - `--disable-delete` / `DISABLE_DELETE` -- blocks all delete operations
   - `--disable <resource>-<action>` / `MCP_K8S_DISABLE` -- blocks specific resource-action pairs

2. Use the `list_my_permissions` tool to see what the MCP server allows.

3. Use the `tools/list` MCP method -- disabled tools are filtered out of the response entirely.

**Fix**: Remove the relevant disable flag or adjust `--disable` entries. For example, to allow deployment deletions while keeping other deletes disabled:

```bash
mcp-k8s --disable-delete --disable deployment-delete  # This does NOT work --
                                                       # global flag wins unless
                                                       # you remove it.

# Instead, use per-resource disables for fine-grained control:
mcp-k8s --disable pod-delete,service-delete  # Only block pod and service deletes
```

## 401 Unauthorized

**Symptom**: HTTP requests to `/mcp` or `/mcp/sse` return `401 Unauthorized`.

**Cause**: The server was started with `--auth-token` (or `AUTH_TOKEN` env var), which requires all requests (except `/healthz`, `/metrics`, `/swagger-ui`, and `/openapi.json`) to include a valid `Authorization: Bearer <token>` header.

**Fix**:

- Include the token in your requests:
  ```bash
  curl -H "Authorization: Bearer <your-token>" \
       -H "Content-Type: application/json" \
       -d '{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}' \
       http://localhost:8080/mcp
  ```
- Verify the token matches exactly what was passed to `--auth-token`.
- If you do not need authentication, remove the `--auth-token` flag.

## TLS errors

**Symptom**: HTTPS connections fail with certificate errors, or the server fails to start with `"Failed to load TLS cert/key"`.

**Causes and fixes**:

- **Wrong file paths**: Ensure `--tls-cert` and `--tls-key` point to valid PEM-encoded files. Both flags must be provided together.
  ```bash
  mcp-k8s --http --tls-cert /path/to/cert.pem --tls-key /path/to/key.pem
  ```
- **Self-signed certificates**: Clients must trust the CA that signed the certificate. For testing, you can disable TLS verification in your client, but never in production.
- **Certificate/key mismatch**: The certificate and private key must correspond to each other. Verify with:
  ```bash
  openssl x509 -noout -modulus -in cert.pem | openssl md5
  openssl rsa -noout -modulus -in key.pem | openssl md5
  # The two MD5 hashes must match
  ```

## Pod CrashLoopBackOff

**Symptom**: The mcp-k8s pod repeatedly crashes when deployed to Kubernetes.

**Common causes**:

- **GLIBC mismatch**: The default container image uses `gcr.io/distroless/cc-debian12:nonroot`. If you build the binary on a system with a newer glibc than what the base image provides, the pod will crash immediately. **Fix**: Build with `--target x86_64-unknown-linux-musl` for a statically-linked binary, or use a matching base image.
- **Missing RBAC permissions**: The pod's service account needs appropriate Kubernetes RBAC permissions. Check pod logs with `kubectl logs <pod-name>` for permission-denied errors. See the [RBAC Setup](./deployment/rbac.md) guide.
- **Invalid arguments**: Check the pod's command/args for typos in flags. Review logs with `kubectl logs <pod-name>`.
- **Resource limits**: If the pod is OOM-killed, increase memory limits in the deployment spec.

Check events for more detail:

```bash
kubectl describe pod <pod-name>
kubectl get events --field-selector involvedObject.name=<pod-name>
```

## No metrics

**Symptom**: The `get_metrics` tool returns an error or empty results.

**Cause**: The `get_metrics` tool (for pod CPU/memory usage) requires [metrics-server](https://github.com/kubernetes-sigs/metrics-server) to be installed and running in the cluster.

**Fix**:

1. Check if metrics-server is installed:
   ```bash
   kubectl get deployment metrics-server -n kube-system
   ```
2. If not installed, deploy it:
   ```bash
   kubectl apply -f https://github.com/kubernetes-sigs/metrics-server/releases/latest/download/components.yaml
   ```
3. Verify it is working:
   ```bash
   kubectl top pods -n default
   ```

Note: This only affects the `get_metrics` tool. The mcp-k8s server's own `/metrics` Prometheus endpoint works independently and does not require metrics-server.

## Secret values not showing

**Symptom**: `get_secret` returns the secret but data values are redacted or missing, even when `decode: true` is passed.

**Causes**:

- **`--disable-secret-decode` is set**: When the server is started with `--disable-secret-decode` (or `DISABLE_SECRET_DECODE=true`), the `decode: true` parameter is ignored and secret values are never returned. This is a safety feature for production deployments.
- **`decode: true` not passed**: By default, `get_secret` returns metadata and keys but redacts the values. You must explicitly pass `"decode": true` in the tool arguments to see decoded values.

**Fix**: If you need to see secret values, ensure:
1. The server was **not** started with `--disable-secret-decode`.
2. The tool call includes `"decode": true`:
   ```json
   {
     "name": "get_secret",
     "arguments": {
       "namespace": "default",
       "name": "my-secret",
       "decode": true
     }
   }
   ```

## Tool not appearing in tools/list

**Symptom**: A tool you expect to see is missing from the `tools/list` response.

**Causes**:

- **Permission controls filtering it out**: If the tool's action (create, update, or delete) is disabled globally or per-resource, it will not appear in `tools/list`. Check the server's `--disable-create`, `--disable-update`, `--disable-delete`, and `--disable` flags.
- **Read tools are always shown**: List and get tools cannot be disabled and always appear in the response.

**How to verify**:

1. Check the server startup logs for permission configuration.
2. Use `list_my_permissions` to see the server's current permission state.
3. Restart the server without any `--disable-*` flags and check if the tool appears.

## How to check RBAC

If mcp-k8s tools return Kubernetes API errors (403 Forbidden), the issue is likely insufficient RBAC permissions on the service account.

**Using the `can_i` tool**:

The built-in `can_i` tool checks whether the server's Kubernetes identity has permission to perform a specific action:

```json
{
  "name": "can_i",
  "arguments": {
    "verb": "list",
    "resource": "pods",
    "namespace": "default"
  }
}
```

**Using kubectl**:

Check permissions from outside the cluster using the service account identity:

```bash
# Check if the service account can list pods
kubectl auth can-i list pods \
  --as system:serviceaccount:mcp-k8s:mcp-k8s \
  -n default

# Check all permissions for the service account
kubectl auth can-i --list \
  --as system:serviceaccount:mcp-k8s:mcp-k8s \
  -n default
```

**Using `whoami`**:

The `whoami` tool returns the current Kubernetes identity (user, groups, service account) that mcp-k8s is running as. This helps confirm which RBAC bindings apply.

See the [RBAC Setup](./deployment/rbac.md) guide for recommended ClusterRole and ClusterRoleBinding configurations.
