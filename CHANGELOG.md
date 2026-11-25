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

Typed workload tools that cover the pod template, workload guardrails on every
typed write tool, and `apply_manifest` turned into an opt-in, unrestricted
escape hatch for users who know what they're applying.

### Breaking changes
- **Workload guardrails, restrictive by default.** The typed create tools, the
  update tools, `create_custom_resource` and `update_custom_resource` now run
  a workload policy (`mcp_k8s::policy::WorkloadPolicy`, on
  `ActionPermissions`). `apply_manifest` doesn't (see below). By default the
  policy rejects:
  - privileged containers, `allowPrivilegeEscalation: true`, capabilities
    beyond the runtime's default set, `hostNetwork`/`hostPID`/`hostIPC`,
    `hostPath`, `hostPort`, volume types outside the safe set (nfs, inline csi,
    gitRepo, …), and Unconfined seccomp/AppArmor;
  - NodePort, LoadBalancer and ExternalName Services, and `externalIPs`;
  - ingress `*-snippet` annotations and nginx config injection in ingress-nginx
    annotation values (IngressNightmare);
  - RBAC escalation: escalate/bind/impersonate, wildcards, aggregation, binding
    cluster-admin or a role with such rules, anonymous subjects, and
    service-account token Secrets;
  - pod-security namespace labels set to `privileged`.

  Each rule has an opt-in flag (`--allow-*`, `MCP_K8S_ALLOW_*`, or a
  `WorkloadPolicy` field), and every rejection names it. Library users who
  pass `ActionPermissions::default()` (deckwatch does) get the restrictive
  policy; `ActionPermissions::default().with_workload_policy(WorkloadPolicy::permissive())`
  restores the old behaviour.
- **`apply_manifest` is off by default.** It's hidden from `tools/list` and
  refused until the operator passes `--enable-apply-manifest`
  (`ENABLE_APPLY_MANIFEST`; Helm `permissions.enableApplyManifest`; library
  `ActionPermissions::with_apply_manifest(true)`). `ActionPermissions::default()`
  now has `apply_manifest_enabled: false`. Enabled, it's still a raw
  server-side apply **without the workload policy**: for users who know exactly
  what they're applying. It's marked `destructiveHint: true`, so MCP clients
  ask the user before each call, and its description and the server
  instructions tell agents to use it only on the user's explicit request,
  after showing them the manifest, and never to get around a rejection.
  `--disable-apply-manifest` is deprecated but still honoured, and wins.
- **`apply_manifest` honours the per-resource switches.** It looks the object
  up and needs `<resource>-create` or, for an existing object,
  `<resource>-update`. `--disable-update` used to be bypassable through it.
  The namespace allowlist still applies too.
- **Custom resource writes need an allowlisted API group**
  (`--allow-custom-resource-groups`), because operators can turn custom
  resources into pods or RBAC objects. This covers `create_custom_resource`,
  `update_custom_resource`. Reads are unaffected. The custom resource tools
  also refuse built-in kinds; use the typed tools.
- **Secure defaults in the typed create tools.** When the caller doesn't set
  them, pods get `runAsNonRoot: true`, seccomp `RuntimeDefault`,
  `allowPrivilegeEscalation: false`, `capabilities.drop: [ALL]` (non-root
  containers) and `automountServiceAccountToken: false` (unless
  `service_account_name` is set). Images that must run as root need
  `security_context: {"run_as_non_root": false}`; `--no-secure-defaults` turns
  the defaults off.
- **`exec_pod` is an update action** (`pod-update`). It used to fall through to
  "read", so read-only mode couldn't turn it off.
- **Typed workload, Service and Ingress tools reject unknown arguments**,
  listing the valid ones; they used to ignore them. camelCase spellings of the
  documented arguments are accepted.
- **Update tools check their result.** `update_deployment` (and its labels,
  annotations and affinity variants, `rollback_deployment`),
  `update_statefulset`, `update_daemonset`, `update_cronjob`, `update_service`,
  `update_ingress`, `update_namespace` and `update_custom_resource` send the
  patch as a server-side dry run first and refuse a result that breaks the
  policy. This includes modifying a workload that already breaks it. Each
  update makes up to three API calls.

### Added
- **Shared pod template arguments** for `create_pod`, `create_job`,
  `create_cronjob`, `create_deployment`, `create_statefulset`,
  `create_daemonset` and their update tools:
  - **containers:** multiple containers; init containers and native sidecars;
    env from values, secrets, configmaps, fields and resources; `env_from`
    with prefix; args; ports; resources including ephemeral-storage; volume
    mounts; liveness/readiness/startup probes (httpGet, tcpSocket, exec, grpc);
    lifecycle hooks; working dir; image pull policy; container securityContext;
  - **pod:** volumes (configMap, secret, emptyDir with medium/sizeLimit, PVC,
    projected incl. service account tokens, downwardAPI; hostPath/nfs/csi
    gated); service account and token automounting; image pull secrets;
    node selector; tolerations; affinity; topology spread; priority class;
    termination grace period; DNS policy/config; host aliases; runtime class;
    hostname/subdomain; process namespace sharing; service links; pod
    securityContext; labels; annotations; pod annotations; custom selectors.

  See `docs/src/tools/pod-template.md`.
- Job settings: `ttl_seconds_after_finished`, `active_deadline_seconds`,
  `completions`, `parallelism`, `completion_mode`, `backoff_limit_per_index`,
  `max_failed_indexes`, `pod_failure_policy`, `success_policy`,
  `pod_replacement_policy`, `suspend`.
- CronJob settings: `time_zone`, `concurrency_policy`, `suspend`,
  `starting_deadline_seconds`, history limits, and all Job settings.
  `update_cronjob` takes all of them plus the pod template.
- Deployment `strategy`, `min_ready_seconds`, `revision_history_limit`,
  `progress_deadline_seconds`.
- StatefulSet `volume_claim_templates`, `pod_management_policy`,
  `update_strategy`, `persistent_volume_claim_retention_policy`.
- DaemonSet `update_strategy`.
- **`create_job_from_cronjob`** (tool #211): run a CronJob now, like
  `kubectl create job --from=cronjob/NAME`.
- **`create_service`**: several named ports (targetPort by number or name),
  headless services, session affinity, traffic policies, traffic
  distribution, IP families, `publish_not_ready_addresses`, labels and
  annotations; NodePort/LoadBalancer/ExternalName behind flags.
  **`update_service`** takes the mutable ones.
- **`create_ingress`** and **`update_ingress`**: several rules, path types,
  backends by port name, TLS, default backend, labels; the single-rule
  arguments still work.
- RBAC rules accept `resource_names`; ClusterRole rules accept
  `non_resource_urls`.
- MCP `instructions` in the `initialize` response, steering agents to the typed
  tools (`mcp_k8s::mcp::SERVER_INSTRUCTIONS` for library users).
- Helm chart: `permissions.enableApplyManifest` and `extraEnv` (to set the
  `MCP_K8S_ALLOW_*` guardrail variables). `permissions.disableApplyManifest`
  is deprecated.
- `ActionPermissions::with_apply_manifest(bool)`.
- Docs: `docs/src/tools/coverage.md` (typed tool coverage matrix),
  `docs/src/tools/pod-template.md`, and a guardrails section in
  `docs/src/permissions.md`.

### Changed
- Every `create_*`/`update_*` tool description says it is preferred over
  `apply_manifest`. `get_resource_json` no longer suggests passing its output
  back to `apply_manifest`.
- `apply_manifest` adds `app.kubernetes.io/managed-by: mcp-k8s` when the
  manifest has no managed-by label. The typed tools now also label pod
  templates with it.
- `create_service`, `create_ingress` and `update_ingress` moved from
  `mcp/handlers.rs` into the `service` and `ingress` resource modules. Their
  names and original arguments are unchanged.

### Fixed
- `update_deployment_affinity` sent `required_node_affinity` terms with
  snake_case keys (`match_expressions`), which the API server dropped, so
  required node affinity was silently not applied.
- `update_deployment`, `update_statefulset`, `update_daemonset` and
  `update_cronjob` patched a container named after the workload even when it
  didn't exist, which added a broken container. They now target the only
  container, or the one named in `container`.
- Numeric and boolean values in the `env` object became empty strings.
- Unknown probe types produced a probe without a handler, which the API
  server rejects. `grpc` probes are now supported.
- Per-resource switches now reach sub-tools: `--disable deployment-update`
  also disables `update_deployment_labels`, `update_deployment_annotations` and
  `update_deployment_affinity`, and `job-create` covers
  `create_job_from_cronjob`.
- `create_ingress` created backing Services even with `service-create`
  disabled.

<!-- From #25 (telemetry). These entries move to `## [0.5.2]` when the 0.5.2
     release PR (#31) merges; drop them from [Unreleased] when merging main then. -->

### Added
- **Native OpenTelemetry telemetry.** Every MCP request produces one SERVER
  span named `{mcp.method.name} {target}` (e.g. `tools/call apply_manifest`),
  one observation of the `mcp.server.operation.duration` histogram (seconds),
  and one log line, named after the OpenTelemetry GenAI MCP conventions
  (Development status, implemented against `semantic-conventions-genai`
  commit `7cb940f`). Tool calls record the tool, `mcp_k8s.action`
  (`read`/`write`/`delete`/`exec`), `mcp_k8s.decision` and
  `mcp_k8s.denied_rule`, `error.type` (`tool_error`, `permission_denied`,
  `unknown_tool`, `cancelled`, or the JSON-RPC code), the target namespace,
  kind, name and API version, and the argument keys. The metric carries only
  low-cardinality attributes. See the Telemetry docs page.
- **Library: `mcp_k8s::telemetry`**, `tracing` only (no `opentelemetry`
  dependency, so embedders on any OTel version get the data):
  - `server_span()`, the request span with every attribute declared;
  - `ToolCall` / `instrument_tool_call()` for an embedder's own tools,
    `record_denial()`, `finish_operation()`, `record_rpc_status()`;
  - `summarize()`, which classifies a call and redacts its arguments to keys
    plus the `ALLOWED_ARGUMENTS` scalars (namespace, name, kind, api_version,
    group, version, replicas, image, container, pod_name, job_name, cluster,
    decode);
  - `semconv`, every attribute and metric name; `ClientInfo`,
    `W3cTraceContext`, `is_known_tool()`.

  `mcp::handle_tool` now instruments every call it serves. Inside an
  embedder's running `ToolCall` it reports into that call instead, so one
  call never yields two spans. A canary test fills every string argument of
  every tool schema with a marker and fails if one outside the allowlist
  reaches telemetry.
- `ActionPermissions::denied_by(tool)` and `action_denied_by(resource,
  action)` name the setting that denies a call (`global:create`,
  `override:deployment-delete`, `apply_manifest:disabled`), for
  `mcp_k8s.denied_rule`. Guardrails report `policy:<setting>` through
  `telemetry::record_denial`.
- **Binary: OTLP export** of traces, the metric and logs over http/protobuf
  (new `otel` feature, enabled by `server`), configured by the standard
  `OTEL_*` variables (`OTEL_EXPORTER_OTLP_ENDPOINT` and per-signal endpoints,
  `_PROTOCOL`, `_HEADERS`, `_TIMEOUT`, `OTEL_SERVICE_NAME` defaulting to
  `mcp-k8s`, `OTEL_RESOURCE_ATTRIBUTES`, `OTEL_TRACES_SAMPLER`,
  `OTEL_*_EXPORTER=none`, `OTEL_SDK_DISABLED`) and off unless an endpoint is
  set. `MCP_K8S_OTEL_FILTER` (default `info`) filters exported spans and logs.
  gRPC isn't supported. TLS uses rustls with the ring provider the binary
  already installs.
- W3C trace context from `params._meta.traceparent`/`tracestate` (preferred)
  and the HTTP `traceparent` header (fallback, or a span link when both are
  present) becomes the request span's parent.
- `initialize` records the client's self-reported `clientInfo` name and
  version and the protocol version. On stdio they're attached to every later
  request; over HTTP, where mcp-k8s issues no `Mcp-Session-Id`, only to the
  `initialize` span.
- Helm: `otel.*` values (`endpoint`, `serviceName`, `clusterName`,
  `resourceAttributes`, `sampler`, `samplerArg`, per-signal switches,
  `headersSecret`, `filter`, `extraEnv`).

### Changed
- **Log output.** Every request ends with an `INFO` line, `tool call
  finished` or `request finished` (outcome, `duration_ms`, decision, target),
  on target `mcp_k8s::telemetry`; `processing request` moved to `DEBUG`. The
  request span is still `mcp_request`, but its fields changed: `method` is now
  `mcp.method.name`, and the random-UUID `trace_id` is now
  `mcp_k8s.request_id`. `trace_id` holds the W3C trace id when traces are
  exported. Embedders get the new `mcp_k8s::telemetry` events too: filter the
  target `mcp_k8s::telemetry::metrics` (metric observations) out of console
  logs.
- The server shuts down gracefully on SIGTERM/SIGINT: HTTP(S) stops
  accepting, finishes in-flight requests and exits; stdio exits. Telemetry is
  flushed first.
- `RUST_LOG` now filters console logs only.

### Fixed
- In stdio mode, logs went to stdout and mixed with the JSON-RPC stream. They
  go to stderr now; HTTP mode still logs to stdout.

### Internal
- Dependabot groups the OpenTelemetry crates (`otel-stack`), which have to
  move together.
- New dependencies, all behind `server` (via the new `otel` feature):
  `opentelemetry`, `opentelemetry_sdk`, `opentelemetry-otlp`,
  `opentelemetry-appender-tracing` (0.33), `tracing-opentelemetry` (0.34) and
  `reqwest` 0.13 (`blocking`, `rustls-no-provider`). The library-only build
  (`--no-default-features`) has no new dependencies.

### Deprecated (announced)
- The Prometheus metrics `mcp_k8s_tool_calls_total`,
  `mcp_k8s_tool_call_duration_seconds` and `mcp_requests_total` are
  unchanged in this release. A later release will deprecate them in favour
  of the OpenTelemetry metric `mcp.server.operation.duration`, and the one
  after that will remove them.

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
