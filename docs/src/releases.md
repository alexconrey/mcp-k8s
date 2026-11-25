# Releases and Compatibility

## Release policy

- **Every change that downstream needs ships in a tagged GitHub Release.** Consumers pin a release, never `main`.
- **Semantic versioning within 0.x:** a minor bump (0.6 → 0.7) for breaking changes to the library API or to behaviour (defaults included); a patch bump otherwise.
- **Cadence:** on demand, and at least monthly while `main` has unreleased changes.
- Each release publishes, from the same commit:
  - the Docker image `ghcr.io/alexconrey/mcp-k8s:vX.Y.Z` (also `:vX.Y` and `:latest`; builds of `main` are `:main` and `:<sha>`),
  - the Helm chart `oci://ghcr.io/alexconrey/charts/mcp-k8s:X.Y.Z` (its default image is `vX.Y.Z`),
  - these docs under `/vX.Y.Z/`,
  - a GitHub Release whose notes are the version's [CHANGELOG](https://github.com/alexconrey/mcp-k8s/blob/main/CHANGELOG.md) section.

  A release is only published if CI passed on the tagged commit and the tag matches `Cargo.toml`, `Chart.yaml` and the CHANGELOG.

## Pinning

As a library (`default-features = false` leaves out the HTTP server stack):

```toml
mcp-k8s = { git = "https://github.com/alexconrey/mcp-k8s.git", tag = "v0.7.0", default-features = false }
```

Your crate must enable exactly one `k8s-openapi` version feature (for example `k8s-openapi = { version = "0.29", features = ["v1_33"] }`) and use the same `kube` major as mcp-k8s, because `K8sClient::new` takes a `kube::Client`.

As a server: pin the image tag `vX.Y.Z` or the chart version `X.Y.Z`. Don't run `:latest` or `:main` in production.

## Compatibility

| mcp-k8s | kube | k8s-openapi (feature) | Minimum Kubernetes | Minimum Rust (library) | Notes |
|---------|------|-----------------------|--------------------|------------------------|-------|
| 0.7.x | 5 | 0.29 (`v1_33`) | 1.33 | 1.95 | Secret values opt-in; MCP tool errors are `isError` results; namespace allowlist enforced everywhere; HTTP browser protections |
| 0.6.x | 5 | 0.29 (`v1_33`) | 1.33 | 1.95 | Workload guardrails restrictive by default; `apply_manifest` off by default |
| 0.5.x | 5 | 0.29 (`v1_33`) | 1.33 | 1.95 | OpenTelemetry telemetry (0.5.2) |
| 0.2.x – 0.4.x | 4 | 0.28 (`v1_32`) | 1.32 | — | |

The k3s integration tests run against Kubernetes 1.35; set `MCP_K8S_K3S_TAG` to test another release, for example the 1.33 minimum.

### deckwatch

deckwatch vendors the library by tag. It's on **0.5.x** and moves to 0.6/0.7 once it exposes the new policy settings: the `WorkloadPolicy`, `with_apply_manifest` and `with_secret_decode`, which `ActionPermissions::default()` now turns off.
