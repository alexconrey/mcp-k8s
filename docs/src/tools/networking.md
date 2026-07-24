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

Create a Kubernetes Ingress resource. Automatically creates a backing ClusterIP Service if one does not already exist.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | Ingress name |
| `service_name` | string | Yes | Backend service name |
| `host` | string | No | Hostname for the ingress rule (e.g. `myapp.example.com`) |
| `service_port` | integer | No | Backend service port (default: 80) |
| `path` | string | No | URL path (default: `/`) |
| `path_type` | string | No | Path matching type: `Prefix` (default), `Exact`, `ImplementationSpecific` |
| `ingress_class` | string | No | IngressClass name (e.g. `alb`, `nginx`) |
| `annotations` | object | No | Ingress annotations as key-value pairs |

Example:

```json
{
  "namespace": "default",
  "name": "my-app-ingress",
  "service_name": "my-app",
  "host": "my-app.example.com",
  "service_port": 8080,
  "path": "/",
  "ingress_class": "nginx",
  "annotations": {
    "cert-manager.io/cluster-issuer": "letsencrypt-prod"
  }
}
```

### update_ingress

Update an existing Kubernetes Ingress resource (host, paths, annotations, TLS).

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | Ingress name |
| `service_name` | string | Yes | Backend service name |
| `host` | string | No | Hostname for the ingress rule |
| `service_port` | integer | No | Backend service port (default: 80) |
| `path` | string | No | URL path (default: `/`) |
| `path_type` | string | No | Path matching type (default: `Prefix`) |
| `ingress_class` | string | No | IngressClass name |
| `annotations` | object | No | Ingress annotations as key-value pairs |

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
