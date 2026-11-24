# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/) (within 0.x, breaking changes bump
the minor version).

Every change that downstream consumers (for example deckwatch, which vendors
the `mcp_k8s` library by git tag) need ships in a tagged GitHub Release. The
release workflow refuses a tag that doesn't match `Cargo.toml`,
`helm/mcp-k8s/Chart.yaml`, and a section in this file.

## [Unreleased]

## [0.5.1] - 2026-11-24

Security and dependency release. No library API changes.

### Security
- Lockfile updates for RustSec advisories: `h2` 0.4.21 (RUSTSEC-2026-0258),
  `rustls` 0.23.46 (RUSTSEC-2026-0285, -0346, -0348, -0349),
  `rustls-webpki` 0.103.17 (RUSTSEC-2026-0382), `webpki-root-certs` 1.0.12
  (RUSTSEC-2026-0383), `tokio` 1.54.1 (RUSTSEC-2026-0386, unsound),
  `url` 2.5.9 (RUSTSEC-2026-0592), and yanked `chacha20` 0.10.1 → 0.10.2.
- `utoipa` 5 → 6 and `utoipa-swagger-ui` 9 → 10 (server feature only). This
  replaces the build-time `zip` 3.0.0 dependency, which had six advisories
  (RUSTSEC-2026-0367, -0368, -0369, -0429, -0545, -0548), with `zip` 9. No API
  changes were needed.

### Changed
- Dependency updates from Dependabot: `tower-http` 0.6 → 0.7 and
  `axum-server` 0.7 → 0.9 (#21), `metrics-exporter-prometheus` 0.17 → 0.18
  (#22), and minor/patch bumps of `serde_json`, `thiserror`, `http`,
  `futures`, `clap`, `uuid` and `tokio-test` (#20). All of these are
  server-feature or dev dependencies except `serde_json`, `thiserror`, `http`
  and `futures`.
- The container image builds with `rust:1.99` (was 1.95). (#19)

### Internal
- Dependabot for cargo, Docker base images and GitHub Actions, plus Dependabot
  alerts and security updates.
- CI runs `cargo deny` (`deny` job): bans, licenses and sources block, and
  advisories only warn. `audit.yaml` runs the advisory check weekly and fails
  on findings. `deny.toml` now denies yanked crates, wildcard versions and
  unknown registries and git sources, and allows `CDLA-Permissive-2.0`
  (Mozilla CA data in `webpki-root-certs`). (#18, #23)
- Router tests for `/openapi.json` and the Swagger UI. (#18)

## [0.5.0] - 2026-11-24

kube 5 / k8s-openapi 0.29. Requires Kubernetes 1.33 or newer.

### Breaking changes
- **kube 5 and k8s-openapi 0.29 types in the public API.** `K8sClient::new`
  takes a kube 5 `kube::Client`, `K8sClient::inner()` returns one, and
  `Error::Kube` wraps a kube 5 `kube::Error`. Consumers must move to `kube` 5
  and `k8s-openapi` 0.29 together with this release.
- **Kubernetes 1.33 or newer.** k8s-openapi 0.29 supports Kubernetes 1.33–1.37.
  The `mcp-k8s` binary compiles against the oldest, `v1_33`. The Helm chart
  declares `kubeVersion: ">=1.33.0-0"`.
- **The library no longer enables a k8s-openapi version feature.** It used to
  hard-enable `v1_32`. k8s-openapi allows only one `v1_*` feature per build, so
  now the final binary chooses:
  - the `mcp-k8s` binary, through the default `server` feature
    (`k8s-openapi/v1_33`);
  - library consumers (`default-features = false`), in their own `Cargo.toml`,
    e.g. `k8s-openapi = { version = "0.29", features = ["v1_33"] }`.

  The library compiles with `v1_33` and `v1_34`, but not yet with `v1_35` or
  newer (StorageVersionMigration `v1alpha1` is gone there).
- **Rust 1.95 or newer**, kube 5's minimum. The Docker build uses `rust:1.95`.
- **Tools that moved to another API version.** Tool names, parameters and
  output fields are unchanged.
  - `list_ipaddresses`, `get_ipaddress`, `list_servicecidrs`,
    `get_servicecidr`: `networking.k8s.io/v1beta1` → `networking.k8s.io/v1`
    (GA since Kubernetes 1.33 and served by default).
  - `list_clustertrustbundles`, `get_clustertrustbundle`:
    `certificates.k8s.io/v1alpha1` → `certificates.k8s.io/v1beta1`.
  - `list_leasecandidates`, `get_leasecandidate`:
    `coordination.k8s.io/v1alpha2` → `coordination.k8s.io/v1beta1`.
  - `list_resourceclaims`, `get_resourceclaim`, `list_resourceclaimtemplates`,
    `list_resourceslices`, `list_deviceclasses`, `get_deviceclass`:
    `resource.k8s.io/v1beta1` → `resource.k8s.io/v1beta2`. The `spec` that
    `get_resourceclaim` returns has the v1beta2 shape: a request's device class
    is under `exactly`, or under the `firstAvailable` alternatives.
    `device_class_name` reads both.

  These stay on their API version, the newest in k8s-openapi `v1_33`:
  MutatingAdmissionPolicy and MutatingAdmissionPolicyBinding
  (`admissionregistration.k8s.io/v1alpha1`), StorageVersion
  (`internal.apiserver.k8s.io/v1alpha1`), StorageVersionMigration
  (`storagemigration.k8s.io/v1alpha1`) and VolumeAttributesClass
  (`storage.k8s.io/v1beta1`). No tools were removed or renamed.

### Known limitations
- Clusters serve alpha APIs, and beta APIs added since Kubernetes 1.24, only
  when they are enabled with `--runtime-config`. A default Kubernetes 1.35
  cluster serves only the GA versions. There the alpha/beta tools above return
  the API server's 404, including the DRA tools (GA `resource.k8s.io/v1`
  since 1.34) and the VolumeAttributesClass tools (GA `storage.k8s.io/v1`
  since 1.34). Using the GA versions needs a `v1_34` floor.
- StorageVersionMigration `v1alpha1` was removed in Kubernetes 1.35 (replaced by
  `v1beta1`). `list_storageversionmigrations` and `get_storageversionmigration`
  only work on 1.33 and 1.34 clusters with the alpha API enabled.

### Changed
- `kube` 4 → 5.1, `k8s-openapi` 0.28 → 0.29.

### Internal
- CI checks that the library's own k8s-openapi dependency enables no version
  feature, lints the library-only build with `--features k8s-openapi/v1_33`,
  and also runs the tests with `--no-default-features`. `make check` runs the
  library-only steps too (`clippy-lib`, `test-lib`).
- The k3s integration tests run against `rancher/k3s:v1.35.10-k3s1` instead of
  testcontainers' default v1.28 (override with `MCP_K8S_K3S_TAG`). New tests
  check the minimum server version and the `networking.k8s.io/v1` tools, and
  run every `list_*` tool against the server: it must decode the response, or
  get a 404 for an API the server doesn't serve.

## [0.4.0] - 2026-11-24

### Added
- **`get_resource_json`**, the honest name for the generic "get any resource"
  tool: it returns the full object as JSON. Edit the output and pass it back to
  `apply_manifest`, or convert it to YAML client-side if a person needs YAML.
- **`server` feature** (on by default). The standalone HTTP server's
  dependencies (`axum`, `axum-server`, `clap`, `tower-http`,
  `tracing-subscriber`, the Prometheus exporter, `utoipa`,
  `utoipa-swagger-ui`, `uuid`, `subtle`, `rustls`) are now optional, and the
  `mcp-k8s` binary requires the feature. Library users can depend with
  `default-features = false` to skip them. In particular this skips
  `utoipa-swagger-ui`'s build-time `zip` dependency and its security
  advisories. (#12)
- `readiness`, `liveness` and `startup` probe support in `update_deployment`. (#11)
- `envFrom` support in `create_deployment` and `update_deployment`. (#10)
- `update_deployment_labels`, `update_deployment_annotations` and
  `update_deployment_affinity` tools. (#9)

### Deprecated
- **`get_resource_yaml`** is now an alias for `get_resource_json`, with the
  same arguments and output. Despite its name it always returned JSON. It will
  be removed in the next minor release.

### Changed
- **`apply_manifest` is described as a last resort.** Its description now
  steers agents to the typed `create_*`/`update_*` tools, which validate input
  and enforce mcp-k8s's guardrails, whereas `apply_manifest` sends raw objects.
  It also states correctly that only **JSON** manifests are accepted. It
  previously claimed "YAML or JSON", but the parser has always rejected YAML.
  The documentation's YAML examples, which could never have worked, are now
  JSON.
- The documented tool count is corrected to 210, including the alias. It said
  202 while the code already had 209.
- Tool-call metrics are recorded inside the library's `handle_tool()`, so
  library consumers get `mcp_k8s_tool_calls_total` and friends without
  wrapping every call. (#7)
- The crate and Helm chart versions now match the release tag. Both said
  `0.1.0` through v0.3.1.

### Internal
- Metrics tests and rustfmt fixes. (#8)
- CI lints the library-only build (`cargo clippy --lib --no-default-features`). (#12)
- The release workflow verifies that the tag matches `Cargo.toml`,
  `Chart.yaml` and this changelog before publishing.

### Fixed
- Docs listed the old metric names (`mcp_tool_calls_total`,
  `mcp_tool_call_errors_total`). The tool metrics are
  `mcp_k8s_tool_calls_total{tool,status}` and
  `mcp_k8s_tool_call_duration_seconds{tool}`. The documented tool
  count was also stale (202). (#15)

## [0.3.1] - 2026-08-05

See the [GitHub release](https://github.com/alexconrey/mcp-k8s/releases/tag/v0.3.1).

## [0.3.0] - 2026-07-30

See the [GitHub release](https://github.com/alexconrey/mcp-k8s/releases/tag/v0.3.0).

## [0.2.0] - 2026-07-25

See the [GitHub release](https://github.com/alexconrey/mcp-k8s/releases/tag/v0.2.0).

[Unreleased]: https://github.com/alexconrey/mcp-k8s/compare/v0.5.0...HEAD
[0.5.0]: https://github.com/alexconrey/mcp-k8s/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/alexconrey/mcp-k8s/compare/v0.3.1...v0.4.0
[0.3.1]: https://github.com/alexconrey/mcp-k8s/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/alexconrey/mcp-k8s/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/alexconrey/mcp-k8s/releases/tag/v0.2.0
