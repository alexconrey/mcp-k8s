# Networking

This page documents tools for networking resources: Ingresses, IngressClasses, NetworkPolicies, Endpoints, and EndpointSlices.

## Ingresses

### list_ingresses

List Ingresses in a namespace with hosts, classes, and addresses.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `label_selector` | string | No | Label selector |

### get_ingress

Get detailed info for a single Ingress.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | Ingress name |

### create_ingress

Create an Ingress. Preferred over `apply_manifest`. It takes several rules, TLS, a default backend and the ingress class. The original single-rule arguments (`host`, `service_name`, `service_port`, `path`, `path_type`) still work as a shorthand.

By default, a ClusterIP Service selecting `app=<service_name>` is created for each numbered backend that doesn't exist yet (`create_missing_services`).

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | Ingress name |
| `rules` | array | No* | `{host, paths: [{path (default /), path_type (default Prefix), service_name, service_port \| service_port_name}]}` |
| `tls` | array | No | `{hosts, secret_name}` |
| `default_backend` | object | No* | `{service_name, service_port \| service_port_name}` |
| `service_name` | string | No* | Single-rule shorthand: backend service |
| `host` | string | No | Single-rule shorthand: hostname (e.g. `myapp.example.com`) |
| `service_port` | integer | No | Single-rule shorthand: backend port (default: 80) |
| `path` | string | No | Single-rule shorthand: URL path (default: `/`) |
| `path_type` | string | No | `Prefix` (default), `Exact`, `ImplementationSpecific` |
| `ingress_class` | string | No | IngressClass name (e.g. `alb`, `nginx`, `traefik`) |
| `annotations` | object | No | Ingress annotations (checked, see below) |
| `labels` | object | No | |
| `create_missing_services` | boolean | No | Default: true |

\*One of `rules`, `default_backend` or `service_name` is required.

**Annotation guardrails.** These are rejected unless the operator sets `--allow-ingress-snippets`:

- `*-snippet(s)` annotations, for any controller;
- characters and words that inject nginx configuration in `nginx.ingress.kubernetes.io/*` values (`;`, `{`, `}`, quotes, backslashes, newlines, `_by_lua`, `load_module`, …);
- `auth-url`/`auth-signin`/`mirror-target`/redirect values that aren't plain http(s) URLs.

This is the IngressNightmare class of attacks (CVE-2025-1097, CVE-2025-1098, CVE-2025-24514). Operators can also set an annotation allowlist or denylist (`--ingress-annotation-allowlist`, `--ingress-annotation-denylist`).

Example:

```json
{
  "namespace": "default",
  "name": "shop",
  "ingress_class": "nginx",
  "rules": [
    { "host": "shop.example.com", "paths": [
      { "path": "/api", "service_name": "api", "service_port_name": "http" },
      { "path": "/", "service_name": "web", "service_port": 80 }
    ] }
  ],
  "tls": [{ "hosts": ["shop.example.com"], "secret_name": "shop-tls" }],
  "annotations": { "cert-manager.io/cluster-issuer": "letsencrypt-prod" }
}
```

### update_ingress

Update an existing Ingress. Preferred over `apply_manifest`. `rules`, `tls` and `default_backend` replace the current values; `annotations` and `labels` merge. The single-rule shorthand replaces all rules with one. The result is dry-run and checked against the guardrails before it is written.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | Ingress name |
| `rules`, `tls`, `default_backend` | array/object | No | As in `create_ingress` |
| `service_name`, `host`, `service_port`, `path`, `path_type` | | No | Single-rule shorthand |
| `ingress_class` | string | No | IngressClass name |
| `annotations`, `labels` | object | No | Merged into the existing ones |

### delete_ingress

Delete an Ingress by name.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | Ingress name |

---

## IngressClasses

### list_ingressclasses

List IngressClasses in the cluster.

No required parameters.

### get_ingressclass

Get detailed info for a single IngressClass.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `name` | string | Yes | IngressClass name |

---

## NetworkPolicies

### list_networkpolicies

List NetworkPolicies in a namespace.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `label_selector` | string | No | Label selector |

### get_networkpolicy

Get detailed info for a single NetworkPolicy.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | NetworkPolicy name |

### create_networkpolicy

Create a NetworkPolicy.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | NetworkPolicy name |
| `pod_selector` | object | No | Label selector for target pods |
| `ingress_rules` | array | No | Ingress rules |
| `egress_rules` | array | No | Egress rules |
| `policy_types` | string[] | No | Policy types: `Ingress`, `Egress` |
| `labels` | object | No | Labels (`app.kubernetes.io/managed-by: mcp-k8s` is added) |
| `annotations` | object | No | Annotations |

### update_networkpolicy

Patch a NetworkPolicy.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | NetworkPolicy name |
| `pod_selector` | object | No | Updated label selector |
| `ingress_rules` | array | No | Updated ingress rules |
| `egress_rules` | array | No | Updated egress rules |

### delete_networkpolicy

Delete a NetworkPolicy.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | NetworkPolicy name |

---

## Endpoints

### list_endpoints

List Endpoints in a namespace.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `label_selector` | string | No | Label selector |

### get_endpoints

Get detailed Endpoints info by name.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | Endpoints name |

---

## EndpointSlices

### list_endpointslices

List EndpointSlices in a namespace.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `label_selector` | string | No | Label selector |

### get_endpointslice

Get detailed EndpointSlice info by name.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | EndpointSlice name |
