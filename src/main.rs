use std::io::BufRead;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::{ConnectInfo, Request, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::middleware::Next;
use axum::response::sse::{Event, Sse};
use axum::response::IntoResponse;
use axum::Json;
use clap::Parser;
use futures::stream;
use metrics::counter;
use metrics_exporter_prometheus::PrometheusHandle;
use subtle::ConstantTimeEq;
use tower_http::cors::CorsLayer;
use tracing::{Instrument, Span};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::Layer;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;
use uuid::Uuid;

use mcp_k8s::mcp::{
    error_response, method_not_found, success_response, JsonRpcError, JsonRpcRequest,
    JsonRpcResponse,
};
use mcp_k8s::permissions::ActionPermissions;
use mcp_k8s::policy::WorkloadPolicy;
use mcp_k8s::telemetry::{self, semconv, ClientInfo, ToolCall, W3cTraceContext};
use mcp_k8s::{ClusterManager, K8sClient, ResponseCache};

mod otel;

/// The MCP protocol version this server speaks.
const PROTOCOL_VERSION: &str = "2025-11-25";
/// The prompts `prompts/get` serves (span names use only these).
const PROMPTS: &[&str] = &[
    "diagnose-pod",
    "review-namespace-rbac",
    "cluster-health-check",
    "resource-usage-report",
];

#[derive(OpenApi)]
#[openapi(
    info(
        title = "mcp-k8s",
        version = env!("CARGO_PKG_VERSION"),
        description = "Kubernetes MCP (Model Context Protocol) server. Exposes Kubernetes \
                       cluster operations as MCP tools over JSON-RPC 2.0.",
        license(name = "MIT"),
    ),
    paths(handle_mcp_http, healthz),
    components(schemas(JsonRpcRequest, JsonRpcResponse, JsonRpcError)),
    tags(
        (name = "mcp", description = "MCP JSON-RPC endpoint"),
        (name = "health", description = "Health check"),
    )
)]
struct ApiDoc;

#[derive(Parser)]
#[command(name = "mcp-k8s", about = "Kubernetes MCP server")]
struct Cli {
    /// Run in HTTP server mode (for in-cluster deployment).
    /// Without this flag, the server runs in stdio mode.
    #[arg(long)]
    http: bool,

    /// HTTP listen address (only used with --http).
    #[arg(long, default_value = "0.0.0.0:8080", env = "MCP_K8S_LISTEN")]
    listen: String,

    /// Comma-separated list of allowed namespaces.
    /// Empty means all namespaces are allowed.
    #[arg(long, env = "MCP_K8S_NAMESPACES", value_delimiter = ',')]
    namespaces: Vec<String>,

    /// Globally disable create actions.
    #[arg(long, env = "DISABLE_CREATE")]
    disable_create: bool,

    /// Globally disable update actions.
    #[arg(long, env = "DISABLE_UPDATE")]
    disable_update: bool,

    /// Globally disable delete actions.
    #[arg(long, env = "DISABLE_DELETE")]
    disable_delete: bool,

    /// Comma-separated list of per-resource action overrides to disable
    /// (e.g. "deployment-delete,pod-create").
    #[arg(long = "disable", env = "MCP_K8S_DISABLE", value_delimiter = ',')]
    disable_actions: Vec<String>,

    /// Bearer token for HTTP endpoint authentication.
    /// When set, all requests (except /healthz and /swagger-ui) must include
    /// an `Authorization: Bearer <token>` header.
    #[arg(long, env = "AUTH_TOKEN")]
    auth_token: Option<String>,

    /// Path to TLS certificate PEM file (enables HTTPS when paired with --tls-key).
    #[arg(long, env = "TLS_CERT")]
    tls_cert: Option<String>,

    /// Path to TLS private key PEM file (enables HTTPS when paired with --tls-cert).
    #[arg(long, env = "TLS_KEY")]
    tls_key: Option<String>,

    /// Disable secret value decoding. When set, `decode: true` in get_secret
    /// is ignored and secret values are never returned.
    #[arg(long, env = "DISABLE_SECRET_DECODE")]
    disable_secret_decode: bool,

    /// Enable the apply_manifest tool (off by default). It's a raw server-side
    /// apply that skips the workload guardrails below (the namespace allowlist
    /// and the --disable switches still apply), for users who know exactly
    /// what they're applying. MCP clients are told it's destructive.
    #[arg(long, env = "ENABLE_APPLY_MANIFEST")]
    enable_apply_manifest: bool,

    /// Keep apply_manifest disabled even if --enable-apply-manifest is set.
    /// It's off by default since v0.6.0; this flag still works so existing
    /// deployments that pass it keep starting.
    #[arg(long, env = "DISABLE_APPLY_MANIFEST", hide = true)]
    disable_apply_manifest: bool,

    /// Log output format: "text" (default) or "json" for structured JSON logging.
    #[arg(long, default_value = "text", env = "LOG_FORMAT")]
    log_format: String,

    /// Response cache TTL in seconds for list operations.
    /// Set to 0 (default) to disable caching.
    #[arg(long, env = "CACHE_TTL", default_value = "0")]
    cache_ttl: u64,

    // --- Workload guardrails (see docs/src/permissions.md). The typed tools and
    // the custom resource tools enforce them (apply_manifest, when enabled,
    // doesn't); each flag relaxes one rule.
    /// Allow privileged containers (and Windows hostProcess, and namespaces
    /// labelled pod-security.kubernetes.io/*=privileged).
    #[arg(long, env = "MCP_K8S_ALLOW_PRIVILEGED")]
    allow_privileged: bool,

    /// Allow allowPrivilegeEscalation: true.
    #[arg(long, env = "MCP_K8S_ALLOW_PRIVILEGE_ESCALATION")]
    allow_privilege_escalation: bool,

    /// Allow hostNetwork, hostPID and hostIPC.
    #[arg(long, env = "MCP_K8S_ALLOW_HOST_NAMESPACES")]
    allow_host_namespaces: bool,

    /// Allow hostPath volumes.
    #[arg(long, env = "MCP_K8S_ALLOW_HOST_PATH")]
    allow_host_path: bool,

    /// Allow hostPort on container ports.
    #[arg(long, env = "MCP_K8S_ALLOW_HOST_PORTS")]
    allow_host_ports: bool,

    /// Allow unconfined seccomp/AppArmor profiles, custom SELinux options,
    /// procMount Unmasked, unsafe sysctls, and probe/hook hosts other than
    /// localhost.
    #[arg(long, env = "MCP_K8S_ALLOW_UNCONFINED")]
    allow_unconfined: bool,

    /// Comma-separated capabilities containers may add beyond the runtime's
    /// default set (e.g. "NET_ADMIN,SYS_TIME"; "ALL" allows any).
    #[arg(long, env = "MCP_K8S_ALLOW_CAPABILITIES", value_delimiter = ',')]
    allow_capabilities: Vec<String>,

    /// Comma-separated volume types allowed beyond configMap, secret,
    /// emptyDir, persistentVolumeClaim, projected, downwardAPI and ephemeral
    /// (e.g. "nfs,csi"; "*" allows any except hostPath).
    #[arg(long, env = "MCP_K8S_ALLOW_VOLUME_TYPES", value_delimiter = ',')]
    allow_volume_types: Vec<String>,

    /// Comma-separated image registry allowlist (e.g.
    /// "ghcr.io/myorg,docker.io/library"). Empty allows any registry.
    #[arg(long, env = "MCP_K8S_ALLOWED_REGISTRIES", value_delimiter = ',')]
    allowed_registries: Vec<String>,

    /// Allow NodePort Services.
    #[arg(long, env = "MCP_K8S_ALLOW_NODEPORT")]
    allow_nodeport: bool,

    /// Allow LoadBalancer Services, loadBalancer* fields and cloud
    /// load-balancer annotations.
    #[arg(long, env = "MCP_K8S_ALLOW_LOADBALANCER")]
    allow_loadbalancer: bool,

    /// Allow ExternalName Services and spec.externalIPs.
    #[arg(long, env = "MCP_K8S_ALLOW_EXTERNAL_SERVICES")]
    allow_external_services: bool,

    /// Allow *-snippet ingress annotations and nginx configuration syntax in
    /// ingress-nginx annotation values.
    #[arg(long, env = "MCP_K8S_ALLOW_INGRESS_SNIPPETS")]
    allow_ingress_snippets: bool,

    /// Comma-separated Ingress annotation keys to accept (trailing * matches a
    /// prefix). When set, every other annotation is rejected.
    #[arg(
        long,
        env = "MCP_K8S_INGRESS_ANNOTATION_ALLOWLIST",
        value_delimiter = ','
    )]
    ingress_annotation_allowlist: Vec<String>,

    /// Comma-separated Ingress annotation keys to reject (trailing * matches a
    /// prefix).
    #[arg(
        long,
        env = "MCP_K8S_INGRESS_ANNOTATION_DENYLIST",
        value_delimiter = ','
    )]
    ingress_annotation_denylist: Vec<String>,

    /// Comma-separated kinds the custom resource tools may write although they
    /// are rejected by default (cluster-scoped kinds). "Kind" or "group/Kind";
    /// "*" allows all.
    #[arg(long, env = "MCP_K8S_ALLOW_MANIFEST_KINDS", value_delimiter = ',')]
    allow_manifest_kinds: Vec<String>,

    /// Comma-separated custom resource API groups that may be written
    /// (e.g. "cert-manager.io"; "*" allows any). Default: none.
    #[arg(
        long,
        env = "MCP_K8S_ALLOW_CUSTOM_RESOURCE_GROUPS",
        value_delimiter = ','
    )]
    allow_custom_resource_groups: Vec<String>,

    /// Allow RBAC escalation primitives (escalate/bind/impersonate verbs,
    /// wildcards, aggregation, binding cluster-admin or anonymous subjects,
    /// service-account token Secrets).
    #[arg(long, env = "MCP_K8S_ALLOW_RBAC_ESCALATION")]
    allow_rbac_escalation: bool,

    /// Don't fill in the restricted securityContext and
    /// automountServiceAccountToken: false defaults in the typed create tools.
    #[arg(long, env = "MCP_K8S_NO_SECURE_DEFAULTS")]
    no_secure_defaults: bool,

    /// Comma-separated list of kubeconfig context names to load.
    /// Each context becomes a named cluster that can be switched via MCP tools.
    /// The first context (or the default if none specified) becomes the active cluster.
    #[arg(long, env = "MCP_K8S_CONTEXTS", value_delimiter = ',')]
    contexts: Vec<String>,
}

#[tokio::main]
async fn main() {
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

    let cli = Cli::parse();

    // The OTLP pipeline feeds the subscriber, so build it first and report
    // on it once logging is up. It stays empty unless an endpoint is set.
    let (pipeline, otel_warnings) = otel::Pipeline::from_env();
    init_tracing(&cli.log_format, !cli.http, &pipeline);
    for warning in otel_warnings {
        tracing::warn!("{warning}");
    }
    let signals = pipeline.signals();
    if signals != otel::Signals::default() {
        tracing::info!(
            traces = signals.traces,
            metrics = signals.metrics,
            logs = signals.logs,
            "exporting OTLP over http/protobuf"
        );
    }

    let prometheus_handle = metrics_exporter_prometheus::PrometheusBuilder::new()
        .install_recorder()
        .expect("Failed to install Prometheus recorder");
    // Deprecated since v0.7.0 and removed in v0.8.0: the OpenTelemetry
    // histogram mcp.server.operation.duration carries the same data.
    metrics::describe_counter!(
        "mcp_k8s_tool_calls_total",
        "DEPRECATED (removed in v0.8.0; use the OpenTelemetry metric mcp.server.operation.duration). Tool calls by tool and status."
    );
    metrics::describe_histogram!(
        "mcp_k8s_tool_call_duration_seconds",
        metrics::Unit::Seconds,
        "DEPRECATED (removed in v0.8.0; use the OpenTelemetry metric mcp.server.operation.duration). Tool call duration."
    );
    metrics::describe_counter!(
        "mcp_requests_total",
        "DEPRECATED (removed in v0.8.0; use the OpenTelemetry metric mcp.server.operation.duration). MCP requests by method."
    );

    let policy = workload_policy(&cli);
    let namespaces: Vec<String> = cli
        .namespaces
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect();

    let permissions = ActionPermissions::new(
        cli.disable_create,
        cli.disable_update,
        cli.disable_delete,
        cli.disable_actions,
        !cli.disable_secret_decode,
        cli.enable_apply_manifest && !cli.disable_apply_manifest,
    )
    .with_workload_policy(policy);

    let contexts: Vec<String> = cli.contexts.into_iter().filter(|s| !s.is_empty()).collect();

    let cluster_manager = if contexts.is_empty() {
        // No explicit contexts: use the default kubeconfig context
        let kube_client = kube::Client::try_default()
            .await
            .expect("Failed to create Kubernetes client");
        let client = K8sClient::new(kube_client, namespaces.clone(), permissions.clone());
        ClusterManager::new(client, "default".to_string()).await
    } else {
        // Load each named kubeconfig context as a separate cluster
        let first_name = contexts[0].clone();
        let mut manager: Option<ClusterManager> = None;

        for ctx_name in &contexts {
            let config = kube::Config::from_kubeconfig(&kube::config::KubeConfigOptions {
                context: Some(ctx_name.clone()),
                ..Default::default()
            })
            .await
            .unwrap_or_else(|e| {
                eprintln!("Failed to load kubeconfig context '{ctx_name}': {e}");
                std::process::exit(1);
            });

            let kube_client = kube::Client::try_from(config).unwrap_or_else(|e| {
                eprintln!("Failed to create client for context '{ctx_name}': {e}");
                std::process::exit(1);
            });

            let client = K8sClient::new(kube_client, namespaces.clone(), permissions.clone());

            match &manager {
                None => {
                    manager = Some(ClusterManager::new(client, ctx_name.clone()).await);
                }
                Some(m) => {
                    m.add_cluster(ctx_name.clone(), client).await;
                }
            }
        }

        let manager = manager.unwrap();
        // The first context is active by default (set during ClusterManager::new)
        tracing::info!(
            contexts = ?contexts,
            active = %first_name,
            "loaded {} cluster contexts",
            contexts.len()
        );
        manager
    };

    let cache = if cli.cache_ttl > 0 {
        tracing::info!(ttl = cli.cache_ttl, "response cache enabled");
        ResponseCache::new(cli.cache_ttl, true)
    } else {
        ResponseCache::disabled()
    };

    if cli.http {
        let auth_token = cli.auth_token.clone();
        // Both servers drain in-flight requests on SIGTERM/SIGINT and return.

        match (&cli.tls_cert, &cli.tls_key) {
            (Some(cert_path), Some(key_path)) => {
                run_https(
                    cluster_manager,
                    &cli.listen,
                    cert_path,
                    key_path,
                    auth_token,
                    prometheus_handle,
                    cache,
                )
                .await;
            }
            (None, None) => {
                run_http(
                    cluster_manager,
                    &cli.listen,
                    auth_token,
                    prometheus_handle,
                    cache,
                )
                .await;
            }
            _ => {
                eprintln!("Error: --tls-cert and --tls-key must both be provided for TLS");
                std::process::exit(1);
            }
        }
    } else {
        // Stdio reads stdin synchronously, so a signal can't interrupt it:
        // flush telemetry and exit from a task instead.
        let on_signal = pipeline.clone();
        tokio::spawn(async move {
            shutdown_signal().await;
            on_signal.shutdown();
            std::process::exit(0);
        });
        run_stdio(cluster_manager).await;
    }

    pipeline.shutdown();
}

/// Console logs (`RUST_LOG`, `--log-format`; stderr in stdio mode) plus the
/// OTLP layers. Each layer has its own filter, so `RUST_LOG=warn` quiets the
/// console without starving the exporters.
fn init_tracing(log_format: &str, stdio: bool, pipeline: &otel::Pipeline) {
    let mut console_filter =
        tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into());
    // Metric events carry no message and would print as noise, unless asked for.
    let asked = std::env::var("RUST_LOG").is_ok_and(|v| v.contains(semconv::METRICS_TARGET));
    if !asked {
        if let Ok(directive) = format!("{}=off", semconv::METRICS_TARGET).parse() {
            console_filter = console_filter.add_directive(directive);
        }
    }
    // In stdio mode stdout carries the JSON-RPC stream, so logs go to stderr.
    let writer = if stdio {
        tracing_subscriber::fmt::writer::BoxMakeWriter::new(std::io::stderr)
    } else {
        tracing_subscriber::fmt::writer::BoxMakeWriter::new(std::io::stdout)
    };
    let console: otel::BoxedLayer = if log_format == "json" {
        tracing_subscriber::fmt::layer()
            .json()
            .with_writer(writer)
            .with_filter(console_filter)
            .boxed()
    } else {
        tracing_subscriber::fmt::layer()
            .with_writer(writer)
            .with_filter(console_filter)
            .boxed()
    };
    let mut layers = vec![console];
    layers.extend(pipeline.layers());
    tracing_subscriber::registry().with(layers).init();
}

/// Resolves on SIGINT (Ctrl-C) or, on Unix, SIGTERM.
async fn shutdown_signal() {
    let ctrl_c = async {
        if tokio::signal::ctrl_c().await.is_err() {
            std::future::pending::<()>().await;
        }
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }
    tracing::info!("shutting down");
}

// ---------------------------------------------------------------------------
// Stdio mode
// ---------------------------------------------------------------------------

/// Build the workload guardrail policy from the CLI flags.
fn workload_policy(cli: &Cli) -> WorkloadPolicy {
    fn list(values: &[String]) -> Vec<String> {
        values
            .iter()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .collect()
    }
    let mut policy = WorkloadPolicy::default();
    policy.allow_privileged = cli.allow_privileged;
    policy.allow_privilege_escalation = cli.allow_privilege_escalation;
    policy.allow_host_namespaces = cli.allow_host_namespaces;
    policy.allow_host_path = cli.allow_host_path;
    policy.allow_host_ports = cli.allow_host_ports;
    policy.allow_unconfined = cli.allow_unconfined;
    policy.allowed_capabilities = list(&cli.allow_capabilities);
    policy.allowed_volume_types = list(&cli.allow_volume_types);
    policy.allowed_registries = list(&cli.allowed_registries);
    policy.allow_node_port = cli.allow_nodeport;
    policy.allow_load_balancer = cli.allow_loadbalancer;
    policy.allow_external_services = cli.allow_external_services;
    policy.allow_ingress_snippets = cli.allow_ingress_snippets;
    policy.ingress_annotation_allowlist = list(&cli.ingress_annotation_allowlist);
    policy.ingress_annotation_denylist = list(&cli.ingress_annotation_denylist);
    policy.allowed_manifest_kinds = list(&cli.allow_manifest_kinds);
    policy.allowed_custom_resource_groups = list(&cli.allow_custom_resource_groups);
    policy.allow_rbac_escalation = cli.allow_rbac_escalation;
    policy.secure_defaults = !cli.no_secure_defaults;
    policy
}

async fn run_stdio(manager: ClusterManager) {
    let stdin = std::io::stdin();
    let reader = stdin.lock();
    // stdio is one client for the life of the process: a real session, so
    // the client info from `initialize` is attached to every later request.
    let mut session = RequestContext::default();

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };

        if line.trim().is_empty() {
            continue;
        }

        let request: JsonRpcRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                let err = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": null,
                    "error": { "code": -32700, "message": format!("Parse error: {e}") }
                });
                println!("{}", serde_json::to_string(&err).unwrap());
                continue;
            }
        };

        let initialize =
            (request.method == "initialize").then(|| ClientInfo::from_initialize(&request.params));
        let response = dispatch(&manager, request, &session).await;
        if let Some(client) = initialize {
            session.session_client = Some(client);
            session.protocol_version = Some(PROTOCOL_VERSION.to_string());
        }
        println!("{}", serde_json::to_string(&response).unwrap());
    }
}

// ---------------------------------------------------------------------------
// Bearer token auth middleware
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct AuthState {
    token: Option<String>,
}

async fn auth_middleware(
    State(auth): State<AuthState>,
    request: Request,
    next: Next,
) -> impl IntoResponse {
    if let Some(ref expected) = auth.token {
        let path = request.uri().path();

        let skip = path == "/healthz"
            || path == "/metrics"
            || path.starts_with("/swagger-ui")
            || path == "/openapi.json";

        if !skip {
            let authorized = request
                .headers()
                .get(header::AUTHORIZATION)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.strip_prefix("Bearer "))
                .map(|t| {
                    let expected_bytes = expected.as_bytes();
                    let provided_bytes = t.as_bytes();
                    // Length check leaks length info, but the token length is not
                    // secret and ct_eq requires equal-length slices.
                    expected_bytes.len() == provided_bytes.len()
                        && bool::from(expected_bytes.ct_eq(provided_bytes))
                })
                .unwrap_or(false);

            if !authorized {
                return StatusCode::UNAUTHORIZED.into_response();
            }
        }
    }

    next.run(request).await.into_response()
}

// ---------------------------------------------------------------------------
// HTTP mode
// ---------------------------------------------------------------------------

/// `/openapi.json` and the Swagger UI at `/swagger-ui`.
fn docs_router<S: Clone + Send + Sync + 'static>() -> axum::Router<S> {
    SwaggerUi::new("/swagger-ui")
        .url("/openapi.json", ApiDoc::openapi())
        .into()
}

fn build_router(
    manager: ClusterManager,
    auth_token: Option<String>,
    prometheus_handle: PrometheusHandle,
    cache: ResponseCache,
) -> axum::Router {
    let state = Arc::new(manager);
    let auth_state = AuthState { token: auth_token };

    axum::Router::new()
        .route("/mcp", axum::routing::post(handle_mcp_http))
        .route("/mcp/sse", axum::routing::post(handle_mcp_sse))
        .route("/healthz", axum::routing::get(healthz))
        .route("/metrics", axum::routing::get(metrics_handler))
        .merge(docs_router())
        .layer(axum::middleware::from_fn_with_state(
            auth_state,
            auth_middleware,
        ))
        .layer(axum::Extension(prometheus_handle))
        .layer(axum::Extension(cache))
        .layer(CorsLayer::permissive())
        .with_state(state)
}

async fn run_http(
    manager: ClusterManager,
    listen: &str,
    auth_token: Option<String>,
    prometheus_handle: PrometheusHandle,
    cache: ResponseCache,
) {
    let app = build_router(manager, auth_token, prometheus_handle, cache);

    let listener = tokio::net::TcpListener::bind(listen)
        .await
        .expect("Failed to bind listener");

    tracing::info!("mcp-k8s HTTP server listening on {listen}");

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await
    .expect("Server error");
}

async fn run_https(
    manager: ClusterManager,
    listen: &str,
    cert_path: &str,
    key_path: &str,
    auth_token: Option<String>,
    prometheus_handle: PrometheusHandle,
    cache: ResponseCache,
) {
    let app = build_router(manager, auth_token, prometheus_handle, cache);

    let config = axum_server::tls_rustls::RustlsConfig::from_pem_file(cert_path, key_path)
        .await
        .expect("Failed to load TLS cert/key");

    let addr: std::net::SocketAddr = listen.parse().expect("Invalid listen address");

    tracing::info!("mcp-k8s HTTPS server listening on {listen}");

    let handle = axum_server::Handle::new();
    let on_signal = handle.clone();
    tokio::spawn(async move {
        shutdown_signal().await;
        on_signal.graceful_shutdown(Some(Duration::from_secs(25)));
    });
    axum_server::bind_rustls(addr, config)
        .handle(handle)
        .serve(app.into_make_service_with_connect_info::<SocketAddr>())
        .await
        .expect("HTTPS server error");
}

#[utoipa::path(
    post,
    path = "/mcp",
    tag = "mcp",
    summary = "MCP JSON-RPC 2.0 endpoint",
    description = "Handles MCP protocol methods: initialize, notifications/initialized, \
                   tools/list, tools/call, resources/list, resources/read, prompts/list, \
                   and prompts/get.",
    request_body(content = JsonRpcRequest, content_type = "application/json"),
    responses(
        (status = 200, description = "JSON-RPC response", body = JsonRpcResponse),
    )
)]
async fn handle_mcp_http(
    State(manager): State<Arc<ClusterManager>>,
    peer: Option<axum::Extension<ConnectInfo<SocketAddr>>>,
    headers: HeaderMap,
    Json(request): Json<JsonRpcRequest>,
) -> impl IntoResponse {
    let ctx = RequestContext::http(peer.map(|e| e.0 .0), &headers);
    let response = dispatch(&manager, request, &ctx).await;
    ([(header::CONTENT_TYPE, "application/json")], Json(response)).into_response()
}

#[utoipa::path(
    get,
    path = "/healthz",
    tag = "health",
    summary = "Health check",
    description = "Returns 200 OK when the server is running.",
    responses(
        (status = 200, description = "Server is healthy", body = String, example = json!("ok")),
    )
)]
async fn healthz() -> &'static str {
    "ok"
}

async fn metrics_handler(axum::Extension(handle): axum::Extension<PrometheusHandle>) -> String {
    handle.render()
}

// ---------------------------------------------------------------------------
// SSE transport
// ---------------------------------------------------------------------------

async fn handle_mcp_sse(
    State(manager): State<Arc<ClusterManager>>,
    peer: Option<axum::Extension<ConnectInfo<SocketAddr>>>,
    headers: HeaderMap,
    Json(request): Json<JsonRpcRequest>,
) -> Sse<impl futures::Stream<Item = Result<Event, std::convert::Infallible>>> {
    let ctx = RequestContext::http(peer.map(|e| e.0 .0), &headers);
    let response = dispatch(&manager, request, &ctx).await;
    let json = serde_json::to_string(&response).unwrap_or_default();

    let events = vec![
        Ok(Event::default().data(json).event("message")),
        Ok(Event::default().data("").event("done")),
    ];

    Sse::new(stream::iter(events)).keep_alive(
        axum::response::sse::KeepAlive::new().interval(std::time::Duration::from_secs(15)),
    )
}

// ---------------------------------------------------------------------------
// Shared dispatch
// ---------------------------------------------------------------------------

/// The transport a request arrived on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Transport {
    #[default]
    Stdio,
    Http,
}

/// What the transport knows about a request, beyond the JSON-RPC message.
#[derive(Default)]
struct RequestContext {
    transport: Transport,
    peer: Option<SocketAddr>,
    /// W3C trace context from the HTTP `traceparent`/`tracestate` headers.
    http_trace: Option<W3cTraceContext>,
    /// The negotiated MCP version: the `MCP-Protocol-Version` header over
    /// HTTP, or the `initialize` result on stdio.
    protocol_version: Option<String>,
    /// The client from `initialize`. Only stdio has a session to keep it in;
    /// mcp-k8s doesn't issue `Mcp-Session-Id` over HTTP.
    session_client: Option<ClientInfo>,
}

impl RequestContext {
    fn http(peer: Option<SocketAddr>, headers: &HeaderMap) -> Self {
        let header = |name: &str| headers.get(name).and_then(|v| v.to_str().ok());
        Self {
            transport: Transport::Http,
            peer,
            http_trace: header("traceparent")
                .and_then(|tp| W3cTraceContext::from_parts(tp, header("tracestate"))),
            protocol_version: header("mcp-protocol-version")
                .filter(|v| telemetry::valid_protocol_version(v))
                .map(str::to_string),
            session_client: None,
        }
    }
}

async fn dispatch(
    manager: &ClusterManager,
    request: JsonRpcRequest,
    ctx: &RequestContext,
) -> JsonRpcResponse {
    let started = Instant::now();
    let method = request.method.as_str();
    // Span names may only contain names we serve: a client-supplied tool or
    // prompt name is unbounded.
    let target = match method {
        "tools/call" => request.params["name"]
            .as_str()
            .filter(|t| telemetry::is_known_tool(t)),
        "prompts/get" => request.params["name"]
            .as_str()
            .filter(|p| PROMPTS.contains(p)),
        _ => None,
    };
    let span = telemetry::server_span(method, target);
    span.record(
        semconv::MCP_K8S_REQUEST_ID,
        Uuid::new_v4().to_string().as_str(),
    );
    record_request(&span, manager, &request, ctx).await;
    // The remote parent must be set before the span is first entered, and
    // reading the trace id starts the span, so this order matters.
    otel::set_remote_parent(
        &span,
        W3cTraceContext::from_meta(&request.params).as_ref(),
        ctx.http_trace.as_ref(),
    );
    otel::record_trace_id(&span);

    let request_span = span.clone();
    async {
        counter!("mcp_requests_total", "method" => request.method.clone()).increment(1);
        tracing::debug!("processing request");

        let (response, tool_call_reported) = match method {
            "initialize" => (handle_initialize(&request, &request_span), false),
            "notifications/initialized" => {
                (success_response(&request, serde_json::json!({})), false)
            }
            "ping" => (success_response(&request, serde_json::json!({})), false),
            "tools/list" => (handle_tools_list(manager, &request).await, false),
            "tools/call" => handle_tool_call(manager, &request).await,
            "resources/list" => (handle_resources_list(&request), false),
            "resources/read" => (handle_resources_read(manager, &request).await, false),
            "prompts/list" => (handle_prompts_list(&request), false),
            "prompts/get" => (handle_prompts_get(&request), false),
            _ => (method_not_found(&request), false),
        };

        if let Some(error) = &response.error {
            telemetry::record_rpc_status(&request_span, i64::from(error.code));
        }
        // A ToolCall already emitted this request's metric and log events.
        if !tool_call_reported {
            let error_type = response.error.as_ref().map(|e| {
                if method == "tools/call" {
                    semconv::error_type::UNKNOWN_TOOL.to_string()
                } else {
                    e.code.to_string()
                }
            });
            telemetry::finish_operation(method, started.elapsed(), error_type.as_deref());
        }
        response
    }
    .instrument(span)
    .await
}

/// Record what the transport and the message say about a request.
async fn record_request(
    span: &Span,
    manager: &ClusterManager,
    request: &JsonRpcRequest,
    ctx: &RequestContext,
) {
    let id = match &request.id {
        Some(serde_json::Value::Number(n)) => Some(n.to_string()),
        Some(serde_json::Value::String(s)) => Some(clip(s, 128)),
        _ => None,
    };
    if let Some(id) = id {
        span.record(semconv::JSONRPC_REQUEST_ID, id.as_str());
    }
    match ctx.transport {
        Transport::Stdio => {
            span.record(semconv::NETWORK_TRANSPORT, "pipe");
        }
        Transport::Http => {
            span.record(semconv::NETWORK_TRANSPORT, "tcp");
            span.record(semconv::NETWORK_PROTOCOL_NAME, "http");
        }
    }
    if let Some(peer) = ctx.peer {
        span.record(semconv::CLIENT_ADDRESS, peer.ip().to_string().as_str());
        span.record(semconv::CLIENT_PORT, i64::from(peer.port()));
    }
    // initialize records the version it negotiates itself.
    if request.method != "initialize" {
        if let Some(version) = &ctx.protocol_version {
            span.record(semconv::MCP_PROTOCOL_VERSION, version.as_str());
        }
    }
    if let Some(client) = &ctx.session_client {
        client.record(span);
    }
    if matches!(request.method.as_str(), "tools/call" | "resources/read") {
        // A context name only means something with --contexts. Without it the
        // cluster is called "default"; single-cluster deployments should set
        // k8s.cluster.name as a resource attribute (OTEL_RESOURCE_ATTRIBUTES).
        let name = manager.active_name().await;
        if name != "default" || manager.list_clusters().await.len() > 1 {
            span.record(semconv::K8S_CLUSTER_NAME, name.as_str());
        }
    }
    if request.method == "resources/read" {
        if let Some(uri) = request.params["uri"].as_str() {
            span.record(semconv::MCP_RESOURCE_URI, clip(uri, 256).as_str());
        }
    }
}

/// `s` without control characters, cut to at most `max` bytes.
fn clip(s: &str, max: usize) -> String {
    let mut out = String::new();
    for c in s.chars().filter(|c| !c.is_control()) {
        if out.len() + c.len_utf8() > max {
            break;
        }
        out.push(c);
    }
    out
}

fn handle_initialize(request: &JsonRpcRequest, span: &Span) -> JsonRpcResponse {
    // Self-reported by the client: a label for telemetry, never an identity.
    let client = ClientInfo::from_initialize(&request.params);
    client.record(span);
    span.record(semconv::MCP_PROTOCOL_VERSION, PROTOCOL_VERSION);
    tracing::event!(
        tracing::Level::INFO,
        {
            { semconv::MCP_K8S_CLIENT_NAME } = client.name.as_deref(),
            { semconv::MCP_K8S_CLIENT_VERSION } = client.version.as_deref(),
            requested_protocol_version = client.protocol_version.as_deref(),
            { semconv::MCP_PROTOCOL_VERSION } = PROTOCOL_VERSION,
        },
        "client initialized"
    );
    success_response(
        request,
        serde_json::json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": { "tools": {}, "resources": {}, "prompts": {} },
            "serverInfo": { "name": "mcp-k8s", "version": env!("CARGO_PKG_VERSION") },
            "instructions": mcp_k8s::mcp::SERVER_INSTRUCTIONS
        }),
    )
}

async fn handle_tools_list(manager: &ClusterManager, request: &JsonRpcRequest) -> JsonRpcResponse {
    let client = match manager.active_client().await {
        Some(c) => c,
        None => return error_response(request, -32000, "no active cluster configured"),
    };
    let tools = mcp_k8s::mcp::tool_definitions(client.permissions());
    success_response(request, serde_json::json!({ "tools": tools }))
}

/// Run a `tools/call`. Returns the response and whether a [`ToolCall`]
/// reported the request (its metric and log events).
async fn handle_tool_call(
    manager: &ClusterManager,
    request: &JsonRpcRequest,
) -> (JsonRpcResponse, bool) {
    let params = &request.params;
    let tool_name = params["name"].as_str().unwrap_or("");
    let args = &params["arguments"];

    let body = async {
        match tool_name {
            // Cluster management tools operate on the manager, not a K8s client.
            "list_clusters" | "switch_cluster" | "get_active_cluster" => {
                mcp_k8s::resources::cluster_mgmt::handle_tool(manager, tool_name, args).await
            }
            // All other tools need the active K8s client. handle_tool sees the
            // ToolCall below and reports into it rather than starting its own.
            _ => match manager.active_client().await {
                Some(client) => mcp_k8s::mcp::handle_tool(&client, tool_name, args).await,
                None => Some(Err("no active cluster configured".to_string())),
            },
        }
    };

    // One ToolCall per call of a tool the server knows, cluster tools
    // included. Unknown names get none: they would put arbitrary client input
    // on the metric.
    let (result, reported) = if telemetry::is_known_tool(tool_name) {
        let call = ToolCall::start(tool_name, args);
        let result = call.run(body).await;
        let reported = match &result {
            Some(r) => {
                call.finish(r);
                true
            }
            None => {
                call.abandon();
                false
            }
        };
        (result, reported)
    } else {
        // On the span only (spans tolerate it), never in its name or the metric.
        if !tool_name.is_empty() {
            Span::current().record(semconv::GEN_AI_TOOL_NAME, clip(tool_name, 128).as_str());
        }
        (body.await, false)
    };

    // MCP: a tool that ran and failed (including a refusal) is a successful
    // JSON-RPC response with `isError: true`, so the model sees the message.
    // An unknown tool is a protocol error: invalid params (-32602).
    let response = match result {
        Some(Ok(text)) => success_response(
            request,
            serde_json::json!({
                "content": [{ "type": "text", "text": text }]
            }),
        ),
        Some(Err(e)) => success_response(
            request,
            serde_json::json!({
                "content": [{ "type": "text", "text": e }],
                "isError": true
            }),
        ),
        None => error_response(request, -32602, &format!("Unknown tool: {tool_name}")),
    };
    (response, reported)
}

// ---------------------------------------------------------------------------
// MCP resources/list and resources/read
// ---------------------------------------------------------------------------

fn handle_resources_list(request: &JsonRpcRequest) -> JsonRpcResponse {
    let resources = serde_json::json!({
        "resources": [
            {"uri": "k8s://{namespace}/pods/{name}", "name": "Kubernetes Pod", "mimeType": "application/json"},
            {"uri": "k8s://{namespace}/deployments/{name}", "name": "Kubernetes Deployment", "mimeType": "application/json"},
            {"uri": "k8s://{namespace}/services/{name}", "name": "Kubernetes Service", "mimeType": "application/json"},
            {"uri": "k8s://{namespace}/configmaps/{name}", "name": "Kubernetes ConfigMap", "mimeType": "application/json"},
            {"uri": "k8s://{namespace}/secrets/{name}", "name": "Kubernetes Secret", "mimeType": "application/json"},
            {"uri": "k8s://{namespace}/statefulsets/{name}", "name": "Kubernetes StatefulSet", "mimeType": "application/json"},
            {"uri": "k8s://{namespace}/daemonsets/{name}", "name": "Kubernetes DaemonSet", "mimeType": "application/json"},
            {"uri": "k8s://{namespace}/jobs/{name}", "name": "Kubernetes Job", "mimeType": "application/json"},
            {"uri": "k8s://{namespace}/cronjobs/{name}", "name": "Kubernetes CronJob", "mimeType": "application/json"},
            {"uri": "k8s://{namespace}/ingresses/{name}", "name": "Kubernetes Ingress", "mimeType": "application/json"},
            {"uri": "k8s://cluster/nodes/{name}", "name": "Kubernetes Node", "mimeType": "application/json"},
            {"uri": "k8s://cluster/namespaces/{name}", "name": "Kubernetes Namespace", "mimeType": "application/json"}
        ]
    });
    success_response(request, resources)
}

async fn handle_resources_read(
    manager: &ClusterManager,
    request: &JsonRpcRequest,
) -> JsonRpcResponse {
    let client = match manager.active_client().await {
        Some(c) => c,
        None => return error_response(request, -32000, "no active cluster configured"),
    };
    let uri = request.params["uri"].as_str().unwrap_or("");
    let result = read_k8s_resource(&client, uri).await;
    match result {
        Ok(content) => success_response(
            request,
            serde_json::json!({
                "contents": [{"uri": uri, "mimeType": "application/json", "text": content}]
            }),
        ),
        Err(e) => error_response(request, -32000, &e),
    }
}

async fn read_k8s_resource(client: &K8sClient, uri: &str) -> Result<String, String> {
    let stripped = uri
        .strip_prefix("k8s://")
        .ok_or_else(|| format!("invalid resource URI (must start with k8s://): {uri}"))?;
    let parts: Vec<&str> = stripped.split('/').collect();
    if parts.len() != 3 {
        return Err(format!("invalid resource URI format: {uri}"));
    }
    let (ns_or_cluster, resource_type, name) = (parts[0], parts[1], parts[2]);

    let (api_version, kind) = match resource_type {
        "pods" => ("v1", "Pod"),
        "deployments" => ("apps/v1", "Deployment"),
        "services" => ("v1", "Service"),
        "configmaps" => ("v1", "ConfigMap"),
        "secrets" => ("v1", "Secret"),
        "nodes" => ("v1", "Node"),
        "namespaces" => ("v1", "Namespace"),
        "statefulsets" => ("apps/v1", "StatefulSet"),
        "daemonsets" => ("apps/v1", "DaemonSet"),
        "jobs" => ("batch/v1", "Job"),
        "cronjobs" => ("batch/v1", "CronJob"),
        "ingresses" => ("networking.k8s.io/v1", "Ingress"),
        _ => return Err(format!("unsupported resource type: {resource_type}")),
    };

    let mut args = serde_json::json!({
        "api_version": api_version,
        "kind": kind,
        "name": name,
    });
    if ns_or_cluster != "cluster" {
        args["namespace"] = serde_json::Value::String(ns_or_cluster.to_string());
    }

    mcp_k8s::resources::generic::handle_tool(client, "get_resource_json", &args)
        .await
        .unwrap_or_else(|| Err("tool not found".to_string()))
}

// ---------------------------------------------------------------------------
// MCP prompts/list and prompts/get
// ---------------------------------------------------------------------------

fn handle_prompts_list(request: &JsonRpcRequest) -> JsonRpcResponse {
    success_response(
        request,
        serde_json::json!({
            "prompts": [
                {
                    "name": "diagnose-pod",
                    "description": "Diagnose why a pod is failing or not ready",
                    "arguments": [
                        {"name": "namespace", "description": "Pod namespace", "required": true},
                        {"name": "pod_name", "description": "Pod name", "required": true}
                    ]
                },
                {
                    "name": "review-namespace-rbac",
                    "description": "Review RBAC configuration for a namespace",
                    "arguments": [
                        {"name": "namespace", "description": "Namespace to review", "required": true}
                    ]
                },
                {
                    "name": "cluster-health-check",
                    "description": "Check overall cluster health: nodes, system pods, resource pressure",
                    "arguments": []
                },
                {
                    "name": "resource-usage-report",
                    "description": "Summarize resource usage (CPU/memory) across pods in a namespace",
                    "arguments": [
                        {"name": "namespace", "description": "Namespace to report on", "required": true}
                    ]
                }
            ]
        }),
    )
}

fn handle_prompts_get(request: &JsonRpcRequest) -> JsonRpcResponse {
    let prompt_name = request.params["name"].as_str().unwrap_or("");
    let args = &request.params["arguments"];

    let messages = match prompt_name {
        "diagnose-pod" => {
            let ns = args["namespace"].as_str().unwrap_or("default");
            let pod = args["pod_name"].as_str().unwrap_or("");
            vec![serde_json::json!({
                "role": "user",
                "content": {
                    "type": "text",
                    "text": format!(
                        "Diagnose the pod '{}' in namespace '{}'. \
                         Use the get_pod tool to check its current status and conditions. \
                         Use get_pod_logs to retrieve recent container logs and look for errors. \
                         Use get_events with the namespace '{}' and resource_name '{}' to \
                         check for Warning events (e.g. FailedScheduling, CrashLoopBackOff, \
                         ImagePullBackOff). Identify the root cause and suggest a concrete fix.",
                        pod, ns, ns, pod
                    )
                }
            })]
        }
        "review-namespace-rbac" => {
            let ns = args["namespace"].as_str().unwrap_or("default");
            vec![serde_json::json!({
                "role": "user",
                "content": {
                    "type": "text",
                    "text": format!(
                        "Review the RBAC configuration for namespace '{}'. \
                         Use list_roles to list all Roles in the namespace. \
                         Use list_rolebindings to list all RoleBindings. \
                         Use list_clusterroles and list_clusterrolebindings to check \
                         cluster-level permissions that may apply. \
                         Use list_serviceaccounts to enumerate service accounts in the namespace. \
                         Identify any overly permissive bindings, unused roles, or security concerns.",
                        ns
                    )
                }
            })]
        }
        "cluster-health-check" => {
            vec![serde_json::json!({
                "role": "user",
                "content": {
                    "type": "text",
                    "text": "Perform a cluster health check. \
                         Use list_nodes to check node status, conditions, and resource pressure \
                         (MemoryPressure, DiskPressure, PIDPressure). \
                         Use list_namespaces to list all namespaces. \
                         Use list_pods in the 'kube-system' namespace to verify system pods are \
                         running. Use get_events in 'kube-system' to check for recent warnings. \
                         Summarize the overall cluster health and flag any issues."
                }
            })]
        }
        "resource-usage-report" => {
            let ns = args["namespace"].as_str().unwrap_or("default");
            vec![serde_json::json!({
                "role": "user",
                "content": {
                    "type": "text",
                    "text": format!(
                        "Generate a resource usage report for namespace '{}'. \
                         Use get_metrics with namespace '{}' to retrieve CPU and memory usage \
                         for all pods. Use list_pods in namespace '{}' to get resource requests \
                         and limits for each container. Compare actual usage against \
                         requests/limits. Identify pods that are over-provisioned or \
                         under-provisioned and suggest right-sizing adjustments.",
                        ns, ns, ns
                    )
                }
            })]
        }
        _ => {
            return error_response(request, -32602, &format!("unknown prompt: {prompt_name}"));
        }
    };

    success_response(
        request,
        serde_json::json!({
            "description": format!("Prompt: {prompt_name}"),
            "messages": messages
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower::ServiceExt;

    async fn get(path: &str) -> (axum::http::StatusCode, String) {
        let response = docs_router::<()>()
            .oneshot(
                axum::http::Request::get(path)
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, String::from_utf8_lossy(&body).into_owned())
    }

    #[tokio::test]
    async fn openapi_json_documents_the_mcp_endpoints() {
        let (status, body) = get("/openapi.json").await;
        assert_eq!(status, axum::http::StatusCode::OK);
        let spec: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(spec["info"]["version"], env!("CARGO_PKG_VERSION"));
        assert!(spec["paths"]["/mcp"].is_object(), "{spec}");
        assert!(spec["paths"]["/healthz"].is_object(), "{spec}");
        assert!(spec["components"]["schemas"]["JsonRpcRequest"].is_object());
    }

    #[tokio::test]
    async fn swagger_ui_is_served() {
        let (status, body) = get("/swagger-ui/index.html").await;
        assert_eq!(status, axum::http::StatusCode::OK);
        assert!(body.contains("swagger"), "{body}");
    }
}

// ---------------------------------------------------------------------------
// Telemetry tests: the real dispatch path into in-memory OTel exporters
// ---------------------------------------------------------------------------

#[cfg(test)]
mod telemetry_tests {
    use std::collections::HashMap;

    use http::{Request as HttpRequest, Response as HttpResponse};
    use kube::client::Body;
    use opentelemetry::trace::{SpanId, SpanKind, Status, TraceId};
    use opentelemetry::Value;
    use opentelemetry_sdk::logs::{InMemoryLogExporter, SdkLoggerProvider};
    use opentelemetry_sdk::metrics::data::{AggregatedMetrics, MetricData};
    use opentelemetry_sdk::metrics::{InMemoryMetricExporter, PeriodicReader, SdkMeterProvider};
    use opentelemetry_sdk::trace::{InMemorySpanExporter, SdkTracerProvider, SpanData};
    use serde_json::json;
    use tracing::instrument::WithSubscriber;

    use super::*;

    const TRACEPARENT: &str = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";
    const HTTP_TRACEPARENT: &str = "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01";

    struct Harness {
        spans: InMemorySpanExporter,
        metrics: InMemoryMetricExporter,
        logs: InMemoryLogExporter,
        pipeline: otel::Pipeline,
        dispatch: tracing::Dispatch,
    }

    /// The production layers (`otel::Pipeline::layers`) over in-memory
    /// exporters, with the binary's metric view.
    fn harness() -> Harness {
        let spans = InMemorySpanExporter::default();
        let metrics = InMemoryMetricExporter::default();
        let logs = InMemoryLogExporter::default();
        let pipeline = otel::Pipeline::from_providers(
            Some(
                SdkTracerProvider::builder()
                    .with_simple_exporter(spans.clone())
                    .build(),
            ),
            Some(
                SdkMeterProvider::builder()
                    .with_reader(PeriodicReader::builder(metrics.clone()).build())
                    .with_view(otel::duration_view)
                    .build(),
            ),
            Some(
                SdkLoggerProvider::builder()
                    .with_simple_exporter(logs.clone())
                    .build(),
            ),
        );
        let dispatch =
            tracing::Dispatch::new(tracing_subscriber::registry().with(pipeline.layers()));
        Harness {
            spans,
            metrics,
            logs,
            pipeline,
            dispatch,
        }
    }

    /// A cluster whose API answers `GET /api/v1/namespaces` and 404s
    /// everything else.
    async fn manager(perms: ActionPermissions) -> ClusterManager {
        let (svc, mut handle) = tower_test::mock::pair::<HttpRequest<Body>, HttpResponse<Body>>();
        tokio::spawn(async move {
            while let Some((req, send)) = handle.next_request().await {
                let (status, body) = if req.uri().path() == "/api/v1/namespaces" {
                    (
                        200,
                        json!({"apiVersion": "v1", "kind": "NamespaceList", "metadata": {},
                               "items": [{"metadata": {"name": "default"}}]}),
                    )
                } else {
                    (
                        404,
                        json!({"kind": "Status", "apiVersion": "v1", "metadata": {},
                               "status": "Failure", "message": "not found",
                               "reason": "NotFound", "code": 404}),
                    )
                };
                send.send_response(
                    HttpResponse::builder()
                        .status(status)
                        .header("content-type", "application/json")
                        .body(Body::from(serde_json::to_vec(&body).unwrap()))
                        .unwrap(),
                );
            }
        });
        let client = K8sClient::new(kube::Client::new(svc, "default"), vec![], perms);
        ClusterManager::new(client, "test-cluster".to_string()).await
    }

    fn request(value: serde_json::Value) -> JsonRpcRequest {
        serde_json::from_value(value).unwrap()
    }

    async fn run(
        h: &Harness,
        manager: &ClusterManager,
        req: JsonRpcRequest,
        ctx: &RequestContext,
    ) -> JsonRpcResponse {
        let response = dispatch(manager, req, ctx)
            .with_subscriber(h.dispatch.clone())
            .await;
        h.pipeline.force_flush();
        response
    }

    fn server_spans(h: &Harness) -> Vec<SpanData> {
        h.spans
            .get_finished_spans()
            .unwrap()
            .into_iter()
            .filter(|s| s.span_kind == SpanKind::Server)
            .collect()
    }

    /// The span's attributes. Fails if a key was recorded twice: the SDK
    /// exports every value, so a double `record` is a bug.
    fn attrs(span: &SpanData) -> HashMap<String, Value> {
        let mut map = HashMap::new();
        for kv in &span.attributes {
            let dup = map.insert(kv.key.to_string(), kv.value.clone());
            assert!(dup.is_none(), "{} recorded twice on {}", kv.key, span.name);
        }
        map
    }

    fn s(v: &str) -> Value {
        Value::from(v.to_string())
    }

    /// Every `mcp.server.operation.duration` data point, as (count,
    /// attributes), from the latest export (temporality is cumulative, so it
    /// holds every series).
    fn duration_points(h: &Harness) -> Vec<(u64, HashMap<String, String>)> {
        let mut points = Vec::new();
        let exports = h.metrics.get_finished_metrics().unwrap();
        if let Some(rm) = exports.last() {
            for sm in rm.scope_metrics() {
                for m in sm.metrics() {
                    if m.name() != semconv::MCP_SERVER_OPERATION_DURATION {
                        continue;
                    }
                    assert_eq!(m.unit(), "s");
                    let AggregatedMetrics::F64(MetricData::Histogram(hist)) = m.data() else {
                        panic!("{} is not an f64 histogram", m.name());
                    };
                    for dp in hist.data_points() {
                        let bounds: Vec<f64> = dp.bounds().collect();
                        assert_eq!(bounds, semconv::MCP_SERVER_OPERATION_DURATION_BUCKETS);
                        points.push((
                            dp.count(),
                            dp.attributes()
                                .map(|kv| (kv.key.to_string(), kv.value.to_string()))
                                .collect(),
                        ));
                    }
                }
            }
        }
        points
    }

    #[tokio::test]
    async fn tools_call_yields_one_server_span_one_point_one_log() {
        let h = harness();
        let manager = manager(ActionPermissions::default()).await;
        let response = run(
            &h,
            &manager,
            request(json!({"jsonrpc": "2.0", "id": 7, "method": "tools/call",
                           "params": {"name": "list_namespaces", "arguments": {}}})),
            &RequestContext::default(),
        )
        .await;
        assert!(
            response.error.is_none(),
            "{:?}",
            response.error.map(|e| e.message)
        );

        let spans = server_spans(&h);
        assert_eq!(spans.len(), 1, "exactly one SERVER span");
        let span = &spans[0];
        assert_eq!(span.name, "tools/call list_namespaces");
        assert_eq!(span.status, Status::Unset);
        let a = attrs(span);
        assert_eq!(a[semconv::MCP_METHOD_NAME], s("tools/call"));
        assert_eq!(a[semconv::GEN_AI_TOOL_NAME], s("list_namespaces"));
        assert_eq!(a[semconv::GEN_AI_OPERATION_NAME], s("execute_tool"));
        assert_eq!(a[semconv::MCP_K8S_ACTION], s("read"));
        assert_eq!(a[semconv::MCP_K8S_DECISION], s("allowed"));
        assert_eq!(a[semconv::MCP_K8S_OUTCOME], s("ok"));
        assert_eq!(a[semconv::JSONRPC_REQUEST_ID], s("7"));
        assert_eq!(a[semconv::NETWORK_TRANSPORT], s("pipe"));
        assert_eq!(a[semconv::K8S_CLUSTER_NAME], s("test-cluster"));
        assert_eq!(
            a[semconv::TRACE_ID],
            s(&span.span_context.trace_id().to_string())
        );
        assert!(a.contains_key(semconv::MCP_K8S_REQUEST_ID));
        assert!(!a.contains_key(semconv::ERROR_TYPE));
        assert!(!a.contains_key("otel.name") && !a.contains_key("otel.kind"));

        let points = duration_points(&h);
        assert_eq!(points.len(), 1, "exactly one histogram point: {points:?}");
        let (count, point) = &points[0];
        assert_eq!(*count, 1);
        let expected: HashMap<String, String> = [
            (semconv::MCP_METHOD_NAME, "tools/call"),
            (semconv::GEN_AI_TOOL_NAME, "list_namespaces"),
            (semconv::GEN_AI_OPERATION_NAME, "execute_tool"),
            (semconv::MCP_K8S_ACTION, "read"),
            (semconv::MCP_K8S_DECISION, "allowed"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        assert_eq!(point, &expected, "only low-cardinality attributes");

        let logs = h.logs.get_emitted_logs().unwrap();
        let finished: Vec<_> = logs
            .iter()
            .filter(|l| {
                l.record
                    .body()
                    .is_some_and(|b| format!("{b:?}").contains("tool call finished"))
            })
            .collect();
        assert_eq!(finished.len(), 1);
        let cx = finished[0]
            .record
            .trace_context()
            .expect("log has trace context");
        assert_eq!(cx.trace_id, span.span_context.trace_id());
        assert_eq!(cx.span_id, span.span_context.span_id());
    }

    #[tokio::test]
    async fn denied_call_sets_decision_rule_and_error_type() {
        let h = harness();
        let manager = manager(ActionPermissions::new(
            true,
            false,
            false,
            vec![],
            true,
            true,
        ))
        .await;
        let response = run(
            &h,
            &manager,
            request(json!({"jsonrpc": "2.0", "id": "a1", "method": "tools/call",
                           "params": {"name": "create_deployment",
                                      "arguments": {"namespace": "prod", "name": "web",
                                                    "image": "nginx", "env": {"PASSWORD": "hunter2"}}}})),
            &RequestContext::default(),
        )
        .await;
        // A refused tool call is a tool error (isError), not a JSON-RPC error.
        assert!(response.error.is_none());
        let result = response.result.as_ref().unwrap();
        assert_eq!(result["isError"], true);
        assert!(result["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("not allowed"));

        let spans = server_spans(&h);
        assert_eq!(spans.len(), 1);
        let a = attrs(&spans[0]);
        assert_eq!(a[semconv::MCP_K8S_DECISION], s("denied"));
        assert_eq!(a[semconv::MCP_K8S_DENIED_RULE], s("global:create"));
        assert_eq!(a[semconv::ERROR_TYPE], s("permission_denied"));
        assert!(!a.contains_key(semconv::RPC_STATUS_CODE));
        assert_eq!(a[semconv::MCP_K8S_ACTION], s("write"));
        assert_eq!(a[semconv::K8S_NAMESPACE_NAME], s("prod"));
        assert!(matches!(spans[0].status, Status::Error { .. }));
        let everything = format!("{:?}", h.spans.get_finished_spans().unwrap());
        assert!(!everything.contains("hunter2") && !everything.contains("PASSWORD"));

        let points = duration_points(&h);
        assert_eq!(points.len(), 1);
        assert_eq!(points[0].1[semconv::MCP_K8S_DECISION], "denied");
        assert_eq!(points[0].1[semconv::ERROR_TYPE], "permission_denied");
        assert!(!points[0].1.contains_key(semconv::MCP_K8S_DENIED_RULE));
    }

    #[tokio::test]
    async fn meta_traceparent_is_the_remote_parent() {
        let h = harness();
        let manager = manager(ActionPermissions::default()).await;
        run(
            &h,
            &manager,
            request(json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
                           "params": {"name": "list_namespaces", "arguments": {},
                                      "_meta": {"traceparent": TRACEPARENT}}})),
            &RequestContext::default(),
        )
        .await;
        let spans = server_spans(&h);
        assert_eq!(spans.len(), 1);
        let span = &spans[0];
        assert_eq!(
            span.span_context.trace_id(),
            TraceId::from_hex("4bf92f3577b34da6a3ce929d0e0e4736").unwrap()
        );
        assert_eq!(
            span.parent_span_id,
            SpanId::from_hex("00f067aa0ba902b7").unwrap()
        );
        assert!(span.parent_span_is_remote);
        assert!(span.links.is_empty());
    }

    #[tokio::test]
    async fn http_traceparent_is_the_fallback_and_a_link() {
        let h = harness();
        let manager = manager(ActionPermissions::default()).await;
        let mut headers = HeaderMap::new();
        headers.insert("traceparent", HTTP_TRACEPARENT.parse().unwrap());
        headers.insert("mcp-protocol-version", "2025-11-25".parse().unwrap());
        let ctx = RequestContext::http(Some("10.0.0.7:51000".parse().unwrap()), &headers);

        // Header only: it is the parent.
        run(
            &h,
            &manager,
            request(json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"})),
            &ctx,
        )
        .await;
        // Both: _meta is the parent, the header a link.
        run(
            &h,
            &manager,
            request(json!({"jsonrpc": "2.0", "id": 2, "method": "ping",
                           "params": {"_meta": {"traceparent": TRACEPARENT}}})),
            &ctx,
        )
        .await;

        let spans = server_spans(&h);
        assert_eq!(spans.len(), 2);
        let http_trace = TraceId::from_hex("0af7651916cd43dd8448eb211c80319c").unwrap();
        assert_eq!(spans[0].name, "tools/list");
        assert_eq!(spans[0].span_context.trace_id(), http_trace);
        let a = attrs(&spans[0]);
        assert_eq!(a[semconv::NETWORK_TRANSPORT], s("tcp"));
        assert_eq!(a[semconv::NETWORK_PROTOCOL_NAME], s("http"));
        assert_eq!(a[semconv::CLIENT_ADDRESS], s("10.0.0.7"));
        assert_eq!(a[semconv::CLIENT_PORT], Value::I64(51000));
        assert_eq!(a[semconv::MCP_PROTOCOL_VERSION], s("2025-11-25"));

        assert_eq!(spans[1].name, "ping");
        assert_eq!(
            spans[1].span_context.trace_id(),
            TraceId::from_hex("4bf92f3577b34da6a3ce929d0e0e4736").unwrap()
        );
        assert_eq!(spans[1].links.len(), 1);
        assert_eq!(spans[1].links[0].span_context.trace_id(), http_trace);

        // Non-tool requests: one point each, method (and error.type) only.
        let points = duration_points(&h);
        let methods: Vec<_> = points
            .iter()
            .map(|(_, p)| p[semconv::MCP_METHOD_NAME].clone())
            .collect();
        assert_eq!(methods.len(), 2, "{points:?}");
        assert!(
            methods.contains(&"tools/list".to_string()) && methods.contains(&"ping".to_string())
        );
        assert!(points.iter().all(|(_, p)| p.len() == 1));
    }

    #[tokio::test]
    async fn initialize_and_unknown_requests() {
        let h = harness();
        let manager = manager(ActionPermissions::default()).await;
        run(
            &h,
            &manager,
            request(json!({"jsonrpc": "2.0", "id": 0, "method": "initialize",
                           "params": {"protocolVersion": "2025-06-18",
                                      "clientInfo": {"name": "claude-code", "version": "2.1.0"}}})),
            &RequestContext::default(),
        )
        .await;
        run(
            &h,
            &manager,
            request(json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
                           "params": {"name": "no-such-tool-1234", "arguments": {}}})),
            &RequestContext::default(),
        )
        .await;
        run(
            &h,
            &manager,
            request(json!({"jsonrpc": "2.0", "id": 2, "method": "bogus/method"})),
            &RequestContext::default(),
        )
        .await;

        let spans = server_spans(&h);
        assert_eq!(spans.len(), 3);
        let init = attrs(&spans[0]);
        assert_eq!(spans[0].name, "initialize");
        assert_eq!(init[semconv::MCP_K8S_CLIENT_NAME], s("claude-code"));
        assert_eq!(init[semconv::MCP_K8S_CLIENT_VERSION], s("2.1.0"));
        assert_eq!(init[semconv::MCP_PROTOCOL_VERSION], s(PROTOCOL_VERSION));

        // An unknown tool never names the span or the metric.
        assert_eq!(spans[1].name, "tools/call");
        let unknown = attrs(&spans[1]);
        assert_eq!(unknown[semconv::ERROR_TYPE], s("unknown_tool"));
        assert_eq!(unknown[semconv::GEN_AI_TOOL_NAME], s("no-such-tool-1234"));
        assert_eq!(spans[2].name, "_OTHER");
        assert_eq!(attrs(&spans[2])[semconv::RPC_STATUS_CODE], s("-32601"));
        assert_eq!(
            spans[2].status,
            Status::Unset,
            "a caller mistake, not an error"
        );

        let points = duration_points(&h);
        assert_eq!(points.len(), 3, "{points:?}");
        let rendered = format!("{points:?}");
        assert!(!rendered.contains("no-such-tool-1234") && !rendered.contains("bogus"));
        assert!(rendered.contains("unknown_tool") && rendered.contains("_OTHER"));
    }

    #[tokio::test]
    async fn unknown_tools_are_invalid_params() {
        let h = harness();
        let manager = manager(ActionPermissions::default()).await;
        let response = run(
            &h,
            &manager,
            request(json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
                           "params": {"name": "no_such_tool", "arguments": {}}})),
            &RequestContext::default(),
        )
        .await;
        assert_eq!(response.error.as_ref().map(|e| e.code), Some(-32602));
    }
}
