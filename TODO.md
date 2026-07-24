# mcp-k8s TODO / Roadmap

## Current State

The server supports **~165 MCP tools** across **38 resource modules** plus core operations,
covering all GA Kubernetes API groups. 325 unit tests pass.

**Features complete:**
- All GA K8s resource types with CRUD operations
- Action permission controls (global + per-resource disable via `--disable-create/update/delete`, `--disable resource-action`, env vars)
- `label_selector` on all list operations
- `field_selector` on key list operations (pods, services, jobs, nodes, secrets, configmaps, endpoints, endpointslices, deployments, events)
- Deployment actions: restart, scale, rollback
- Node operations: metrics, cordon, uncordon, drain
- Pod operations: create, delete, evict (PDB-aware), exec (command execution with stdout/stderr capture)
- Auth tools: can_i, whoami, list_my_permissions
- Generic tools: apply_manifest (server-side apply), get_resource_yaml (raw JSON output)
- Swagger UI at `/swagger-ui` (HTTP mode)
- Helm chart (`helm/mcp-k8s/`)
- Raw K8s manifests with Kustomize + read-only overlay (`helm/charts/mcp-k8s/`)
- GitHub Actions CI/CD (`.github/workflows/`)
- RBAC documentation with full-access, read-only, and per-namespace ClusterRoles (`docs/RBAC.md`)
- Type consolidation — `crate::types` re-exports all per-resource summary/detail types
- Standardized error handling across all modules

---

## Remaining Work

### Testing
- [ ] Add integration tests against a real or mock K8s cluster (current 325 tests cover extraction, serialization, and permission filtering only — no actual API calls)

### Pod Operations
- [ ] `port_forward` — forward a local port to a pod (requires persistent tunnel, doesn't fit request/response MCP model cleanly — may need a different approach like "check port reachability" instead)

### Alpha/Beta Resources (not GA, implement only on demand)
- [ ] networking.k8s.io/v1beta1 — IPAddress, ServiceCIDR
- [ ] coordination.k8s.io/v1alpha2 — LeaseCandidate
- [ ] certificates.k8s.io/v1alpha1 — ClusterTrustBundle
- [ ] resource.k8s.io/v1beta1 — ResourceClaim, ResourceClaimTemplate, ResourceSlice, DeviceClass
- [ ] storage.k8s.io/v1alpha1/v1beta1 — VolumeAttributesClass
- [ ] storagemigration.k8s.io/v1alpha1 — StorageVersionMigration
- [ ] apiserverinternal.k8s.io/v1alpha1 — StorageVersion
- [ ] admissionregistration.k8s.io/v1alpha1 — MutatingAdmissionPolicy, MutatingAdmissionPolicyBinding
