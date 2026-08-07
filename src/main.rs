use std::io::BufRead;
use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::{header, StatusCode};
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
use tracing::Instrument;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;
use uuid::Uuid;

use mcp_k8s::mcp::{
    error_response, method_not_found, success_response, JsonRpcError, JsonRpcRequest,
    JsonRpcResponse,
};
use mcp_k8s::permissions::ActionPermissions;
use mcp_k8s::{ClusterManager, K8sClient, ResponseCache};

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

    /// Disable the apply_manifest tool. When set, apply_manifest calls are
    /// rejected regardless of other permission settings.
    #[arg(long, env = "DISABLE_APPLY_MANIFEST")]
    disable_apply_manifest: bool,

    /// Log output format: "text" (default) or "json" for structured JSON logging.
    #[arg(long, default_value = "text", env = "LOG_FORMAT")]
    log_format: String,

    /// Response cache TTL in seconds for list operations.
    /// Set to 0 (default) to disable caching.
    #[arg(long, env = "CACHE_TTL", default_value = "0")]
    cache_ttl: u64,

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

    let env_filter =
        tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into());

    if cli.log_format == "json" {
        tracing_subscriber::fmt()
            .with_env_filter(env_filter)
            .json()
            .init();
    } else {
        tracing_subscriber::fmt().with_env_filter(env_filter).init();
    }

    let prometheus_handle = metrics_exporter_prometheus::PrometheusBuilder::new()
        .install_recorder()
        .expect("Failed to install Prometheus recorder");

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
        !cli.disable_apply_manifest,
    );

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
        run_stdio(cluster_manager).await;
    }
}

// ---------------------------------------------------------------------------
// Stdio mode
// ---------------------------------------------------------------------------

async fn run_stdio(manager: ClusterManager) {
    let stdin = std::io::stdin();
    let reader = stdin.lock();

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

        let response = dispatch(&manager, request).await;
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
        .merge(SwaggerUi::new("/swagger-ui").url("/openapi.json", ApiDoc::openapi()))
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

    axum::serve(listener, app).await.expect("Server error");
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

    axum_server::bind_rustls(addr, config)
        .serve(app.into_make_service())
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
    Json(request): Json<JsonRpcRequest>,
) -> impl IntoResponse {
    let response = dispatch(&manager, request).await;
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
    Json(request): Json<JsonRpcRequest>,
) -> Sse<impl futures::Stream<Item = Result<Event, std::convert::Infallible>>> {
    let response = dispatch(&manager, request).await;
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

async fn dispatch(manager: &ClusterManager, request: JsonRpcRequest) -> JsonRpcResponse {
    let trace_id = Uuid::new_v4().to_string();
    let span = tracing::info_span!(
        "mcp_request",
        trace_id = %trace_id,
        method = %request.method,
    );

    async {
        counter!("mcp_requests_total", "method" => request.method.clone()).increment(1);
        tracing::info!("processing request");

        match request.method.as_str() {
            "initialize" => handle_initialize(&request),
            "notifications/initialized" => success_response(&request, serde_json::json!({})),
            "ping" => success_response(&request, serde_json::json!({})),
            "tools/list" => handle_tools_list(manager, &request).await,
            "tools/call" => handle_tool_call(manager, &request).await,
            "resources/list" => handle_resources_list(&request),
            "resources/read" => handle_resources_read(manager, &request).await,
            "prompts/list" => handle_prompts_list(&request),
            "prompts/get" => handle_prompts_get(&request),
            _ => method_not_found(&request),
        }
    }
    .instrument(span)
    .await
}

fn handle_initialize(request: &JsonRpcRequest) -> JsonRpcResponse {
    success_response(
        request,
        serde_json::json!({
            "protocolVersion": "2025-11-25",
            "capabilities": { "tools": {}, "resources": {}, "prompts": {} },
            "serverInfo": { "name": "mcp-k8s", "version": env!("CARGO_PKG_VERSION") }
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

async fn handle_tool_call(manager: &ClusterManager, request: &JsonRpcRequest) -> JsonRpcResponse {
    let params = &request.params;
    let tool_name = params["name"].as_str().unwrap_or("");
    let args = &params["arguments"];

    // Handle cluster management tools first (they operate on the manager, not a K8s client)
    let result = match tool_name {
        "list_clusters" | "switch_cluster" | "get_active_cluster" => {
            mcp_k8s::resources::cluster_mgmt::handle_tool(manager, tool_name, args).await
        }
        _ => {
            // All other tools need the active K8s client
            let client = match manager.active_client().await {
                Some(c) => c,
                None => {
                    return error_response(request, -32000, "no active cluster configured");
                }
            };
            mcp_k8s::mcp::handle_tool(&client, tool_name, args).await
        }
    };

    match result {
        Some(Ok(text)) => success_response(
            request,
            serde_json::json!({
                "content": [{ "type": "text", "text": text }]
            }),
        ),
        Some(Err(e)) => error_response(request, -32000, &e),
        None => error_response(request, -32000, &format!("Unknown tool: {tool_name}")),
    }
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

    mcp_k8s::resources::generic::handle_tool(client, "get_resource_yaml", &args)
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
            return error_response(request, -32000, &format!("unknown prompt: {prompt_name}"));
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
