# Security

mcp-k8s hands an AI agent broad powers over a Kubernetes cluster, so its defaults lean towards refusing. This page collects the protections and the settings that relax them. Everything here applies from **v0.7.0**.

## Defaults at a glance

| Area | Default | Relax with |
|------|---------|------------|
| Workload guardrails (privileged pods, host namespaces, `hostPath`, host ports, extra capabilities, NodePort/LoadBalancer Services, ingress snippets, RBAC escalation, custom resource groups) | Rejected on every typed write tool and the custom resource tools | One `--allow-*` flag per rule ([Permissions](./permissions.md#workload-guardrails)) |
| `apply_manifest` (raw, unrestricted server-side apply) | **Off**, and hidden from `tools/list` | `--enable-apply-manifest` |
| Secret values | **Redacted everywhere**: `get_secret` refuses `decode`, and `get_resource_json` and `k8s://…/secrets/…` resources redact `data`, `stringData` and the `last-applied-configuration` annotation | `--enable-secret-decode` |
| CSR approval for privileged identities (`system:masters`, any `system:` user or group) | Refused | `--allow-privileged-csr` |
| `exec_pod` into pods with node-level access (privileged, host namespaces, `hostPath`, …) | Refused; exec has a 60 s default and 300 s maximum timeout, and a 1 MiB output cap per stream | The matching `--allow-*` flag |
| Namespace allowlist (`--namespaces`) | Off (all namespaces). When set, every namespaced tool refuses other namespaces, and tools that would act across all namespaces (an omitted `namespace`, `drain_node`, namespace objects) refuse too | — |
| HTTP authentication | Optional, with a **startup warning** on a non-loopback address | `--auth-token-file` (or `AUTH_TOKEN`) |
| CORS | **None** | `--cors-allowed-origins` |
| Cross-site browser requests that could change state | **403** | `--cors-allowed-origins` |
| DNS rebinding (`Host` allowlist) | Loopback names on a loopback `--listen`, otherwise any host (with a warning) | `--allowed-hosts` |
| Request size / time | 4 MiB / 360 s | `--max-request-bytes`, `--request-timeout-secs` |

## Deploying safely

1. **Start read-only.** The Helm chart's `rbac.readOnly: true` grants only get/list/watch and sets `--disable-create/update/delete`, so the tool list matches what the ServiceAccount can do. Widen it deliberately.
2. **Scope it.** Set `namespaces` (the namespace allowlist) and, with `rbac.clusterWide: false`, the chart creates a Role per namespace instead of a ClusterRole.
3. **Authenticate.** Put a random token in a Secret and set `http.auth.existingSecret`. The chart mounts it as a file (`AUTH_TOKEN_FILE`), so it never shows up in the pod spec or `ps`.
4. **Name the hosts.** Set `http.allowedHosts` to the names clients use (the Service DNS name, the Ingress host).
5. **Use TLS** in front of it (an Ingress, a service mesh, or `--tls-cert`/`--tls-key`).
6. **Keep the escape hatches off** (`enableApplyManifest`, `enableSecretDecode`, the `--allow-*` flags) unless a person who knows the cluster needs them.
7. **Restrict the network.** `networkPolicy.enabled: true` limits ingress to the MCP port from chosen sources, and egress to the API server, DNS and the OTLP collector.

The pod already runs as non-root (65534) with a read-only root filesystem, no capabilities and the `RuntimeDefault` seccomp profile.

## Audit trail

Every tool call that writes, deletes or execs, and every refused call, emits an `audit` event on the `tracing` target `mcp_k8s::audit` with the tool, action, decision and denied rule (`global:create`, `policy:allow_privileged`, `namespace_allowlist`, …), outcome and target. Argument values are never recorded. See [Telemetry](./telemetry.md#audit-events).

## Agents and refusals

A refused or failed tool call returns an MCP result with `isError: true` and a message that names the setting that would allow it. The server instructions tell agents to report a refusal to the user rather than work around it with another tool. `apply_manifest`, when enabled, carries the MCP `destructiveHint` annotation, so clients such as Claude Code ask the user before each call.

## Relationship to cluster security

These protections keep the AI-facing tools inside sensible lines; they don't replace Kubernetes RBAC, Pod Security Admission or an admission policy engine. The ServiceAccount's RBAC is the hard boundary: mcp-k8s can never do more than it allows.

## Reporting a vulnerability

Open a [GitHub security advisory](https://github.com/alexconrey/mcp-k8s/security/advisories/new) rather than a public issue.
