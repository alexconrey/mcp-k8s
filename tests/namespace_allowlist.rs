//! The namespace allowlist, proven for every tool.
//!
//! deckwatch (and any operator using `--namespaces`) relies on every tool
//! honouring `K8sClient`'s namespace allowlist. These tests walk the full
//! tool list (`tool_definitions` with every tool enabled) and call each
//! namespaced tool with a namespace outside the allowlist, against a mock
//! Kubernetes API server (`tower_test::mock`). Each call must be refused with
//! the allowlist error **before any request reaches the API server**.
//!
//! A tool without a `namespace` argument must be listed in one of the
//! exemption lists below with a reason, so a new tool can't silently skip the
//! check. Tools whose omitted namespace would mean "all namespaces" have their
//! own tests: under an allowlist they must refuse.

use std::pin::pin;

use http::{Request, Response};
use kube::client::Body;
use kube::Client;
use mcp_k8s::mcp::{handle_tool, tool_definitions};
use mcp_k8s::permissions::ActionPermissions;
use mcp_k8s::K8sClient;
use serde_json::{json, Value};
use tower_test::mock;

type Handle = mock::Handle<Request<Body>, Response<Body>>;

const ALLOWED: &str = "team-a";
const DENIED: &str = "kube-system";
const ALLOWLIST_ERROR: &str = "is not in the allowed list";

/// Every tool enabled, the workload policy at its (restrictive) default.
fn all_permissions() -> ActionPermissions {
    ActionPermissions::default().with_apply_manifest(true)
}

fn client_with(namespaces: &[&str]) -> (K8sClient, Handle) {
    let (svc, handle) = mock::pair::<Request<Body>, Response<Body>>();
    let client = Client::new(svc, "default");
    let namespaces = namespaces.iter().map(|s| s.to_string()).collect();
    (
        K8sClient::new(client, namespaces, all_permissions()),
        handle,
    )
}

fn restricted_client() -> (K8sClient, Handle) {
    client_with(&[ALLOWED])
}

/// A request the mock API server received.
#[derive(Debug)]
struct Seen {
    method: http::Method,
    path: String,
    query: String,
}

fn respond(status: u16, body: &Value) -> Response<Body> {
    Response::builder()
        .status(status)
        .body(Body::from(serde_json::to_vec(body).unwrap()))
        .unwrap()
}

/// Answer the next `responses.len()` requests in order and return what was
/// asked. A further request is answered with a 500 and recorded as an error.
fn serve(
    handle: Handle,
    responses: Vec<(u16, Value)>,
) -> tokio::task::JoinHandle<Result<Vec<Seen>, String>> {
    tokio::spawn(async move {
        let mut handle = pin!(handle);
        let mut seen = Vec::new();
        for (status, body) in responses {
            let Some((req, send)) = handle.next_request().await else {
                return Err(format!("expected a request; saw {seen:#?}"));
            };
            seen.push(Seen {
                method: req.method().clone(),
                path: req.uri().path().to_string(),
                query: req.uri().query().unwrap_or("").to_string(),
            });
            send.send_response(respond(status, &body));
        }
        if let Some((req, send)) = handle.next_request().await {
            let msg = format!("unexpected request: {} {}", req.method(), req.uri());
            send.send_response(respond(500, &json!({})));
            return Err(msg);
        }
        Ok(seen)
    })
}

/// Call `tool`, answering `responses`, and return its result plus the
/// requests the API server saw (or the extra request that broke the script).
async fn call(
    client: K8sClient,
    handle: Handle,
    tool: &str,
    args: &Value,
    responses: Vec<(u16, Value)>,
) -> (Option<Result<String, String>>, Result<Vec<Seen>, String>) {
    let server = serve(handle, responses);
    let result = handle_tool(&client, tool, args).await;
    drop(client);
    (result, server.await.expect("mock API server"))
}

/// Call a tool that must be refused with the allowlist error and without
/// contacting the API server. Returns a description of what went wrong.
async fn check_refused_offline(tool: &str, args: &Value) -> Result<(), String> {
    let (client, handle) = restricted_client();
    let (result, seen) = call(client, handle, tool, args, vec![]).await;
    if let Err(e) = seen {
        return Err(format!("{tool}: reached the API server ({e}) with {args}"));
    }
    match result {
        None => Err(format!("{tool}: not handled by handle_tool")),
        Some(Ok(out)) => Err(format!("{tool}: succeeded ({out}) with {args}")),
        Some(Err(e)) if e.contains(ALLOWLIST_ERROR) => Ok(()),
        Some(Err(e)) => Err(format!(
            "{tool}: refused, but not by the allowlist: {e:?} (args {args}); fix the dummy arguments or the tool"
        )),
    }
}

// ---------------------------------------------------------------------------
// Exemptions: tools without a namespace argument. Each must be justified.
// ---------------------------------------------------------------------------

/// Cluster-scoped resources and cluster-wide reads. The allowlist covers
/// namespaced objects; these are governed by `ActionPermissions` (and the
/// workload policy for writes).
const CLUSTER_SCOPED: &[&str] = &[
    "list_mutatingwebhookconfigs",
    "get_mutatingwebhookconfig",
    "list_validatingwebhookconfigs",
    "get_validatingwebhookconfig",
    "list_validatingadmissionpolicies",
    "get_validatingadmissionpolicy",
    "list_mutatingadmissionpolicies",
    "get_mutatingadmissionpolicy",
    "list_mutatingadmissionpolicybindings",
    "get_mutatingadmissionpolicybinding",
    "list_clustertrustbundles",
    "get_clustertrustbundle",
    "list_clusterroles",
    "get_clusterrole",
    "create_clusterrole",
    "update_clusterrole",
    "delete_clusterrole",
    "list_clusterrolebindings",
    "get_clusterrolebinding",
    "create_clusterrolebinding",
    "delete_clusterrolebinding",
    "list_crds",
    "get_crd",
    "list_csrs",
    "get_csr",
    "approve_csr",
    "deny_csr",
    "list_resourceslices",
    "list_deviceclasses",
    "get_deviceclass",
    "list_flowschemas",
    "get_flowschema",
    "list_prioritylevelconfigs",
    "get_prioritylevelconfig",
    "list_ingressclasses",
    "get_ingressclass",
    "create_ingressclass",
    "update_ingressclass",
    "delete_ingressclass",
    "list_ipaddresses",
    "get_ipaddress",
    "list_servicecidrs",
    "get_servicecidr",
    "list_nodes",
    "get_node",
    "get_node_metrics",
    "cordon_node",
    "uncordon_node",
    "list_priorityclasses",
    "get_priorityclass",
    "list_pvs",
    "get_pv",
    "delete_pv",
    "list_runtimeclasses",
    "get_runtimeclass",
    "list_csidrivers",
    "get_csidriver",
    "list_csinodes",
    "list_volumeattachments",
    "get_volumeattachment",
    "list_storageversionmigrations",
    "get_storageversionmigration",
    "list_storageversions",
    "get_storageversion",
    "list_storageclasses",
    "get_storageclass",
    "create_storageclass",
    "update_storageclass",
    "delete_storageclass",
    "list_volumeattributesclasses",
    "get_volumeattributesclass",
    // A SelfSubjectReview about the server's own identity: no namespace.
    "whoami",
];

/// Tools that never reach the Kubernetes API: the cluster manager answers
/// them (`handle_tool` doesn't handle them), and they take no namespace.
const NO_API: &[&str] = &["list_clusters", "switch_cluster", "get_active_cluster"];

/// Namespace-touching tools without a `namespace` argument, each covered by a
/// dedicated test in this file.
const TESTED_SEPARATELY: &[(&str, &str)] = &[
    (
        "list_namespaces",
        "filters its result through the allowlist: list_namespaces_hides_namespaces_outside_the_allowlist",
    ),
    (
        "get_namespace",
        "its name is a namespace: namespace_tools_refuse_namespaces_outside_the_allowlist",
    ),
    (
        "create_namespace",
        "its name is a namespace: namespace_tools_refuse_namespaces_outside_the_allowlist",
    ),
    (
        "update_namespace",
        "its name is a namespace: namespace_tools_refuse_namespaces_outside_the_allowlist",
    ),
    (
        "delete_namespace",
        "its name is a namespace: namespace_tools_refuse_namespaces_outside_the_allowlist",
    ),
    (
        "drain_node",
        "evicts pods in every namespace on the node: drain_node_refuses_*",
    ),
];

// ---------------------------------------------------------------------------
// Dummy arguments
// ---------------------------------------------------------------------------

/// A plausible value for a schema property.
fn dummy(schema: &Value) -> Value {
    match schema["type"].as_str() {
        Some("string") => json!("x"),
        Some("integer") | Some("number") => json!(1),
        Some("boolean") => json!(false),
        Some("array") => match schema["items"]["type"].as_str() {
            Some("string") => json!(["x"]),
            Some("integer") => json!([1]),
            _ => json!([{}]),
        },
        Some("object") => json!({}),
        _ => json!("x"),
    }
}

/// Arguments for `tool` that are valid apart from the namespace: every
/// required property gets a dummy value, then tool-specific values replace
/// the ones a dummy can't satisfy, then `namespace` is set.
fn args_for(tool: &Value, namespace: &str) -> Value {
    let name = tool["name"].as_str().unwrap();
    let schema = &tool["inputSchema"];
    let mut args = json!({});
    for key in schema["required"].as_array().into_iter().flatten() {
        let key = key.as_str().unwrap();
        args[key] = dummy(&schema["properties"][key]);
    }
    // Structured arguments a generic dummy can't fill in.
    let overrides = match name {
        "apply_manifest" => json!({
            "manifest": json!({
                "apiVersion": "v1", "kind": "ConfigMap",
                "metadata": { "name": "x", "namespace": namespace }
            }).to_string()
        }),
        "list_custom_resources" | "get_custom_resource" | "delete_custom_resource" => {
            json!({ "group": "example.com", "version": "v1", "kind": "Widget" })
        }
        "create_custom_resource" => json!({
            "group": "example.com", "version": "v1", "kind": "Widget",
            "manifest": { "apiVersion": "example.com/v1", "kind": "Widget", "metadata": { "name": "x" } }
        }),
        "update_custom_resource" => json!({
            "group": "example.com", "version": "v1", "kind": "Widget", "patch": { "spec": {} }
        }),
        "watch_resource" => json!({ "api_version": "v1", "kind": "Pod" }),
        "get_resource_json" | "get_resource_yaml" => {
            json!({ "api_version": "v1", "kind": "ConfigMap" })
        }
        "can_i" => json!({ "verb": "get", "resource": "pods" }),
        "exec_pod" => json!({ "command": ["ls"] }),
        "port_forward_check" => json!({ "port": 8080 }),
        "create_role" | "update_role" => json!({
            "rules": [{ "api_groups": [""], "resources": ["configmaps"], "verbs": ["get"] }]
        }),
        "create_rolebinding" => json!({
            "role_ref": { "kind": "Role", "name": "x", "apiGroup": "rbac.authorization.k8s.io" },
            "subjects": [{ "kind": "ServiceAccount", "name": "x", "namespace": namespace }]
        }),
        "create_networkpolicy" => json!({ "pod_selector": {}, "policy_types": ["Ingress"] }),
        "create_limitrange" => json!({ "limits": [{ "type": "Container" }] }),
        "create_resourcequota" | "update_resourcequota" => json!({ "hard": { "pods": "1" } }),
        "create_pdb" => json!({ "selector": { "app": "x" }, "min_available": 1 }),
        "create_hpa" => json!({
            "scale_target_kind": "Deployment", "scale_target_name": "x",
            "scale_target_api_version": "apps/v1", "min_replicas": 1, "max_replicas": 2
        }),
        "create_pvc" | "update_pvc" => json!({ "storage": "1Gi" }),
        "create_cronjob" => json!({ "schedule": "* * * * *", "image": "nginx" }),
        "create_job" | "create_pod" | "create_deployment" | "create_daemonset" => {
            json!({ "image": "nginx" })
        }
        "create_statefulset" => json!({ "image": "nginx", "service_name": "x" }),
        "create_configmap" | "update_configmap" => json!({ "data": { "k": "v" } }),
        "create_secret" | "update_secret" => json!({ "string_data": { "k": "v" } }),
        "update_deployment_affinity" => json!({ "node_selector": { "k": "v" } }),
        "create_ingress" | "update_ingress" => json!({ "service_name": "x", "service_port": 80 }),
        "update_networkpolicy" => json!({ "policy_types": ["Ingress"] }),
        "update_service" => json!({ "selector": { "app": "x" } }),
        "update_deployment_labels" => json!({ "labels": { "k": "v" } }),
        "update_deployment_annotations" => json!({ "annotations": { "k": "v" } }),
        _ => json!({}),
    };
    for (k, v) in overrides.as_object().unwrap() {
        args[k] = v.clone();
    }
    args["namespace"] = json!(namespace);
    args
}

fn has_namespace_property(tool: &Value) -> bool {
    tool["inputSchema"]["properties"].get("namespace").is_some()
}

fn requires_namespace(tool: &Value) -> bool {
    tool["inputSchema"]["required"]
        .as_array()
        .is_some_and(|r| r.iter().any(|k| k == "namespace"))
}

// ---------------------------------------------------------------------------
// Every namespaced tool
// ---------------------------------------------------------------------------

#[test]
fn every_tool_is_namespaced_or_explicitly_exempt() {
    let mut unclassified = Vec::new();
    let mut stale = Vec::new();
    let tools = tool_definitions(&all_permissions());
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    let separately: Vec<&str> = TESTED_SEPARATELY.iter().map(|(n, _)| *n).collect();
    for tool in &tools {
        let name = tool["name"].as_str().unwrap();
        let exempt =
            CLUSTER_SCOPED.contains(&name) || NO_API.contains(&name) || separately.contains(&name);
        match (has_namespace_property(tool), exempt) {
            (false, false) => unclassified.push(name),
            (true, true) => stale.push(name),
            _ => {}
        }
    }
    for name in CLUSTER_SCOPED.iter().chain(NO_API).chain(&separately) {
        if !names.contains(name) {
            stale.push(name);
        }
    }
    assert!(
        unclassified.is_empty(),
        "tools without a `namespace` argument that aren't classified: {unclassified:?}. \
         If a tool touches namespaced objects it must check the allowlist; add it to the \
         right list in tests/namespace_allowlist.rs with a reason (and a test if it's in \
         TESTED_SEPARATELY)."
    );
    assert!(
        stale.is_empty(),
        "exempt tools that take a namespace or no longer exist: {stale:?}"
    );
}

#[tokio::test]
async fn every_namespaced_tool_refuses_a_namespace_outside_the_allowlist() {
    let tools = tool_definitions(&all_permissions());
    let mut failures = Vec::new();
    let mut checked = 0;
    for tool in tools.iter().filter(|t| has_namespace_property(t)) {
        let name = tool["name"].as_str().unwrap();
        let args = args_for(tool, DENIED);
        if let Err(e) = check_refused_offline(name, &args).await {
            failures.push(e);
        }
        checked += 1;
    }
    assert!(
        failures.is_empty(),
        "{} of {checked} namespaced tools don't refuse namespace {DENIED:?} offline:\n{}",
        failures.len(),
        failures.join("\n")
    );
    // Guard against the filter silently matching nothing.
    assert!(checked > 100, "only {checked} namespaced tools found");
}

#[tokio::test]
async fn tools_that_require_a_namespace_refuse_to_run_without_one() {
    // An omitted required namespace must not turn into "every namespace".
    let tools = tool_definitions(&all_permissions());
    let mut failures = Vec::new();
    for tool in tools.iter().filter(|t| requires_namespace(t)) {
        let name = tool["name"].as_str().unwrap();
        let mut args = args_for(tool, DENIED);
        args.as_object_mut().unwrap().remove("namespace");
        let (client, handle) = restricted_client();
        let (result, seen) = call(client, handle, name, &args, vec![]).await;
        match (result, seen) {
            (_, Err(e)) => failures.push(format!("{name}: reached the API server: {e}")),
            (Some(Ok(out)), _) => failures.push(format!("{name}: succeeded: {out}")),
            (None, _) => failures.push(format!("{name}: not handled")),
            (Some(Err(_)), Ok(_)) => {}
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

// ---------------------------------------------------------------------------
// apply_manifest: metadata.namespace and the namespace argument
// ---------------------------------------------------------------------------

fn config_map(namespace: Option<&str>) -> String {
    let mut m = json!({ "apiVersion": "v1", "kind": "ConfigMap", "metadata": { "name": "c" } });
    if let Some(ns) = namespace {
        m["metadata"]["namespace"] = json!(ns);
    }
    m.to_string()
}

#[tokio::test]
async fn apply_manifest_checks_both_namespace_sources_against_the_allowlist() {
    // metadata.namespace only
    check_refused_offline(
        "apply_manifest",
        &json!({ "manifest": config_map(Some(DENIED)) }),
    )
    .await
    .unwrap();
    // namespace argument only
    check_refused_offline(
        "apply_manifest",
        &json!({ "manifest": config_map(None), "namespace": DENIED }),
    )
    .await
    .unwrap();
    // both, agreeing
    check_refused_offline(
        "apply_manifest",
        &json!({ "manifest": config_map(Some(DENIED)), "namespace": DENIED }),
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn apply_manifest_rejects_an_allowed_argument_with_a_different_manifest_namespace() {
    let (client, handle) = restricted_client();
    let args = json!({ "manifest": config_map(Some(DENIED)), "namespace": ALLOWED });
    let (result, seen) = call(client, handle, "apply_manifest", &args, vec![]).await;
    seen.expect("no request may reach the API server");
    let err = result.unwrap().unwrap_err();
    assert!(
        err.contains("does not match the namespace argument"),
        "{err}"
    );
}

// ---------------------------------------------------------------------------
// Omitted namespace = all namespaces: refused under an allowlist
// ---------------------------------------------------------------------------

fn core_v1_discovery() -> (u16, Value) {
    (
        200,
        json!({
            "kind": "APIResourceList", "apiVersion": "v1", "groupVersion": "v1",
            "resources": [
                { "name": "pods", "singularName": "pod", "namespaced": true, "kind": "Pod", "verbs": ["get", "list", "watch"] },
                { "name": "configmaps", "singularName": "configmap", "namespaced": true, "kind": "ConfigMap", "verbs": ["get", "list", "watch"] }
            ]
        }),
    )
}

fn widget_discovery() -> (u16, Value) {
    (
        200,
        json!({
            "kind": "APIResourceList", "apiVersion": "v1", "groupVersion": "example.com/v1",
            "resources": [{ "name": "widgets", "singularName": "widget", "namespaced": true, "kind": "Widget", "verbs": ["create", "delete", "get", "list", "patch", "watch"] }]
        }),
    )
}

fn empty_list(kind: &str) -> (u16, Value) {
    (
        200,
        json!({ "apiVersion": "v1", "kind": kind, "metadata": {}, "items": [] }),
    )
}

/// Under an allowlist, an omitted namespace may only cost a discovery
/// request: the tool must refuse instead of acting on every namespace.
async fn refused_without_namespace(tool: &str, args: Value, discovery: Vec<(u16, Value)>) {
    let expected_requests = discovery.len();
    let (client, handle) = restricted_client();
    let (result, seen) = call(client, handle, tool, &args, discovery).await;
    let seen = seen.unwrap_or_else(|e| panic!("{tool}: {e}"));
    assert_eq!(seen.len(), expected_requests, "{tool}: {seen:?}");
    assert!(
        seen.iter().all(|s| s.path.starts_with("/api")
            && !s.path.contains("/namespaces")
            && s.method == http::Method::GET
            && s.query.is_empty()),
        "{tool}: only discovery may run: {seen:?}"
    );
    let err = result.unwrap().unwrap_err();
    assert!(err.contains("namespace is required"), "{tool}: {err}");
}

#[tokio::test]
async fn watch_resource_without_a_namespace_is_refused_under_an_allowlist() {
    refused_without_namespace(
        "watch_resource",
        json!({ "api_version": "v1", "kind": "Pod", "duration_seconds": 1 }),
        vec![core_v1_discovery()],
    )
    .await;
}

#[tokio::test]
async fn list_custom_resources_without_a_namespace_is_refused_under_an_allowlist() {
    refused_without_namespace(
        "list_custom_resources",
        json!({ "group": "example.com", "version": "v1", "kind": "Widget" }),
        vec![widget_discovery()],
    )
    .await;
}

#[tokio::test]
async fn list_csistoragecapacities_without_a_namespace_is_refused_under_an_allowlist() {
    refused_without_namespace("list_csistoragecapacities", json!({}), vec![]).await;
}

#[tokio::test]
async fn custom_resource_object_tools_need_a_namespace_for_namespaced_kinds() {
    // get/delete without a namespace used to address the cluster-scoped path.
    for tool in ["get_custom_resource", "delete_custom_resource"] {
        let (client, handle) = restricted_client();
        let args =
            json!({ "group": "example.com", "version": "v1", "kind": "Widget", "name": "w" });
        let (result, seen) = call(client, handle, tool, &args, vec![widget_discovery()]).await;
        assert_eq!(seen.unwrap().len(), 1, "{tool}: discovery only");
        let err = result.unwrap().unwrap_err();
        assert!(
            err.contains("namespace argument is required"),
            "{tool}: {err}"
        );
    }
}

#[tokio::test]
async fn without_an_allowlist_an_omitted_namespace_still_lists_every_namespace() {
    let (client, handle) = client_with(&[]);
    let (result, seen) = call(
        client,
        handle,
        "list_csistoragecapacities",
        &json!({}),
        vec![empty_list("CSIStorageCapacityList")],
    )
    .await;
    result.unwrap().unwrap();
    let seen = seen.unwrap();
    assert_eq!(seen[0].path, "/apis/storage.k8s.io/v1/csistoragecapacities");
}

// ---------------------------------------------------------------------------
// Namespace objects
// ---------------------------------------------------------------------------

#[tokio::test]
async fn namespace_tools_refuse_namespaces_outside_the_allowlist() {
    for tool in [
        "get_namespace",
        "create_namespace",
        "update_namespace",
        "delete_namespace",
    ] {
        check_refused_offline(tool, &json!({ "name": DENIED, "labels": { "k": "v" } }))
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn delete_namespace_is_allowed_inside_the_allowlist() {
    let (client, handle) = restricted_client();
    let (result, seen) = call(
        client,
        handle,
        "delete_namespace",
        &json!({ "name": ALLOWED }),
        vec![(
            200,
            json!({ "kind": "Status", "apiVersion": "v1", "status": "Success" }),
        )],
    )
    .await;
    result.unwrap().unwrap();
    let seen = seen.unwrap();
    assert_eq!(seen[0].method, http::Method::DELETE);
    assert_eq!(seen[0].path, format!("/api/v1/namespaces/{ALLOWED}"));
}

#[tokio::test]
async fn list_namespaces_hides_namespaces_outside_the_allowlist() {
    let (client, handle) = restricted_client();
    let list = json!({
        "apiVersion": "v1", "kind": "NamespaceList", "metadata": {},
        "items": [
            { "apiVersion": "v1", "kind": "Namespace", "metadata": { "name": ALLOWED } },
            { "apiVersion": "v1", "kind": "Namespace", "metadata": { "name": DENIED } }
        ]
    });
    let (result, _) = call(
        client,
        handle,
        "list_namespaces",
        &json!({}),
        vec![(200, list)],
    )
    .await;
    let out = result.unwrap().unwrap();
    assert_eq!(out, ALLOWED);
}

// ---------------------------------------------------------------------------
// drain_node
// ---------------------------------------------------------------------------

fn pod(namespace: &str, name: &str, owner_kind: Option<&str>) -> Value {
    let mut p = json!({
        "apiVersion": "v1", "kind": "Pod",
        "metadata": { "name": name, "namespace": namespace },
        "spec": { "nodeName": "n1", "containers": [{ "name": "c", "image": "nginx" }] }
    });
    if let Some(kind) = owner_kind {
        p["metadata"]["ownerReferences"] = json!([{
            "apiVersion": "apps/v1", "kind": kind, "name": "owner", "uid": "u", "controller": true
        }]);
    }
    p
}

fn pod_list(items: Vec<Value>) -> (u16, Value) {
    (
        200,
        json!({ "apiVersion": "v1", "kind": "PodList", "metadata": {}, "items": items }),
    )
}

#[tokio::test]
async fn drain_node_refuses_to_evict_pods_outside_the_allowlist() {
    let (client, handle) = restricted_client();
    let (result, seen) = call(
        client,
        handle,
        "drain_node",
        &json!({ "name": "n1" }),
        vec![pod_list(vec![
            pod(ALLOWED, "web", Some("ReplicaSet")),
            pod(DENIED, "coredns", Some("ReplicaSet")),
        ])],
    )
    .await;
    // Only the pod list ran: no cordon, no eviction.
    let seen = seen.unwrap();
    assert_eq!(seen.len(), 1, "{seen:?}");
    assert_eq!(seen[0].method, http::Method::GET);
    let err = result.unwrap().unwrap_err();
    assert!(err.contains("outside the allowed list"), "{err}");
    assert!(!err.contains("coredns"), "pod names must not leak: {err}");
}

#[tokio::test]
async fn drain_node_ignores_daemonset_pods_outside_the_allowlist() {
    let (client, handle) = restricted_client();
    let pods = vec![
        pod(ALLOWED, "web", Some("ReplicaSet")),
        pod(DENIED, "kube-proxy", Some("DaemonSet")),
    ];
    let node = json!({ "apiVersion": "v1", "kind": "Node", "metadata": { "name": "n1" }, "spec": { "unschedulable": true } });
    let (result, seen) = call(
        client,
        handle,
        "drain_node",
        &json!({ "name": "n1" }),
        vec![
            pod_list(pods.clone()),
            (200, node),
            pod_list(pods),
            (200, pod(ALLOWED, "web", None)),
        ],
    )
    .await;
    let out = result.unwrap().unwrap();
    assert!(out.contains("\"evicted_pods\": 1"), "{out}");
    let seen = seen.unwrap();
    assert_eq!(seen[1].method, http::Method::PATCH);
    assert_eq!(seen[3].method, http::Method::DELETE);
    assert_eq!(
        seen[3].path,
        format!("/api/v1/namespaces/{ALLOWED}/pods/web")
    );
}
