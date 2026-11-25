//! Request-body tests for the typed create/update tools outside the workload
//! family (ConfigMap, Secret, PVC, PDB, ServiceAccount, HPA, RBAC), against a
//! mock Kubernetes API server.

use std::pin::pin;

use http::{Request, Response};
use kube::client::Body;
use kube::Client;
use mcp_k8s::mcp::handle_tool;
use mcp_k8s::permissions::ActionPermissions;
use mcp_k8s::K8sClient;
use serde_json::{json, Value};
use tower_test::mock;

type Handle = mock::Handle<Request<Body>, Response<Body>>;

fn client() -> (K8sClient, Handle) {
    let (svc, handle) = mock::pair::<Request<Body>, Response<Body>>();
    let client = Client::new(svc, "default");
    (
        K8sClient::new(client, vec![], ActionPermissions::default()),
        handle,
    )
}

/// Call `tool`, answer its single request with `response`, and return the
/// request (method, path, body).
async fn one_request(tool: &str, args: Value, response: Value) -> (String, String, Value) {
    let (client, handle) = client();
    let server = tokio::spawn(async move {
        let mut handle = pin!(handle);
        let (req, send) = handle.next_request().await.expect("expected a request");
        let method = req.method().to_string();
        let path = req.uri().path().to_string();
        let bytes = req.into_body().collect_bytes().await.unwrap();
        send.send_response(
            Response::builder()
                .status(200)
                .body(Body::from(serde_json::to_vec(&response).unwrap()))
                .unwrap(),
        );
        (
            method,
            path,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    });
    handle_tool(&client, tool, &args)
        .await
        .unwrap_or_else(|| panic!("{tool} should be a known tool"))
        .unwrap_or_else(|e| panic!("{tool} failed: {e}"));
    server.await.unwrap()
}

fn meta(kind: &str, api_version: &str) -> Value {
    json!({ "apiVersion": api_version, "kind": kind, "metadata": { "name": "x", "namespace": "default" } })
}

#[tokio::test]
async fn configmap_binary_data_immutable_labels() {
    let (_, _, body) = one_request(
        "create_configmap",
        json!({
            "namespace": "default", "name": "cfg",
            "data": { "a": "1" }, "binary_data": { "logo.png": "iVBORw0K" },
            "immutable": true, "labels": { "team": "web" }, "annotations": { "note": "x" }
        }),
        meta("ConfigMap", "v1"),
    )
    .await;
    assert_eq!(body["binaryData"]["logo.png"], "iVBORw0K");
    assert_eq!(body["immutable"], true);
    assert_eq!(body["metadata"]["labels"]["team"], "web");
    assert_eq!(
        body["metadata"]["labels"]["app.kubernetes.io/managed-by"],
        "mcp-k8s"
    );
    assert_eq!(body["metadata"]["annotations"]["note"], "x");
}

#[tokio::test]
async fn configmap_needs_some_data() {
    let (client, _handle) = client();
    let err = handle_tool(
        &client,
        "create_configmap",
        &json!({ "namespace": "default", "name": "cfg" }),
    )
    .await
    .unwrap()
    .unwrap_err();
    assert!(err.contains("data or binary_data"), "{err}");
}

#[tokio::test]
async fn secret_base64_data_and_legacy_string_data() {
    let (_, _, body) = one_request(
        "create_secret",
        json!({ "namespace": "default", "name": "s", "data": { "token": "c2VjcmV0" }, "immutable": true, "labels": { "a": "b" } }),
        meta("Secret", "v1"),
    )
    .await;
    assert_eq!(body["data"]["token"], "c2VjcmV0");
    assert_eq!(body["immutable"], true);
    assert_eq!(body["metadata"]["labels"]["a"], "b");

    let (_, _, body) = one_request(
        "create_secret",
        json!({ "namespace": "default", "name": "s", "string_data": { "password": "x" } }),
        meta("Secret", "v1"),
    )
    .await;
    assert_eq!(body["stringData"]["password"], "x");
    assert_eq!(body["type"], "Opaque");
}

#[tokio::test]
async fn pvc_volume_mode_data_source_and_selector() {
    let (_, path, body) = one_request(
        "create_pvc",
        json!({
            "namespace": "default", "name": "restore", "storage": "5Gi",
            "volume_mode": "Block",
            "data_source": { "kind": "VolumeSnapshot", "name": "nightly" },
            "selector": { "tier": "fast" },
            "labels": { "app": "db" }
        }),
        meta("PersistentVolumeClaim", "v1"),
    )
    .await;
    assert_eq!(path, "/api/v1/namespaces/default/persistentvolumeclaims");
    let spec = &body["spec"];
    assert_eq!(spec["volumeMode"], "Block");
    assert_eq!(
        spec["dataSource"],
        json!({ "apiGroup": "snapshot.storage.k8s.io", "kind": "VolumeSnapshot", "name": "nightly" })
    );
    assert_eq!(spec["selector"]["matchLabels"]["tier"], "fast");
    assert_eq!(spec["resources"]["requests"]["storage"], "5Gi");
    assert_eq!(body["metadata"]["labels"]["app"], "db");
}

#[tokio::test]
async fn pdb_unhealthy_pod_eviction_policy() {
    let (_, _, body) = one_request(
        "create_pdb",
        json!({ "namespace": "default", "name": "web", "min_available": 1, "selector": { "app": "web" }, "unhealthy_pod_eviction_policy": "AlwaysAllow" }),
        meta("PodDisruptionBudget", "policy/v1"),
    )
    .await;
    assert_eq!(body["spec"]["unhealthyPodEvictionPolicy"], "AlwaysAllow");

    let (method, _, body) = one_request(
        "update_pdb",
        json!({ "namespace": "default", "name": "web", "unhealthy_pod_eviction_policy": "IfHealthyBudget" }),
        meta("PodDisruptionBudget", "policy/v1"),
    )
    .await;
    assert_eq!(method, "PATCH");
    assert_eq!(
        body["spec"]["unhealthyPodEvictionPolicy"],
        "IfHealthyBudget"
    );
}

#[tokio::test]
async fn serviceaccount_image_pull_secrets_and_labels() {
    let (_, _, body) = one_request(
        "create_serviceaccount",
        json!({ "namespace": "default", "name": "builder", "image_pull_secrets": ["regcred"], "labels": { "team": "ci" }, "automount_service_account_token": false }),
        meta("ServiceAccount", "v1"),
    )
    .await;
    assert_eq!(body["imagePullSecrets"], json!([{ "name": "regcred" }]));
    assert_eq!(body["metadata"]["labels"]["team"], "ci");
    assert_eq!(body["automountServiceAccountToken"], false);
}

#[tokio::test]
async fn hpa_memory_custom_metrics_and_behavior() {
    let (_, _, body) = one_request(
        "create_hpa",
        json!({
            "namespace": "default", "name": "web",
            "scale_target_kind": "Deployment", "scale_target_name": "web", "scale_target_api_version": "apps/v1",
            "min_replicas": 2, "max_replicas": 10,
            "target_cpu_utilization": 70, "target_memory_utilization": 80,
            "metrics": [{ "type": "Pods", "pods": { "metric": { "name": "requests_per_second" }, "target": { "type": "AverageValue", "average_value": "100" } } }],
            "behavior": { "scale_down": { "stabilization_window_seconds": 300 } }
        }),
        meta("HorizontalPodAutoscaler", "autoscaling/v2"),
    )
    .await;
    let metrics = body["spec"]["metrics"].as_array().unwrap();
    assert_eq!(metrics.len(), 3);
    assert_eq!(
        metrics[0]["pods"]["target"]["averageValue"], "100",
        "snake_case keys are converted"
    );
    assert_eq!(metrics[1]["resource"]["name"], "cpu");
    assert_eq!(metrics[2]["resource"]["name"], "memory");
    assert_eq!(metrics[2]["resource"]["target"]["averageUtilization"], 80);
    assert_eq!(
        body["spec"]["behavior"]["scaleDown"]["stabilizationWindowSeconds"],
        300
    );

    let (_, _, body) = one_request(
        "update_hpa",
        json!({ "namespace": "default", "name": "web", "target_memory_utilization": 60, "max_replicas": 20 }),
        meta("HorizontalPodAutoscaler", "autoscaling/v2"),
    )
    .await;
    assert_eq!(body["spec"]["maxReplicas"], 20);
    assert_eq!(body["spec"]["metrics"][0]["resource"]["name"], "memory");
}

#[tokio::test]
async fn rbac_objects_take_labels_and_annotations() {
    let (_, _, body) = one_request(
        "create_role",
        json!({ "namespace": "default", "name": "reader", "rules": [{ "api_groups": [""], "resources": ["pods"], "verbs": ["get"] }], "labels": { "team": "a" }, "annotations": { "owner": "me" } }),
        meta("Role", "rbac.authorization.k8s.io/v1"),
    )
    .await;
    assert_eq!(body["metadata"]["labels"]["team"], "a");
    assert_eq!(body["metadata"]["annotations"]["owner"], "me");
    assert_eq!(
        body["metadata"]["labels"]["app.kubernetes.io/managed-by"],
        "mcp-k8s"
    );
}

#[tokio::test]
async fn networkpolicy_takes_labels() {
    let (_, _, body) = one_request(
        "create_networkpolicy",
        json!({ "namespace": "default", "name": "deny-all", "pod_selector": {}, "policy_types": ["Ingress"], "labels": { "tier": "sec" } }),
        meta("NetworkPolicy", "networking.k8s.io/v1"),
    )
    .await;
    assert_eq!(body["metadata"]["labels"]["tier"], "sec");
}
