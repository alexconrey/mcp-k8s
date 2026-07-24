use std::io::BufRead;
use std::sync::Arc;

use axum::extract::State;
use axum::http::header;
use axum::response::IntoResponse;
use axum::Json;
use clap::Parser;
use tower_http::cors::CorsLayer;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use mcp_k8s::mcp::{
    error_response, method_not_found, success_response, JsonRpcError, JsonRpcRequest,
    JsonRpcResponse,
};
use mcp_k8s::permissions::ActionPermissions;
use mcp_k8s::K8sClient;

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
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let cli = Cli::parse();

    let kube_client = kube::Client::try_default()
        .await
        .expect("Failed to create Kubernetes client");

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
    );

    let client = K8sClient::new(kube_client, namespaces, permissions);

    if cli.http {
        run_http(client, &cli.listen).await;
    } else {
        run_stdio(client).await;
    }
}

// ---------------------------------------------------------------------------
// Stdio mode — for Claude Code MCP server config
// ---------------------------------------------------------------------------

async fn run_stdio(client: K8sClient) {
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

        let response = dispatch(&client, request).await;
        println!("{}", serde_json::to_string(&response).unwrap());
    }
}

// ---------------------------------------------------------------------------
// HTTP mode — for in-cluster deployment as a K8s pod
// ---------------------------------------------------------------------------

async fn run_http(client: K8sClient, listen: &str) {
    let state = Arc::new(client);

    let app = axum::Router::new()
        .route("/mcp", axum::routing::post(handle_mcp_http))
        .route("/healthz", axum::routing::get(healthz))
        .merge(SwaggerUi::new("/swagger-ui").url("/openapi.json", ApiDoc::openapi()))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(listen)
        .await
        .expect("Failed to bind listener");

    tracing::info!("mcp-k8s HTTP server listening on {listen}");

    axum::serve(listener, app).await.expect("Server error");
}

#[utoipa::path(
    post,
    path = "/mcp",
    tag = "mcp",
    summary = "MCP JSON-RPC 2.0 endpoint",
    description = "Handles MCP protocol methods: initialize, notifications/initialized, \
                   tools/list, and tools/call. Tools provide Kubernetes cluster operations \
                   (list deployments, get pod logs, create ingress, etc.).",
    request_body(content = JsonRpcRequest, content_type = "application/json"),
    responses(
        (status = 200, description = "JSON-RPC response", body = JsonRpcResponse),
    )
)]
async fn handle_mcp_http(
    State(client): State<Arc<K8sClient>>,
    Json(request): Json<JsonRpcRequest>,
) -> impl IntoResponse {
    let response = dispatch(&client, request).await;
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

// ---------------------------------------------------------------------------
// Shared dispatch
// ---------------------------------------------------------------------------

async fn dispatch(client: &K8sClient, request: JsonRpcRequest) -> JsonRpcResponse {
    match request.method.as_str() {
        "initialize" => handle_initialize(&request),
        "notifications/initialized" => success_response(&request, serde_json::json!({})),
        "tools/list" => handle_tools_list(client, &request),
        "tools/call" => handle_tool_call(client, &request).await,
        _ => method_not_found(&request),
    }
}

fn handle_initialize(request: &JsonRpcRequest) -> JsonRpcResponse {
    success_response(
        request,
        serde_json::json!({
            "protocolVersion": "2025-11-25",
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "mcp-k8s", "version": env!("CARGO_PKG_VERSION") }
        }),
    )
}

fn handle_tools_list(client: &K8sClient, request: &JsonRpcRequest) -> JsonRpcResponse {
    let tools = mcp_k8s::mcp::tool_definitions(client.permissions());
    success_response(request, serde_json::json!({ "tools": tools }))
}

async fn handle_tool_call(client: &K8sClient, request: &JsonRpcRequest) -> JsonRpcResponse {
    let params = &request.params;
    let tool_name = params["name"].as_str().unwrap_or("");
    let args = &params["arguments"];

    let result = mcp_k8s::mcp::handle_tool(client, tool_name, args).await;

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
