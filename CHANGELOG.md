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

## [0.4.0] - 2026-11-24

### Added
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

### Changed
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

## [0.3.1] - 2026-08-05

See the [GitHub release](https://github.com/alexconrey/mcp-k8s/releases/tag/v0.3.1).

## [0.3.0] - 2026-07-30

See the [GitHub release](https://github.com/alexconrey/mcp-k8s/releases/tag/v0.3.0).

## [0.2.0] - 2026-07-25

See the [GitHub release](https://github.com/alexconrey/mcp-k8s/releases/tag/v0.2.0).

[Unreleased]: https://github.com/alexconrey/mcp-k8s/compare/v0.4.0...HEAD
[0.4.0]: https://github.com/alexconrey/mcp-k8s/compare/v0.3.1...v0.4.0
[0.3.1]: https://github.com/alexconrey/mcp-k8s/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/alexconrey/mcp-k8s/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/alexconrey/mcp-k8s/releases/tag/v0.2.0
