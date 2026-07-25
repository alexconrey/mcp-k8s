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
//! Requirements:
//!   - Docker daemon running
//!   - Sufficient permissions for privileged containers

use mcp_k8s::mcp::handle_tool;
use mcp_k8s::permissions::ActionPermissions;
use mcp_k8s::K8sClient;
use testcontainers::runners::AsyncRunner;
use testcontainers::{ContainerAsync, ImageExt};
use testcontainers_modules::k3s::{K3s, KUBE_SECURE_PORT};

/// Spin up a k3s container and return a connected `K8sClient` plus the
/// container handle (which must be kept alive for the cluster to persist).
async fn setup_k3s_client() -> (K8sClient, ContainerAsync<K3s>) {
    let conf_dir = tempfile::tempdir().expect("failed to create temp dir");

    let k3s = K3s::default()
        .with_conf_mount(conf_dir.path())
        .with_privileged(true)
        .with_userns_mode("host");

    let container = k3s.start().await.expect("failed to start k3s container");

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
    let k8s_client = K8sClient::new(client, vec![], ActionPermissions::default());

    (k8s_client, container)
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
    let result = handle_tool(&client, "list_deployments", &args).await;
    let text = result
        .expect("list_deployments should be a known tool")
        .expect("list_deployments should succeed");

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
    let conf_dir = tempfile::tempdir().expect("failed to create temp dir");

    let k3s = K3s::default()
        .with_conf_mount(conf_dir.path())
        .with_privileged(true)
        .with_userns_mode("host");

    let container = k3s.start().await.expect("failed to start k3s container");

    let kube_conf = container
        .image()
        .read_kube_config()
        .expect("failed to read kubeconfig from k3s");

    let kube_port = container
        .get_host_port_ipv4(KUBE_SECURE_PORT)
        .await
        .expect("failed to get mapped port");

    let kube_conf = kube_conf.replace(
        "https://127.0.0.1:6443",
        &format!("https://127.0.0.1:{kube_port}"),
    );

    let _ = rustls::crypto::ring::default_provider().install_default();

    let kubeconfig =
        kube::config::Kubeconfig::from_yaml(&kube_conf).expect("failed to parse kubeconfig");
    let config = kube::Config::from_custom_kubeconfig(
        kubeconfig,
        &kube::config::KubeConfigOptions::default(),
    )
    .await
    .expect("failed to build kube Config");

    let kube_client = kube::Client::try_from(config).expect("failed to create kube Client");

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
    let result = handle_tool(&client, "list_pods", &args).await;
    let text = result
        .expect("list_pods should be a known tool")
        .expect("list_pods should succeed");

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
