//! Integration tests exercising the full MCP tool dispatch path with a mock
//! Kubernetes API server using `tower_test::mock`.
//!
//! Each test creates a mock `kube::Client` via `tower_test::mock::pair()`,
//! wires it into a `K8sClient`, and then calls `mcp_k8s::mcp::handle_tool()`
//! to exercise the real handler code end-to-end.

use std::pin::pin;

use http::{Request, Response};
use kube::client::Body;
use kube::Client;
use mcp_k8s::permissions::ActionPermissions;
use mcp_k8s::K8sClient;
use tower_test::mock;

// ---------------------------------------------------------------------------
// Helper: build a K8sClient backed by a tower-test mock
// ---------------------------------------------------------------------------

fn mock_client() -> (K8sClient, mock::Handle<Request<Body>, Response<Body>>) {
    let (mock_service, handle) = mock::pair::<Request<Body>, Response<Body>>();
    let client = Client::new(mock_service, "default");
    let k8s_client = K8sClient::new(client, vec![], ActionPermissions::default());
    (k8s_client, handle)
}

/// Build a mock client with restricted namespace access.
fn mock_client_with_namespaces(
    allowed: Vec<String>,
) -> (K8sClient, mock::Handle<Request<Body>, Response<Body>>) {
    let (mock_service, handle) = mock::pair::<Request<Body>, Response<Body>>();
    let client = Client::new(mock_service, "default");
    let k8s_client = K8sClient::new(client, allowed, ActionPermissions::default());
    (k8s_client, handle)
}

/// Build a mock client with custom permissions.
fn mock_client_with_permissions(
    perms: ActionPermissions,
) -> (K8sClient, mock::Handle<Request<Body>, Response<Body>>) {
    let (mock_service, handle) = mock::pair::<Request<Body>, Response<Body>>();
    let client = Client::new(mock_service, "default");
    let k8s_client = K8sClient::new(client, vec![], perms);
    (k8s_client, handle)
}

/// Serialize a value to a `Response<Body>`.
fn json_response<T: serde::Serialize>(val: &T) -> Response<Body> {
    Response::new(Body::from(serde_json::to_vec(val).unwrap()))
}

// ---------------------------------------------------------------------------
// 1. test_list_namespaces
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_list_namespaces() {
    let (client, handle) = mock_client();

    let mock_task = tokio::spawn(async move {
        let mut handle = pin!(handle);
        let (request, send) = handle.next_request().await.expect("service not called");

        assert_eq!(request.method(), http::Method::GET);
        assert!(
            request.uri().path().contains("namespaces"),
            "expected namespaces path, got: {}",
            request.uri()
        );

        let ns_list = serde_json::json!({
            "apiVersion": "v1",
            "kind": "NamespaceList",
            "metadata": {},
            "items": [
                {
                    "apiVersion": "v1",
                    "kind": "Namespace",
                    "metadata": { "name": "default" }
                },
                {
                    "apiVersion": "v1",
                    "kind": "Namespace",
                    "metadata": { "name": "kube-system" }
                }
            ]
        });

        send.send_response(json_response(&ns_list));
    });

    let args = serde_json::json!({});
    let result = mcp_k8s::mcp::handle_tool(&client, "list_namespaces", &args).await;

    let text = result
        .expect("tool should be recognized")
        .expect("tool should succeed");
    assert!(
        text.contains("default"),
        "response should contain 'default'"
    );
    assert!(
        text.contains("kube-system"),
        "response should contain 'kube-system'"
    );

    mock_task.await.unwrap();
}

// ---------------------------------------------------------------------------
// 2. test_list_pods
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_list_pods() {
    let (client, handle) = mock_client();

    let mock_task = tokio::spawn(async move {
        let mut handle = pin!(handle);
        let (request, send) = handle.next_request().await.expect("service not called");

        assert_eq!(request.method(), http::Method::GET);
        assert!(
            request.uri().path().contains("/namespaces/default/pods"),
            "expected pods path, got: {}",
            request.uri()
        );

        let pod_list = serde_json::json!({
            "apiVersion": "v1",
            "kind": "PodList",
            "metadata": {},
            "items": [{
                "apiVersion": "v1",
                "kind": "Pod",
                "metadata": { "name": "nginx-pod", "namespace": "default" },
                "spec": {
                    "containers": [{ "name": "nginx", "image": "nginx:1.25" }]
                },
                "status": { "phase": "Running" }
            }]
        });

        send.send_response(json_response(&pod_list));
    });

    let args = serde_json::json!({"namespace": "default"});
    let result = mcp_k8s::mcp::handle_tool(&client, "list_pods", &args).await;

    let text = result
        .expect("tool should be recognized")
        .expect("tool should succeed");
    assert!(
        text.contains("nginx-pod"),
        "response should contain pod name"
    );
    assert!(
        text.contains("Running"),
        "response should contain pod phase"
    );

    mock_task.await.unwrap();
}

// ---------------------------------------------------------------------------
// 3. test_list_configmaps
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_list_configmaps() {
    let (client, handle) = mock_client();

    let mock_task = tokio::spawn(async move {
        let mut handle = pin!(handle);
        let (request, send) = handle.next_request().await.expect("service not called");

        assert_eq!(request.method(), http::Method::GET);
        assert!(
            request
                .uri()
                .path()
                .contains("/namespaces/default/configmaps"),
            "expected configmaps path, got: {}",
            request.uri()
        );

        let cm_list = serde_json::json!({
            "apiVersion": "v1",
            "kind": "ConfigMapList",
            "metadata": {},
            "items": [{
                "apiVersion": "v1",
                "kind": "ConfigMap",
                "metadata": { "name": "app-config", "namespace": "default" },
                "data": { "key1": "value1" }
            }]
        });

        send.send_response(json_response(&cm_list));
    });

    let args = serde_json::json!({"namespace": "default"});
    let result = mcp_k8s::mcp::handle_tool(&client, "list_configmaps", &args).await;

    let text = result
        .expect("tool should be recognized")
        .expect("tool should succeed");
    assert!(
        text.contains("app-config"),
        "response should contain configmap name"
    );

    mock_task.await.unwrap();
}

// ---------------------------------------------------------------------------
// 4. test_get_deployment
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_get_deployment() {
    let (client, handle) = mock_client();

    // get_deployment makes 3 sequential API calls:
    //   1. GET deployment
    //   2. LIST pods (by label selector)
    //   3. LIST ingresses
    let mock_task = tokio::spawn(async move {
        let mut handle = pin!(handle);

        // 1. GET /apis/apps/v1/namespaces/default/deployments/nginx
        {
            let (request, send) = handle.next_request().await.expect("service not called (1)");
            assert_eq!(request.method(), http::Method::GET);
            assert!(
                request.uri().path().contains("/deployments/nginx"),
                "expected deployment GET, got: {}",
                request.uri()
            );

            let dep = serde_json::json!({
                "apiVersion": "apps/v1",
                "kind": "Deployment",
                "metadata": { "name": "nginx", "namespace": "default" },
                "spec": {
                    "replicas": 2,
                    "selector": {
                        "matchLabels": { "app": "nginx" }
                    },
                    "template": {
                        "metadata": { "labels": { "app": "nginx" } },
                        "spec": {
                            "containers": [{ "name": "nginx", "image": "nginx:1.25" }]
                        }
                    }
                },
                "status": {
                    "replicas": 2,
                    "readyReplicas": 2,
                    "availableReplicas": 2,
                    "updatedReplicas": 2
                }
            });

            send.send_response(json_response(&dep));
        }

        // 2. LIST /api/v1/namespaces/default/pods?labelSelector=app%3Dnginx
        {
            let (request, send) = handle.next_request().await.expect("service not called (2)");
            assert_eq!(request.method(), http::Method::GET);
            assert!(
                request.uri().path().contains("/pods"),
                "expected pods LIST, got: {}",
                request.uri()
            );

            let pod_list = serde_json::json!({
                "apiVersion": "v1",
                "kind": "PodList",
                "metadata": {},
                "items": [{
                    "apiVersion": "v1",
                    "kind": "Pod",
                    "metadata": { "name": "nginx-abc123", "namespace": "default" },
                    "spec": {
                        "containers": [{ "name": "nginx", "image": "nginx:1.25" }]
                    },
                    "status": { "phase": "Running" }
                }]
            });

            send.send_response(json_response(&pod_list));
        }

        // 3. LIST /apis/networking.k8s.io/v1/namespaces/default/ingresses
        {
            let (request, send) = handle.next_request().await.expect("service not called (3)");
            assert_eq!(request.method(), http::Method::GET);
            assert!(
                request.uri().path().contains("/ingresses"),
                "expected ingresses LIST, got: {}",
                request.uri()
            );

            let ing_list = serde_json::json!({
                "apiVersion": "networking.k8s.io/v1",
                "kind": "IngressList",
                "metadata": {},
                "items": []
            });

            send.send_response(json_response(&ing_list));
        }
    });

    let args = serde_json::json!({"namespace": "default", "name": "nginx"});
    let result = mcp_k8s::mcp::handle_tool(&client, "get_deployment", &args).await;

    let text = result
        .expect("tool should be recognized")
        .expect("tool should succeed");

    // Parse the result to verify structure
    let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert!(parsed.get("detail").is_some(), "should have detail field");
    assert!(parsed.get("pods").is_some(), "should have pods field");
    assert!(
        parsed.get("ingresses").is_some(),
        "should have ingresses field"
    );
    assert!(
        text.contains("nginx"),
        "response should contain deployment name"
    );

    mock_task.await.unwrap();
}

// ---------------------------------------------------------------------------
// 5. test_create_configmap
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_create_configmap() {
    let (client, handle) = mock_client();

    let mock_task = tokio::spawn(async move {
        let mut handle = pin!(handle);
        let (request, send) = handle.next_request().await.expect("service not called");

        assert_eq!(request.method(), http::Method::POST);
        assert!(
            request
                .uri()
                .path()
                .contains("/namespaces/default/configmaps"),
            "expected configmaps POST, got: {}",
            request.uri()
        );

        // Verify the request body contains the expected data
        let body_bytes = request.into_body().collect_bytes().await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(body["metadata"]["name"], "my-config");
        assert_eq!(body["data"]["app.conf"], "port=8080");

        // Return the "created" configmap
        let cm = serde_json::json!({
            "apiVersion": "v1",
            "kind": "ConfigMap",
            "metadata": { "name": "my-config", "namespace": "default" },
            "data": { "app.conf": "port=8080" }
        });

        send.send_response(json_response(&cm));
    });

    let args = serde_json::json!({
        "namespace": "default",
        "name": "my-config",
        "data": {"app.conf": "port=8080"}
    });
    let result = mcp_k8s::mcp::handle_tool(&client, "create_configmap", &args).await;

    let text = result
        .expect("tool should be recognized")
        .expect("tool should succeed");
    assert!(
        text.contains("my-config"),
        "response should contain configmap name"
    );

    mock_task.await.unwrap();
}

// ---------------------------------------------------------------------------
// 6. test_delete_pod
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_delete_pod() {
    let (client, handle) = mock_client();

    let mock_task = tokio::spawn(async move {
        let mut handle = pin!(handle);
        let (request, send) = handle.next_request().await.expect("service not called");

        assert_eq!(request.method(), http::Method::DELETE);
        assert!(
            request
                .uri()
                .path()
                .contains("/namespaces/default/pods/test-pod"),
            "expected pod DELETE, got: {}",
            request.uri()
        );

        // The K8s API returns a Status or the deleted Pod on DELETE.
        // kube-rs expects either. We return a Status-like response.
        let status = serde_json::json!({
            "apiVersion": "v1",
            "kind": "Pod",
            "metadata": {
                "name": "test-pod",
                "namespace": "default"
            }
        });

        send.send_response(Response::new(Body::from(
            serde_json::to_vec(&status).unwrap(),
        )));
    });

    let args = serde_json::json!({"namespace": "default", "name": "test-pod"});
    let result = mcp_k8s::mcp::handle_tool(&client, "delete_pod", &args).await;

    let text = result
        .expect("tool should be recognized")
        .expect("tool should succeed");
    assert!(
        text.contains("test-pod"),
        "response should confirm deleted pod"
    );
    assert!(
        text.contains("deleted"),
        "response should mention 'deleted'"
    );

    mock_task.await.unwrap();
}

// ---------------------------------------------------------------------------
// 7. test_permission_denied
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_permission_denied() {
    // Disable create actions globally
    let perms = ActionPermissions::new(true, false, false, vec![], true, true);
    let (client, _handle) = mock_client_with_permissions(perms);

    let args = serde_json::json!({
        "namespace": "default",
        "name": "denied-config",
        "data": {"key": "value"}
    });
    let result = mcp_k8s::mcp::handle_tool(&client, "create_configmap", &args).await;

    let err = result
        .expect("tool should be recognized")
        .expect_err("tool should return an error");
    assert!(
        err.contains("not allowed"),
        "error should mention 'not allowed', got: {err}"
    );
}

// ---------------------------------------------------------------------------
// 8. test_namespace_not_allowed
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_namespace_not_allowed() {
    // Only allow "allowed-ns" namespace
    let (client, _handle) = mock_client_with_namespaces(vec!["allowed-ns".to_string()]);

    let args = serde_json::json!({"namespace": "forbidden-ns"});
    let result = mcp_k8s::mcp::handle_tool(&client, "list_pods", &args).await;

    let err = result
        .expect("tool should be recognized")
        .expect_err("tool should return an error");
    assert!(
        err.contains("forbidden-ns"),
        "error should mention the forbidden namespace, got: {err}"
    );
    assert!(
        err.contains("not in the allowed list"),
        "error should mention namespace not allowed, got: {err}"
    );
}
