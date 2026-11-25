//! Request-level protections for the HTTP server: the DNS-rebinding `Host`
//! check, cross-site request blocking, and the optional CORS allowlist.
//!
//! * **DNS rebinding.** A web page whose domain resolves to the server's
//!   address can talk to it from a victim's browser, and its `Origin` and
//!   `Host` agree with each other, so an origin check alone doesn't catch it.
//!   The MCP Streamable HTTP spec requires servers to guard against it. When
//!   an allowlist of host names is configured (and by default when the server
//!   listens on loopback: `localhost`, `127.0.0.1`, `[::1]`), requests for any
//!   other `Host` are refused. `/healthz` is exempt so kubelet probes, which
//!   use the pod IP, keep working.
//! * **Cross-site requests.** Without CORS a page on another origin can't read
//!   responses, but a "simple" cross-site POST still reaches the handler and
//!   runs the tool call. Browsers label those with `Sec-Fetch-Site` and/or
//!   `Origin`; they're refused unless the origin is in the CORS allowlist.
//!   Non-browser clients (MCP clients, curl) send neither and are unaffected.
//! * **CORS.** No CORS headers by default. `--cors-allowed-origins` allows
//!   exact origins, for browser-based MCP clients.

use axum::extract::{Request, State};
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode, Uri};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use tower_http::cors::{AllowOrigin, CorsLayer};

/// Host names a loopback listener accepts when no `--allowed-hosts` is given.
pub const LOOPBACK_HOSTS: &[&str] = &["localhost", "127.0.0.1", "[::1]"];

/// What the request-level checks allow.
#[derive(Clone, Debug, Default)]
pub struct OriginPolicy {
    /// Allowed `Host` names (without port, lowercase). `None` means any host.
    pub allowed_hosts: Option<Vec<String>>,
    /// Origins allowed to make cross-site requests and get CORS headers
    /// (`scheme://host[:port]`, lowercase).
    pub allowed_origins: Vec<String>,
}

impl OriginPolicy {
    /// Build the policy from the CLI values. An empty `allowed_hosts` list
    /// means the default: the loopback names when `loopback` is true, any
    /// host otherwise.
    pub fn new(allowed_hosts: &[String], allowed_origins: &[String], loopback: bool) -> Self {
        let hosts: Vec<String> = allowed_hosts
            .iter()
            .map(|h| h.trim().to_ascii_lowercase())
            .filter(|h| !h.is_empty())
            .collect();
        let allowed_hosts = if !hosts.is_empty() {
            Some(hosts)
        } else if loopback {
            Some(LOOPBACK_HOSTS.iter().map(|h| h.to_string()).collect())
        } else {
            None
        };
        let allowed_origins = allowed_origins
            .iter()
            .map(|o| o.trim().trim_end_matches('/').to_ascii_lowercase())
            .filter(|o| !o.is_empty())
            .collect();
        Self {
            allowed_hosts,
            allowed_origins,
        }
    }

    /// The CORS layer for the configured origins, or `None` (no CORS headers).
    pub fn cors_layer(&self) -> Option<CorsLayer> {
        if self.allowed_origins.is_empty() {
            return None;
        }
        let origins: Vec<HeaderValue> = self
            .allowed_origins
            .iter()
            .filter_map(|o| HeaderValue::from_str(o).ok())
            .collect();
        Some(
            CorsLayer::new()
                .allow_origin(AllowOrigin::list(origins))
                .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
                .allow_headers([
                    header::AUTHORIZATION,
                    header::CONTENT_TYPE,
                    header::ACCEPT,
                    header::HeaderName::from_static("mcp-protocol-version"),
                    header::HeaderName::from_static("mcp-session-id"),
                    header::HeaderName::from_static("traceparent"),
                    header::HeaderName::from_static("tracestate"),
                ]),
        )
    }

    fn host_allowed(&self, req: &Request) -> bool {
        let Some(allowed) = &self.allowed_hosts else {
            return true;
        };
        // HTTP/1.1 sends `Host`; HTTP/2 sends `:authority`, which hyper puts
        // in the URI.
        let host = req
            .headers()
            .get(header::HOST)
            .and_then(|v| v.to_str().ok())
            .or_else(|| req.uri().authority().map(|a| a.as_str()));
        host.is_some_and(|h| {
            let name = host_name(h);
            allowed.iter().any(|a| a == &name)
        })
    }

    fn origin_allowed(&self, origin: &str) -> bool {
        let origin = origin.trim_end_matches('/').to_ascii_lowercase();
        self.allowed_origins.iter().any(|o| o == &origin)
    }
}

/// Axum middleware: refuse requests for a `Host` outside the allowlist
/// (DNS rebinding) and browser-initiated cross-site requests that could
/// change state, with `403 Forbidden`.
pub async fn guard(State(policy): State<OriginPolicy>, req: Request, next: Next) -> Response {
    let path = req.uri().path();
    if path != "/healthz" && !policy.host_allowed(&req) {
        tracing::warn!(
            path = %path,
            host = ?req.headers().get(header::HOST),
            "blocked request for a host outside --allowed-hosts"
        );
        return (StatusCode::FORBIDDEN, "host not allowed\n").into_response();
    }

    if needs_origin_check(req.method()) && is_cross_site(&policy, req.headers()) {
        tracing::warn!(
            method = %req.method(),
            path = %path,
            origin = ?req.headers().get(header::ORIGIN),
            sec_fetch_site = ?req.headers().get("sec-fetch-site"),
            "blocked cross-site request"
        );
        return (StatusCode::FORBIDDEN, "cross-site request blocked\n").into_response();
    }
    next.run(req).await
}

/// Every method except the safe ones can change state. CORS preflights
/// (`OPTIONS`) are answered by the CORS layer.
fn needs_origin_check(method: &Method) -> bool {
    !matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS)
}

/// True when a browser reports that another site initiated the request and
/// that site's origin isn't allowlisted.
fn is_cross_site(policy: &OriginPolicy, headers: &HeaderMap) -> bool {
    let origin = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok());
    if let Some(origin) = origin {
        if policy.origin_allowed(origin) {
            return false;
        }
    }
    // Fetch metadata is set by the browser and can't be forged by page script.
    // `none` means user-initiated (typed URL, bookmark).
    if let Some(site) = headers.get("sec-fetch-site") {
        return !matches!(site.to_str(), Ok("same-origin" | "none"));
    }
    match headers.get(header::ORIGIN).map(|v| v.to_str()) {
        // No Origin: not a browser-initiated cross-site request.
        None => false,
        Some(Ok(origin)) => !origin_matches_host(origin, headers),
        Some(Err(_)) => true,
    }
}

/// Compare the `Origin` authority with `Host`. An opaque origin (`null`)
/// never matches.
fn origin_matches_host(origin: &str, headers: &HeaderMap) -> bool {
    let Some(origin_authority) = origin
        .parse::<Uri>()
        .ok()
        .and_then(|u| u.authority().map(|a| normalize_authority(a.as_str())))
    else {
        return false;
    };
    headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|host| normalize_authority(host) == origin_authority)
}

/// Lowercase and drop default ports so `Host: example.com` matches
/// `Origin: https://example.com:443`.
fn normalize_authority(authority: &str) -> String {
    let a = authority.trim().to_ascii_lowercase();
    a.strip_suffix(":443")
        .or_else(|| a.strip_suffix(":80"))
        .map(str::to_string)
        .unwrap_or(a)
}

/// The host name of a `Host` value, lowercase, without the port. IPv6
/// literals keep their brackets (`[::1]`).
fn host_name(host: &str) -> String {
    let host = host.trim().to_ascii_lowercase();
    if host.starts_with('[') {
        return match host.find(']') {
            Some(end) => host[..=end].to_string(),
            None => host,
        };
    }
    match host.rsplit_once(':') {
        Some((name, port)) if port.chars().all(|c| c.is_ascii_digit()) => name.to_string(),
        _ => host,
    }
}

/// Whether a listen address is loopback-only (`127.0.0.0/8`, `::1`,
/// `localhost`).
pub fn is_loopback_listen(listen: &str) -> bool {
    match listen.parse::<std::net::SocketAddr>() {
        Ok(addr) => addr.ip().is_loopback(),
        Err(_) => listen
            .rsplit_once(':')
            .map(|(h, _)| h)
            .unwrap_or(listen)
            .eq_ignore_ascii_case("localhost"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::routing::{get, post};
    use tower::ServiceExt;

    fn app(policy: OriginPolicy) -> axum::Router {
        let router = axum::Router::new()
            .route("/mcp", post(|| async { "ok" }))
            .route("/healthz", get(|| async { "ok" }))
            .route("/openapi.json", get(|| async { "{}" }))
            .layer(axum::middleware::from_fn_with_state(policy.clone(), guard));
        match policy.cors_layer() {
            Some(cors) => router.layer(cors),
            None => router,
        }
    }

    async fn send(
        policy: OriginPolicy,
        req: axum::http::Request<Body>,
    ) -> axum::http::Response<axum::body::Body> {
        app(policy).oneshot(req).await.unwrap()
    }

    fn post_mcp(host: &str) -> axum::http::request::Builder {
        axum::http::Request::post("/mcp").header(header::HOST, host)
    }

    #[test]
    fn host_name_strips_ports_and_keeps_ipv6_brackets() {
        assert_eq!(host_name("LocalHost:8080"), "localhost");
        assert_eq!(host_name("127.0.0.1"), "127.0.0.1");
        assert_eq!(host_name("[::1]:8080"), "[::1]");
        assert_eq!(host_name("mcp.example.com"), "mcp.example.com");
    }

    #[test]
    fn loopback_listen_addresses() {
        assert!(is_loopback_listen("127.0.0.1:8080"));
        assert!(is_loopback_listen("[::1]:8080"));
        assert!(is_loopback_listen("localhost:8080"));
        assert!(!is_loopback_listen("0.0.0.0:8080"));
        assert!(!is_loopback_listen("[::]:8080"));
        assert!(!is_loopback_listen("10.0.0.5:8080"));
    }

    #[test]
    fn loopback_listener_defaults_to_loopback_hosts() {
        let p = OriginPolicy::new(&[], &[], true);
        assert_eq!(
            p.allowed_hosts.as_deref(),
            Some(&["localhost".to_string(), "127.0.0.1".into(), "[::1]".into()][..])
        );
        assert!(OriginPolicy::new(&[], &[], false).allowed_hosts.is_none());
        let p = OriginPolicy::new(&["MCP.example.com".into()], &[], true);
        assert_eq!(
            p.allowed_hosts.as_deref(),
            Some(&["mcp.example.com".to_string()][..])
        );
    }

    #[tokio::test]
    async fn dns_rebinding_host_is_refused_on_loopback() {
        // evil.example resolves to 127.0.0.1; its Origin and Host agree.
        let p = OriginPolicy::new(&[], &[], true);
        let req = post_mcp("evil.example:8080")
            .header(header::ORIGIN, "http://evil.example:8080")
            .body(Body::empty())
            .unwrap();
        assert_eq!(send(p.clone(), req).await.status(), StatusCode::FORBIDDEN);
        let req = post_mcp("127.0.0.1:8080").body(Body::empty()).unwrap();
        assert_eq!(send(p, req).await.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn healthz_is_exempt_from_the_host_allowlist() {
        let p = OriginPolicy::new(&["mcp.example.com".into()], &[], false);
        let req = axum::http::Request::get("/healthz")
            .header(header::HOST, "10.1.2.3:8080")
            .body(Body::empty())
            .unwrap();
        assert_eq!(send(p.clone(), req).await.status(), StatusCode::OK);
        let req = axum::http::Request::get("/openapi.json")
            .header(header::HOST, "10.1.2.3:8080")
            .body(Body::empty())
            .unwrap();
        assert_eq!(send(p, req).await.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn non_browser_clients_pass() {
        // No Origin, no Sec-Fetch-Site: an MCP client or curl.
        let p = OriginPolicy::new(&[], &[], false);
        let req = post_mcp("mcp-k8s.mcp.svc:8080")
            .body(Body::empty())
            .unwrap();
        assert_eq!(send(p, req).await.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn cross_site_browser_posts_are_refused() {
        let p = OriginPolicy::new(&[], &[], false);
        let req = post_mcp("mcp.internal:8080")
            .header(header::ORIGIN, "https://attacker.example")
            .body(Body::empty())
            .unwrap();
        assert_eq!(send(p.clone(), req).await.status(), StatusCode::FORBIDDEN);
        let req = post_mcp("mcp.internal:8080")
            .header("sec-fetch-site", "cross-site")
            .body(Body::empty())
            .unwrap();
        assert_eq!(send(p.clone(), req).await.status(), StatusCode::FORBIDDEN);
        let req = post_mcp("mcp.internal:8080")
            .header(header::ORIGIN, "null")
            .body(Body::empty())
            .unwrap();
        assert_eq!(send(p, req).await.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn same_origin_browser_posts_pass() {
        // Swagger UI's "Try it out" on the server's own origin.
        let p = OriginPolicy::new(&[], &[], false);
        let req = post_mcp("mcp.internal:8080")
            .header(header::ORIGIN, "http://mcp.internal:8080")
            .header("sec-fetch-site", "same-origin")
            .body(Body::empty())
            .unwrap();
        assert_eq!(send(p, req).await.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn allowlisted_origins_get_cors_and_pass() {
        let p = OriginPolicy::new(&[], &["https://client.example/".into()], false);
        let req = post_mcp("mcp.internal:8080")
            .header(header::ORIGIN, "https://client.example")
            .header("sec-fetch-site", "cross-site")
            .body(Body::empty())
            .unwrap();
        let resp = send(p.clone(), req).await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .unwrap(),
            "https://client.example"
        );
        // Preflight.
        let req = axum::http::Request::builder()
            .method(Method::OPTIONS)
            .uri("/mcp")
            .header(header::HOST, "mcp.internal:8080")
            .header(header::ORIGIN, "https://client.example")
            .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
            .header(
                header::ACCESS_CONTROL_REQUEST_HEADERS,
                "authorization,content-type",
            )
            .body(Body::empty())
            .unwrap();
        let resp = send(p, req).await;
        assert!(resp.status().is_success(), "{}", resp.status());
        assert!(resp
            .headers()
            .contains_key(header::ACCESS_CONTROL_ALLOW_METHODS));
    }

    #[tokio::test]
    async fn no_cors_headers_by_default() {
        let p = OriginPolicy::new(&[], &[], false);
        assert!(p.cors_layer().is_none());
        let req = axum::http::Request::get("/openapi.json")
            .header(header::HOST, "mcp.internal:8080")
            .header(header::ORIGIN, "https://attacker.example")
            .body(Body::empty())
            .unwrap();
        let resp = send(p, req).await;
        assert!(!resp
            .headers()
            .contains_key(header::ACCESS_CONTROL_ALLOW_ORIGIN));
    }
}
