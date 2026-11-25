//! Integration tests using testcontainers-k3s.
//!
//! These tests spin up an ephemeral k3s cluster in Docker and exercise the
//! `mcp_k8s` tool handlers against it.  They are gated behind `#[ignore]`
//! so that `cargo test` skips them by default.  Run with:
//!
//! ```sh
//! cargo test --test k3s_integration -- --ignored
//! ```
//!
//! The k3s image defaults to [`DEFAULT_K3S_TAG`] (Kubernetes 1.35, the version
//! production runs). Set `MCP_K8S_K3S_TAG` to test another release, e.g.
//! `MCP_K8S_K3S_TAG=v1.33.6-k3s1` for the oldest supported minor version.
//!
//! Requirements:
//!   - Docker daemon running
//!   - Sufficient permissions for privileged containers

use mcp_k8s::mcp::{handle_tool, tool_definitions};
use mcp_k8s::permissions::ActionPermissions;
use mcp_k8s::K8sClient;
use testcontainers::runners::AsyncRunner;
use testcontainers::{ContainerAsync, ImageExt};
use testcontainers_modules::k3s::{K3s, KUBE_SECURE_PORT};

/// k3s image tag used unless `MCP_K8S_K3S_TAG` is set. The library compiles
/// against k8s-openapi `v1_33`, so tags older than v1.33 are unsupported.
const DEFAULT_K3S_TAG: &str = "v1.35.10-k3s1";

fn k3s_tag() -> String {
    std::env::var("MCP_K8S_K3S_TAG").unwrap_or_else(|_| DEFAULT_K3S_TAG.to_string())
}

/// Spin up a k3s container and return a `kube::Client` connected to it plus
/// the container handle (which must be kept alive for the cluster to persist).
async fn start_k3s() -> (kube::Client, ContainerAsync<K3s>) {
    let conf_dir = tempfile::tempdir().expect("failed to create temp dir");

    let container = K3s::default()
        .with_conf_mount(conf_dir.path())
        .with_tag(k3s_tag())
        .with_privileged(true)
        .with_userns_mode("host")
        .start()
        .await
        .expect("failed to start k3s container");

    // Read the kubeconfig that k3s wrote to the mounted directory.
    let kube_conf = container
        .image()
        .read_kube_config()
        .expect("failed to read kubeconfig from k3s");

    // Determine the host port mapped to the k3s API server.
    let kube_port = container
        .get_host_port_ipv4(KUBE_SECURE_PORT)
        .await
        .expect("failed to get mapped port for k3s API server");

    // Rewrite the server URL so the client connects via the mapped host port.
    let kube_conf = kube_conf.replace(
        "https://127.0.0.1:6443",
        &format!("https://127.0.0.1:{kube_port}"),
    );

    // Initialise the rustls crypto provider (required by kube-client).
    let _ = rustls::crypto::ring::default_provider().install_default();

    let kubeconfig =
        kube::config::Kubeconfig::from_yaml(&kube_conf).expect("failed to parse kubeconfig YAML");
    let config = kube::Config::from_custom_kubeconfig(
        kubeconfig,
        &kube::config::KubeConfigOptions::default(),
    )
    .await
    .expect("failed to build kube Config from kubeconfig");

    let client = kube::Client::try_from(config).expect("failed to create kube Client");
    wait_for_api_ready(&client).await;

    (client, container)
}

/// Wait until the API server serves the `default` namespace and its `default`
/// ServiceAccount. k3s reports ready before that, and on a loaded machine (a CI
/// runner, or several clusters starting at once) tests that call the API
/// straight away occasionally failed (k3s_get_configmap, k3s_get_events).
async fn wait_for_api_ready(client: &kube::Client) {
    use k8s_openapi::api::core::v1::{Namespace, ServiceAccount};
    let namespaces: kube::Api<Namespace> = kube::Api::all(client.clone());
    let service_accounts: kube::Api<ServiceAccount> =
        kube::Api::namespaced(client.clone(), "default");
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(90);
    loop {
        let ns = namespaces.get_opt("default").await;
        let sa = service_accounts.get_opt("default").await;
        match (&ns, &sa) {
            (Ok(Some(_)), Ok(Some(_))) => return,
            _ if tokio::time::Instant::now() >= deadline => {
                panic!("k3s API not ready after 90s: namespace={ns:?} serviceaccount={sa:?}")
            }
            _ => tokio::time::sleep(std::time::Duration::from_secs(1)).await,
        }
    }
}

/// Like [`start_k3s`], wrapped in a `K8sClient` with default permissions and
/// no namespace allowlist.
async fn setup_k3s_client() -> (K8sClient, ContainerAsync<K3s>) {
    let (client, container) = start_k3s().await;
    let k8s_client = K8sClient::new(client, vec![], ActionPermissions::default());

    (k8s_client, container)
}

/// Whether a tool error is the API server's 404 for a resource type it doesn't
/// serve (alpha and most beta API versions are disabled by default). The body
/// is a JSON `NotFound` Status for an unserved version of a served group, and
/// plain "404 page not found" for an unserved group; kube reports both with
/// the status code.
fn is_api_not_served(err: &str) -> bool {
    err.contains("code: 404")
}

/// Call a read-only tool until its output contains `needle`, for up to 90s.
/// k3s reports ready before its packaged components (such as CoreDNS) exist.
async fn wait_for_tool_output(
    client: &K8sClient,
    tool: &str,
    args: &serde_json::Value,
    needle: &str,
) -> String {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(90);
    loop {
        let text = handle_tool(client, tool, args)
            .await
            .unwrap_or_else(|| panic!("{tool} should be a known tool"))
            .unwrap_or_else(|err| panic!("{tool} should succeed: {err}"));
        if text.contains(needle) || tokio::time::Instant::now() >= deadline {
            return text;
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore] // requires Docker
async fn k3s_list_namespaces() {
    let (client, _container) = setup_k3s_client().await;

    let result = handle_tool(&client, "list_namespaces", &serde_json::json!({})).await;
    let text = result
        .expect("list_namespaces should be a known tool")
        .expect("list_namespaces should succeed");

    // k3s always provisions these namespaces.
    assert!(text.contains("default"), "missing 'default' namespace");
    assert!(
        text.contains("kube-system"),
        "missing 'kube-system' namespace"
    );
}

#[tokio::test]
#[ignore]
async fn k3s_list_nodes() {
    let (client, _container) = setup_k3s_client().await;

    let result = handle_tool(&client, "list_nodes", &serde_json::json!({})).await;
    let text = result
        .expect("list_nodes should be a known tool")
        .expect("list_nodes should succeed");

    // k3s runs a single node which should be Ready.
    assert!(text.contains("Ready"), "node should be in Ready state");
}

#[tokio::test]
#[ignore]
async fn k3s_list_deployments_kube_system() {
    let (client, _container) = setup_k3s_client().await;

    let args = serde_json::json!({"namespace": "kube-system"});
    let text = wait_for_tool_output(&client, "list_deployments", &args, "coredns").await;

    // k3s kube-system always runs coredns.
    assert!(text.contains("coredns"), "coredns deployment not found");
}

#[tokio::test]
#[ignore]
async fn k3s_create_and_list_configmap() {
    let (client, _container) = setup_k3s_client().await;

    // Create a configmap via the tool handler.
    let create_args = serde_json::json!({
        "namespace": "default",
        "name": "integration-test-cm",
        "data": {"key1": "value1", "key2": "value2"}
    });
    let result = handle_tool(&client, "create_configmap", &create_args).await;
    result
        .expect("create_configmap should be a known tool")
        .expect("create_configmap should succeed");

    // List configmaps and verify ours is present.
    let list_args = serde_json::json!({"namespace": "default"});
    let result = handle_tool(&client, "list_configmaps", &list_args).await;
    let text = result
        .expect("list_configmaps should be a known tool")
        .expect("list_configmaps should succeed");

    assert!(
        text.contains("integration-test-cm"),
        "created configmap not found in listing"
    );
}

#[tokio::test]
#[ignore]
async fn k3s_get_configmap() {
    let (client, _container) = setup_k3s_client().await;

    // Create a configmap first.
    let create_args = serde_json::json!({
        "namespace": "default",
        "name": "get-test-cm",
        "data": {"hello": "world"}
    });
    handle_tool(&client, "create_configmap", &create_args)
        .await
        .expect("known tool")
        .expect("create should succeed");

    // Get the configmap by name and verify data.
    let get_args = serde_json::json!({"namespace": "default", "name": "get-test-cm"});
    let result = handle_tool(&client, "get_configmap", &get_args).await;
    let text = result
        .expect("get_configmap should be a known tool")
        .expect("get_configmap should succeed");

    assert!(text.contains("hello"), "configmap data key missing");
    assert!(text.contains("world"), "configmap data value missing");
}

#[tokio::test]
#[ignore]
async fn k3s_create_and_delete_namespace() {
    let (client, _container) = setup_k3s_client().await;

    // Create a namespace.
    let create_args = serde_json::json!({"name": "integration-test-ns"});
    let result = handle_tool(&client, "create_namespace", &create_args).await;
    result
        .expect("create_namespace should be a known tool")
        .expect("create_namespace should succeed");

    // Verify it appears in the namespace list.
    let result = handle_tool(&client, "list_namespaces", &serde_json::json!({})).await;
    let text = result
        .expect("list_namespaces should be a known tool")
        .expect("list_namespaces should succeed");
    assert!(
        text.contains("integration-test-ns"),
        "created namespace not found"
    );

    // Delete the namespace.
    let delete_args = serde_json::json!({"name": "integration-test-ns"});
    let result = handle_tool(&client, "delete_namespace", &delete_args).await;
    result
        .expect("delete_namespace should be a known tool")
        .expect("delete_namespace should succeed");
}

#[tokio::test]
#[ignore]
async fn k3s_create_service() {
    let (client, _container) = setup_k3s_client().await;

    let args = serde_json::json!({
        "namespace": "default",
        "name": "integration-test-svc",
        "port": 8080,
        "target_port": 80
    });
    let result = handle_tool(&client, "create_service", &args).await;
    let text = result
        .expect("create_service should be a known tool")
        .expect("create_service should succeed");

    assert!(
        text.contains("integration-test-svc"),
        "service name missing from response"
    );
    assert!(text.contains("8080"), "service port missing from response");
}

#[tokio::test]
#[ignore]
async fn k3s_permission_controls() {
    let (kube_client, _container) = start_k3s().await;

    // Build permissions with delete disabled.
    let perms = ActionPermissions::new(
        false, // disable_create = false  (create allowed)
        false, // disable_update = false  (update allowed)
        true,  // disable_delete = true   (delete blocked)
        vec![],
        true,
        true, // apply_manifest_enabled
    );
    let k8s_client = K8sClient::new(kube_client, vec![], perms);

    // Create should succeed.
    let create_args = serde_json::json!({
        "namespace": "default",
        "name": "perm-test-cm",
        "data": {"k": "v"}
    });
    let result = handle_tool(&k8s_client, "create_configmap", &create_args).await;
    result
        .expect("create_configmap should be a known tool")
        .expect("create should succeed when create is allowed");

    // Delete should be blocked by permissions before reaching the K8s API.
    let delete_args = serde_json::json!({
        "namespace": "default",
        "name": "perm-test-cm"
    });
    let result = handle_tool(&k8s_client, "delete_configmap", &delete_args).await;
    let err = result
        .expect("delete_configmap should be a known tool")
        .expect_err("delete should be rejected when delete is disabled");

    assert!(
        err.contains("not allowed"),
        "error message should indicate the action is not allowed, got: {err}"
    );

    // Reads should still work regardless.
    let list_args = serde_json::json!({"namespace": "default"});
    let result = handle_tool(&k8s_client, "list_configmaps", &list_args).await;
    result
        .expect("list_configmaps should be a known tool")
        .expect("reads should always succeed");
}

#[tokio::test]
#[ignore]
async fn k3s_list_pods_kube_system() {
    let (client, _container) = setup_k3s_client().await;

    let args = serde_json::json!({"namespace": "kube-system"});
    let text = wait_for_tool_output(&client, "list_pods", &args, "coredns").await;

    // k3s kube-system always runs coredns pods.
    assert!(
        text.contains("coredns"),
        "coredns pod not found in kube-system"
    );
}

#[tokio::test]
#[ignore]
async fn k3s_get_events() {
    let (client, _container) = setup_k3s_client().await;

    // kube-system should have events from the cluster bootstrapping.
    let args = serde_json::json!({"namespace": "kube-system"});
    let result = handle_tool(&client, "get_events", &args).await;
    let text = result
        .expect("get_events should be a known tool")
        .expect("get_events should succeed");

    // We just verify the call succeeds and returns something non-empty.
    // The exact events vary, but there should be at least startup events.
    assert!(
        !text.is_empty(),
        "events response should not be empty after cluster bootstrap"
    );
}

#[tokio::test]
#[ignore]
async fn k3s_server_meets_minimum_kubernetes_version() {
    let (client, _container) = start_k3s().await;

    let info = client
        .apiserver_version()
        .await
        .expect("apiserver version should be readable");
    let minor: u32 = info
        .minor
        .trim_end_matches('+')
        .parse()
        .expect("minor version should be numeric");

    // k8s-openapi `v1_33` is the oldest version k8s-openapi 0.29 offers, so
    // the library supports Kubernetes 1.33 and newer.
    assert_eq!(info.major, "1");
    assert!(
        minor >= 33,
        "k3s image {} runs Kubernetes {}.{}, below the supported minimum 1.33",
        k3s_tag(),
        info.major,
        info.minor
    );
}

#[tokio::test]
#[ignore]
async fn k3s_networking_v1_servicecidrs_and_ipaddresses() {
    // IPAddress and ServiceCIDR are GA (networking.k8s.io/v1) since Kubernetes
    // 1.33 and served by default, unlike the v1beta1 versions used before 0.5.0.
    let (client, _container) = setup_k3s_client().await;

    let text = handle_tool(&client, "list_servicecidrs", &serde_json::json!({}))
        .await
        .expect("list_servicecidrs should be a known tool")
        .expect("list_servicecidrs should succeed");
    let cidrs: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
    // The API server creates the default ServiceCIDR, named "kubernetes".
    assert!(
        cidrs
            .as_array()
            .expect("list_servicecidrs returns an array")
            .iter()
            .any(|c| c["name"] == "kubernetes"),
        "default 'kubernetes' ServiceCIDR not found: {text}"
    );

    let text = handle_tool(
        &client,
        "get_servicecidr",
        &serde_json::json!({"name": "kubernetes"}),
    )
    .await
    .expect("get_servicecidr should be a known tool")
    .expect("get_servicecidr should succeed");
    let detail: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
    assert!(
        !detail["cidrs"]
            .as_array()
            .expect("cidrs is an array")
            .is_empty(),
        "default ServiceCIDR should list its CIDRs: {text}"
    );

    // Every Service ClusterIP is backed by an IPAddress, and the "kubernetes"
    // Service in "default" always exists.
    let text = handle_tool(&client, "list_ipaddresses", &serde_json::json!({}))
        .await
        .expect("list_ipaddresses should be a known tool")
        .expect("list_ipaddresses should succeed");
    let ips: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
    assert!(
        ips.as_array()
            .expect("list_ipaddresses returns an array")
            .iter()
            .any(|ip| ip["parent_ref"]["resource"] == "services"
                && ip["parent_ref"]["name"] == "kubernetes"),
        "no IPAddress for the 'kubernetes' Service: {text}"
    );
}

#[tokio::test]
#[ignore]
async fn k3s_list_tools_decode_or_report_unserved_api() {
    // Every `list_*` tool that needs no more than a namespace must decode what
    // a real API server returns. Tools for API versions the server doesn't
    // serve (alpha, and beta APIs that are off by default) must fail with the
    // server's 404 rather than a decode error.
    let (client, _container) = setup_k3s_client().await;

    // The binary dispatches the cluster-management tools to its
    // ClusterManager; `mcp::handle_tool` doesn't handle them.
    let cluster_mgmt: Vec<String> = mcp_k8s::resources::cluster_mgmt::tool_definitions()
        .iter()
        .filter_map(|d| d["name"].as_str().map(String::from))
        .collect();

    let mut checked = Vec::new();
    let mut unserved = Vec::new();
    for def in tool_definitions(client.permissions()) {
        let name = def["name"].as_str().expect("tool name").to_string();
        if !name.starts_with("list_") || cluster_mgmt.contains(&name) {
            continue;
        }
        let required: Vec<&str> = def["inputSchema"]["required"]
            .as_array()
            .map(|r| r.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default();
        let args = match required.as_slice() {
            [] => serde_json::json!({}),
            ["namespace"] => serde_json::json!({"namespace": "kube-system"}),
            // Needs resource-specific arguments (e.g. list_custom_resources).
            _ => continue,
        };

        match handle_tool(&client, &name, &args)
            .await
            .unwrap_or_else(|| panic!("{name} should be a known tool"))
        {
            Ok(_) => {}
            Err(err) if is_api_not_served(&err) => unserved.push(name.clone()),
            Err(err) => panic!("{name} failed: {err}"),
        }
        checked.push(name);
    }

    eprintln!(
        "k3s {}: {} list tools checked; API not served for {:?}",
        k3s_tag(),
        checked.len(),
        unserved
    );
    assert!(checked.len() > 50, "expected to check most list tools");

    // GA APIs are always served, including networking.k8s.io/v1 IPAddress and
    // ServiceCIDR (GA in 1.33).
    for ga in [
        "list_namespaces",
        "list_nodes",
        "list_deployments",
        "list_pods",
        "list_csidrivers",
        "list_flowschemas",
        "list_validatingadmissionpolicies",
        "list_ipaddresses",
        "list_servicecidrs",
    ] {
        assert!(checked.iter().any(|c| c == ga), "{ga} was not checked");
        assert!(
            !unserved.iter().any(|u| u == ga),
            "{ga} uses an API version the server doesn't serve"
        );
    }
}
