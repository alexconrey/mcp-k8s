//! Guardrail tests through the full MCP tool dispatch path, against a mock
//! Kubernetes API server (`tower_test::mock`).
//!
//! The rejection tests check that a request breaking the workload policy is
//! refused *before* anything reaches the API server. The acceptance tests
//! check the request bodies the typed tools send, the old minimal arguments,
//! and the dry-run sequence update tools use.
//!
//! `apply_manifest` is deliberately different: it's off by default, and when
//! an operator enables it it's a raw server-side apply that skips the workload
//! policy (keeping only the namespace allowlist and the create/update
//! switches). Its tests check both halves.

use std::pin::pin;

use http::{Request, Response};
use kube::client::Body;
use kube::Client;
use mcp_k8s::mcp::handle_tool;
use mcp_k8s::permissions::ActionPermissions;
use mcp_k8s::policy::WorkloadPolicy;
use mcp_k8s::K8sClient;
use serde_json::{json, Value};
use tower_test::mock;

type Handle = mock::Handle<Request<Body>, Response<Body>>;

fn client_with(perms: ActionPermissions, namespaces: &[&str]) -> (K8sClient, Handle) {
    let (svc, handle) = mock::pair::<Request<Body>, Response<Body>>();
    let client = Client::new(svc, "default");
    let namespaces = namespaces.iter().map(|s| s.to_string()).collect();
    (K8sClient::new(client, namespaces, perms), handle)
}

fn strict_client() -> (K8sClient, Handle) {
    client_with(ActionPermissions::default(), &[])
}

/// The restrictive defaults, with apply_manifest enabled by the operator.
fn raw_apply_client() -> (K8sClient, Handle) {
    client_with(ActionPermissions::default().with_apply_manifest(true), &[])
}

fn client_with_policy(policy: WorkloadPolicy) -> (K8sClient, Handle) {
    client_with(
        ActionPermissions::default().with_workload_policy(policy),
        &[],
    )
}

/// A request the mock API server received.
#[derive(Debug)]
struct Seen {
    method: http::Method,
    path: String,
    query: String,
    body: Value,
}

fn respond(status: u16, body: &Value) -> Response<Body> {
    Response::builder()
        .status(status)
        .body(Body::from(serde_json::to_vec(body).unwrap()))
        .unwrap()
}

/// Answer the next `responses.len()` requests in order and return what was
/// asked. After that, any further request fails the test.
fn serve(handle: Handle, responses: Vec<(u16, Value)>) -> tokio::task::JoinHandle<Vec<Seen>> {
    tokio::spawn(async move {
        let mut handle = pin!(handle);
        let mut seen = Vec::new();
        for (status, body) in responses {
            let (req, send) = handle
                .next_request()
                .await
                .unwrap_or_else(|| panic!("expected a request; saw {seen:#?}"));
            let method = req.method().clone();
            let path = req.uri().path().to_string();
            let query = req.uri().query().unwrap_or("").to_string();
            let bytes = req.into_body().collect_bytes().await.unwrap();
            let body_in = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
            seen.push(Seen {
                method,
                path,
                query,
                body: body_in,
            });
            send.send_response(respond(status, &body));
        }
        if let Some((req, send)) = handle.next_request().await {
            send.send_response(respond(500, &json!({})));
            panic!("unexpected extra request: {} {}", req.method(), req.uri());
        }
        seen
    })
}

/// Drop the client (closing the mock connection) and collect what the mock
/// API server saw, failing on any extra request.
async fn finish(client: K8sClient, server: tokio::task::JoinHandle<Vec<Seen>>) -> Vec<Seen> {
    drop(client);
    server.await.expect("mock API server")
}

/// Call a tool that must be refused without contacting the API server.
async fn rejected_offline(client: K8sClient, handle: Handle, tool: &str, args: Value) -> String {
    let server = serve(handle, vec![]);
    let result = handle_tool(&client, tool, &args)
        .await
        .unwrap_or_else(|| panic!("{tool} should be a known tool"));
    drop(client);
    server.await.expect("no request may reach the API server");
    result.expect_err("the request should have been rejected")
}

fn not_found() -> (u16, Value) {
    (
        404,
        json!({ "kind": "Status", "apiVersion": "v1", "status": "Failure", "reason": "NotFound", "code": 404, "message": "not found" }),
    )
}

fn privileged_template() -> Value {
    json!({
        "metadata": { "labels": { "app": "web" } },
        "spec": { "containers": [{ "name": "web", "image": "nginx", "securityContext": { "privileged": true } }] }
    })
}

// ---------------------------------------------------------------------------
// apply_manifest: off by default; raw (no workload policy) when enabled
// ---------------------------------------------------------------------------

#[tokio::test]
async fn apply_manifest_is_disabled_by_default() {
    let (client, handle) = strict_client();
    let manifest = json!({ "apiVersion": "v1", "kind": "ConfigMap", "metadata": { "name": "c", "namespace": "default" } });
    let err = rejected_offline(
        client,
        handle,
        "apply_manifest",
        json!({ "manifest": manifest.to_string() }),
    )
    .await;
    assert!(err.contains("apply_manifest is disabled"), "{err}");
    assert!(err.contains("--enable-apply-manifest"), "{err}");
}

fn core_v1_discovery() -> (u16, Value) {
    (
        200,
        json!({
            "kind": "APIResourceList", "apiVersion": "v1", "groupVersion": "v1",
            "resources": [{ "name": "pods", "singularName": "pod", "namespaced": true, "kind": "Pod", "verbs": ["create", "delete", "get", "list", "patch", "update", "watch"] }]
        }),
    )
}

fn rbac_v1_discovery() -> (u16, Value) {
    (
        200,
        json!({
            "kind": "APIResourceList", "apiVersion": "v1", "groupVersion": "rbac.authorization.k8s.io/v1",
            "resources": [{ "name": "clusterrolebindings", "singularName": "clusterrolebinding", "namespaced": false, "kind": "ClusterRoleBinding", "verbs": ["create", "delete", "get", "list", "patch", "update", "watch"] }]
        }),
    )
}

#[tokio::test]
async fn enabled_apply_manifest_skips_the_workload_policy() {
    // The same privileged, hostPath, hostPID pod create_pod refuses goes
    // straight to discovery, the existence check and the apply.
    let (client, handle) = raw_apply_client();
    let manifest = json!({
        "apiVersion": "v1", "kind": "Pod", "metadata": { "name": "p", "namespace": "default" },
        "spec": {
            "hostPID": true,
            "containers": [{ "name": "c", "image": "busybox", "securityContext": { "privileged": true } }],
            "volumes": [{ "name": "root", "hostPath": { "path": "/" } }]
        }
    });
    let server = serve(
        handle,
        vec![core_v1_discovery(), not_found(), (200, manifest.clone())],
    );
    handle_tool(
        &client,
        "apply_manifest",
        &json!({ "manifest": manifest.to_string() }),
    )
    .await
    .unwrap()
    .unwrap();
    let seen = finish(client, server).await;
    assert_eq!(seen[2].method, http::Method::PATCH);
    assert_eq!(
        seen[2].body["spec"]["containers"][0]["securityContext"]["privileged"],
        true
    );
}

#[tokio::test]
async fn enabled_apply_manifest_writes_cluster_scoped_rbac() {
    // No kind gate and no bound-role lookup: a ClusterRoleBinding to
    // cluster-admin is applied as given. (`serve` fails the test on any extra
    // request, such as a GET of the bound role.)
    let (client, handle) = raw_apply_client();
    let crb = json!({
        "apiVersion": "rbac.authorization.k8s.io/v1", "kind": "ClusterRoleBinding", "metadata": { "name": "admins" },
        "roleRef": { "apiGroup": "rbac.authorization.k8s.io", "kind": "ClusterRole", "name": "cluster-admin" },
        "subjects": [{ "kind": "Group", "name": "platform-admins", "apiGroup": "rbac.authorization.k8s.io" }]
    });
    let server = serve(
        handle,
        vec![rbac_v1_discovery(), not_found(), (200, crb.clone())],
    );
    handle_tool(
        &client,
        "apply_manifest",
        &json!({ "manifest": crb.to_string() }),
    )
    .await
    .unwrap()
    .unwrap();
    let seen = finish(client, server).await;
    assert_eq!(
        seen[2].path,
        "/apis/rbac.authorization.k8s.io/v1/clusterrolebindings/admins"
    );
}

#[tokio::test]
async fn enabled_apply_manifest_still_enforces_the_namespace_allowlist() {
    let (client, handle) = client_with(
        ActionPermissions::default().with_apply_manifest(true),
        &["team-a"],
    );
    let manifest = json!({ "apiVersion": "v1", "kind": "ConfigMap", "metadata": { "name": "c", "namespace": "kube-system" } });
    let err = rejected_offline(
        client,
        handle,
        "apply_manifest",
        json!({ "manifest": manifest.to_string() }),
    )
    .await;
    assert!(err.contains("not in the allowed list"), "{err}");
}

#[tokio::test]
async fn apply_manifest_namespace_argument_is_checked_against_the_allowlist() {
    let (client, handle) = client_with(
        ActionPermissions::default().with_apply_manifest(true),
        &["team-a"],
    );
    let manifest = json!({ "apiVersion": "v1", "kind": "ConfigMap", "metadata": { "name": "c" } });
    let err = rejected_offline(
        client,
        handle,
        "apply_manifest",
        json!({ "manifest": manifest.to_string(), "namespace": "kube-system" }),
    )
    .await;
    assert!(err.contains("not in the allowed list"), "{err}");
}

#[tokio::test]
async fn apply_manifest_rejects_a_namespace_argument_that_contradicts_the_manifest() {
    let (client, handle) = raw_apply_client();
    let manifest = json!({ "apiVersion": "v1", "kind": "ConfigMap", "metadata": { "name": "c", "namespace": "other" } });
    let err = rejected_offline(
        client,
        handle,
        "apply_manifest",
        json!({ "manifest": manifest.to_string(), "namespace": "default" }),
    )
    .await;
    assert!(
        err.contains("does not match the namespace argument"),
        "{err}"
    );
}

#[tokio::test]
async fn apply_manifest_fills_metadata_namespace_from_the_argument() {
    let (client, handle) = client_with(
        ActionPermissions::default().with_apply_manifest(true),
        &["team-a"],
    );
    let discovery = (
        200,
        json!({
            "kind": "APIResourceList", "apiVersion": "v1", "groupVersion": "v1",
            "resources": [{ "name": "configmaps", "singularName": "configmap", "namespaced": true, "kind": "ConfigMap", "verbs": ["create", "get", "patch"] }]
        }),
    );
    let manifest = json!({ "apiVersion": "v1", "kind": "ConfigMap", "metadata": { "name": "c" }, "data": { "k": "v" } });
    let applied = json!({ "apiVersion": "v1", "kind": "ConfigMap", "metadata": { "name": "c", "namespace": "team-a" } });
    let server = serve(handle, vec![discovery, not_found(), (200, applied)]);
    handle_tool(
        &client,
        "apply_manifest",
        &json!({ "manifest": manifest.to_string(), "namespace": "team-a" }),
    )
    .await
    .unwrap()
    .unwrap();
    let seen = finish(client, server).await;
    assert_eq!(seen[1].path, "/api/v1/namespaces/team-a/configmaps/c");
    assert_eq!(seen[2].method, http::Method::PATCH);
    assert_eq!(seen[2].path, "/api/v1/namespaces/team-a/configmaps/c");
    assert_eq!(seen[2].body["metadata"]["namespace"], "team-a");
}

// ---------------------------------------------------------------------------
// apply_manifest (enabled): allowed path, create/update switches, managed-by label
// ---------------------------------------------------------------------------

fn apps_v1_discovery() -> (u16, Value) {
    (
        200,
        json!({
            "kind": "APIResourceList", "apiVersion": "v1", "groupVersion": "apps/v1",
            "resources": [{ "name": "deployments", "singularName": "deployment", "namespaced": true, "kind": "Deployment", "verbs": ["create", "delete", "get", "list", "patch", "update", "watch"] }]
        }),
    )
}

fn clean_deployment(rv: &str) -> Value {
    json!({
        "apiVersion": "apps/v1", "kind": "Deployment",
        "metadata": { "name": "web", "namespace": "default", "resourceVersion": rv },
        "spec": {
            "selector": { "matchLabels": { "app": "web" } },
            "template": { "metadata": { "labels": { "app": "web" } }, "spec": { "containers": [{ "name": "web", "image": "nginx" }] } }
        }
    })
}

#[tokio::test]
async fn apply_manifest_applies_a_clean_deployment_with_the_managed_by_label() {
    let (client, handle) = raw_apply_client();
    let manifest = clean_deployment("1");
    let server = serve(
        handle,
        vec![
            apps_v1_discovery(),
            not_found(),
            (200, clean_deployment("2")),
        ],
    );
    let out = handle_tool(
        &client,
        "apply_manifest",
        &json!({ "manifest": manifest.to_string() }),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(out.contains("\"web\""), "{out}");
    let seen = finish(client, server).await;
    assert_eq!(seen[0].path, "/apis/apps/v1");
    assert_eq!(
        (seen[1].method.as_str(), seen[1].path.as_str()),
        ("GET", "/apis/apps/v1/namespaces/default/deployments/web")
    );
    let apply = &seen[2];
    assert_eq!(apply.method, http::Method::PATCH);
    assert!(
        apply.query.contains("fieldManager=mcp-k8s") && apply.query.contains("force=true"),
        "{}",
        apply.query
    );
    assert_eq!(
        apply.body["metadata"]["labels"]["app.kubernetes.io/managed-by"],
        "mcp-k8s"
    );
}

#[tokio::test]
async fn apply_manifest_cannot_update_when_updates_are_disabled() {
    // --disable-update: apply_manifest (a create) is listed, but it must not
    // update an object that already exists.
    let perms = ActionPermissions::new(false, true, false, vec![], true, true);
    let (client, handle) = client_with(perms, &[]);
    let server = serve(
        handle,
        vec![apps_v1_discovery(), (200, clean_deployment("7"))],
    );
    let err = handle_tool(
        &client,
        "apply_manifest",
        &json!({ "manifest": clean_deployment("1").to_string() }),
    )
    .await
    .unwrap()
    .unwrap_err();
    assert!(
        err.contains("would update the existing Deployment 'web'"),
        "{err}"
    );
    finish(client, server).await;
}

#[tokio::test]
async fn apply_manifest_honours_per_resource_create_switches() {
    let perms = ActionPermissions::new(
        false,
        false,
        false,
        vec!["deployment-create".into()],
        true,
        true,
    );
    let (client, handle) = client_with(perms, &[]);
    let server = serve(handle, vec![apps_v1_discovery(), not_found()]);
    let err = handle_tool(
        &client,
        "apply_manifest",
        &json!({ "manifest": clean_deployment("1").to_string() }),
    )
    .await
    .unwrap()
    .unwrap_err();
    assert!(
        err.contains("would create Deployment 'web'") && err.contains("deployment-create"),
        "{err}"
    );
    finish(client, server).await;
}

// ---------------------------------------------------------------------------
// Typed tools: same rules, rejected before any API call
// ---------------------------------------------------------------------------

#[tokio::test]
async fn create_pod_rejects_privileged_and_host_settings() {
    let (client, handle) = strict_client();
    let err = rejected_offline(
        client,
        handle,
        "create_pod",
        json!({
            "namespace": "default", "name": "p", "image": "busybox",
            "host_network": true,
            "security_context": { "privileged": true, "capabilities": { "add": ["SYS_ADMIN"] } },
            "volumes": [{ "name": "root", "host_path": { "path": "/" } }],
            "ports": [{ "container_port": 80, "host_port": 80 }]
        }),
    )
    .await;
    for flag in [
        "--allow-privileged",
        "--allow-capabilities SYS_ADMIN",
        "--allow-host-namespaces",
        "--allow-host-path",
        "--allow-host-ports",
    ] {
        assert!(err.contains(flag), "missing {flag}: {err}");
    }
}

#[tokio::test]
async fn create_deployment_rejects_a_privileged_container() {
    let (client, handle) = strict_client();
    let err = rejected_offline(
        client,
        handle,
        "create_deployment",
        json!({ "namespace": "default", "name": "web", "image": "nginx", "security_context": { "privileged": true } }),
    )
    .await;
    assert!(
        err.contains("--allow-privileged")
            && err.contains("spec.template.spec.containers[0] (web)"),
        "{err}"
    );
}

#[tokio::test]
async fn typed_tools_reject_unknown_arguments() {
    let (client, handle) = strict_client();
    let err = rejected_offline(
        client,
        handle,
        "create_job",
        json!({ "namespace": "default", "name": "j", "image": "busybox", "securityContextt": {} }),
    )
    .await;
    assert!(err.contains("unknown argument 'securityContextt'"), "{err}");
}

#[tokio::test]
async fn create_service_rejects_load_balancers() {
    let (client, handle) = strict_client();
    let err = rejected_offline(
        client,
        handle,
        "create_service",
        json!({ "namespace": "default", "name": "s", "type": "LoadBalancer" }),
    )
    .await;
    assert!(err.contains("--allow-loadbalancer"), "{err}");
}

#[tokio::test]
async fn create_ingress_rejects_snippets_before_creating_backing_services() {
    let (client, handle) = strict_client();
    let err = rejected_offline(
        client,
        handle,
        "create_ingress",
        json!({
            "namespace": "default", "name": "i", "service_name": "web",
            "annotations": { "nginx.ingress.kubernetes.io/configuration-snippet": "return 200;" }
        }),
    )
    .await;
    assert!(err.contains("--allow-ingress-snippets"), "{err}");
}

#[tokio::test]
async fn rbac_tools_reject_escalation_primitives() {
    let (client, handle) = strict_client();
    let err = rejected_offline(
        client,
        handle,
        "create_clusterrole",
        json!({ "name": "god", "rules": [{ "api_groups": ["*"], "resources": ["*"], "verbs": ["*"] }] }),
    )
    .await;
    assert!(err.contains("--allow-rbac-escalation"), "{err}");

    let (client, handle) = strict_client();
    let err = rejected_offline(
        client,
        handle,
        "create_clusterrolebinding",
        json!({ "name": "pwn", "role_ref": { "kind": "ClusterRole", "name": "cluster-admin" }, "subjects": [{ "kind": "User", "name": "me" }] }),
    )
    .await;
    assert!(err.contains("binding cluster-admin"), "{err}");

    let (client, handle) = strict_client();
    let err = rejected_offline(
        client,
        handle,
        "create_secret",
        json!({ "namespace": "default", "name": "t", "type": "kubernetes.io/service-account-token", "string_data": {} }),
    )
    .await;
    assert!(err.contains("service-account-token"), "{err}");
}

#[tokio::test]
async fn bindings_check_the_rules_of_the_bound_role() {
    // edit-like role: impersonate on serviceaccounts.
    let (client, handle) = strict_client();
    let edit = json!({
        "apiVersion": "rbac.authorization.k8s.io/v1", "kind": "ClusterRole", "metadata": { "name": "edit" },
        "rules": [{ "apiGroups": [""], "resources": ["serviceaccounts"], "verbs": ["impersonate"] }]
    });
    let server = serve(handle, vec![(200, edit)]);
    let err = handle_tool(
        &client,
        "create_rolebinding",
        &json!({ "namespace": "default", "name": "b", "role_ref": { "kind": "ClusterRole", "name": "edit" }, "subjects": [{ "kind": "ServiceAccount", "name": "app", "namespace": "default" }] }),
    )
    .await
    .unwrap()
    .unwrap_err();
    assert!(err.contains("bound ClusterRole \"edit\""), "{err}");
    let seen = finish(client, server).await;
    assert_eq!(
        seen[0].path,
        "/apis/rbac.authorization.k8s.io/v1/clusterroles/edit"
    );

    // view-like role: allowed.
    let (client, handle) = strict_client();
    let view = json!({
        "apiVersion": "rbac.authorization.k8s.io/v1", "kind": "ClusterRole", "metadata": { "name": "view" },
        "rules": [{ "apiGroups": [""], "resources": ["pods"], "verbs": ["get", "list", "watch"] }]
    });
    let created = json!({
        "apiVersion": "rbac.authorization.k8s.io/v1", "kind": "RoleBinding", "metadata": { "name": "b", "namespace": "default" },
        "roleRef": { "apiGroup": "rbac.authorization.k8s.io", "kind": "ClusterRole", "name": "view" }
    });
    let server = serve(handle, vec![(200, view), (201, created)]);
    handle_tool(
        &client,
        "create_rolebinding",
        &json!({ "namespace": "default", "name": "b", "role_ref": { "kind": "ClusterRole", "name": "view" }, "subjects": [{ "kind": "ServiceAccount", "name": "app", "namespace": "default" }] }),
    )
    .await
    .unwrap()
    .unwrap();
    let seen = finish(client, server).await;
    assert_eq!(seen[1].method, http::Method::POST);
}

#[tokio::test]
async fn custom_resource_tools_reject_built_in_kinds_and_unlisted_groups() {
    let (client, handle) = strict_client();
    let err = rejected_offline(
        client,
        handle,
        "create_custom_resource",
        json!({ "group": "", "version": "v1", "kind": "Pod", "namespace": "default", "manifest": { "metadata": { "name": "p" }, "spec": { "containers": [{ "name": "c", "image": "x", "securityContext": { "privileged": true } }] } } }),
    )
    .await;
    assert!(err.contains("built-in kind"), "{err}");

    let (client, handle) = strict_client();
    let err = rejected_offline(
        client,
        handle,
        "update_custom_resource",
        json!({ "group": "apps", "version": "v1", "kind": "Deployment", "name": "web", "namespace": "default", "patch": {} }),
    )
    .await;
    assert!(err.contains("built-in kind"), "{err}");

    let (client, handle) = strict_client();
    let err = rejected_offline(
        client,
        handle,
        "create_custom_resource",
        json!({ "group": "argoproj.io", "version": "v1alpha1", "kind": "Workflow", "namespace": "default", "manifest": { "metadata": { "name": "w" }, "spec": {} } }),
    )
    .await;
    assert!(
        err.contains("--allow-custom-resource-groups argoproj.io"),
        "{err}"
    );
}

#[tokio::test]
async fn exec_pod_is_unavailable_in_read_only_mode() {
    let perms = ActionPermissions::new(true, true, true, vec![], true, true);
    let (client, handle) = client_with(perms, &[]);
    let err = rejected_offline(
        client,
        handle,
        "exec_pod",
        json!({ "namespace": "default", "name": "p", "command": ["id"] }),
    )
    .await;
    assert!(err.contains("not allowed"), "{err}");
}

fn running_pod(spec: Value) -> Value {
    json!({ "apiVersion": "v1", "kind": "Pod", "metadata": { "name": "p", "namespace": "default" }, "spec": spec })
}

#[tokio::test]
async fn exec_pod_refuses_a_privileged_pod() {
    let (client, handle) = strict_client();
    let pod = running_pod(json!({
        "containers": [{ "name": "c", "image": "busybox", "securityContext": { "privileged": true } }]
    }));
    let server = serve(handle, vec![(200, pod)]);
    let err = handle_tool(
        &client,
        "exec_pod",
        &json!({ "namespace": "default", "name": "p", "command": ["id"] }),
    )
    .await
    .unwrap()
    .unwrap_err();
    assert!(err.contains("node-level access"), "{err}");
    assert!(err.contains("--allow-privileged"), "{err}");
    let seen = finish(client, server).await;
    assert_eq!(seen.len(), 1, "only the pod lookup, no exec: {seen:?}");
    assert_eq!(seen[0].path, "/api/v1/namespaces/default/pods/p");
}

#[tokio::test]
async fn exec_pod_refuses_host_namespaces_and_host_path_naming_each_flag() {
    let (client, handle) = strict_client();
    let pod = running_pod(json!({
        "hostPID": true,
        "hostNetwork": true,
        "containers": [{ "name": "c", "image": "busybox" }],
        "volumes": [{ "name": "root", "hostPath": { "path": "/" } }]
    }));
    let server = serve(handle, vec![(200, pod)]);
    let err = handle_tool(
        &client,
        "exec_pod",
        &json!({ "namespace": "default", "name": "p", "command": ["id"] }),
    )
    .await
    .unwrap()
    .unwrap_err();
    assert!(err.contains("--allow-host-namespaces"), "{err}");
    assert!(err.contains("--allow-host-path"), "{err}");
    assert_eq!(finish(client, server).await.len(), 1);
}

/// The exec request itself: the mock can't upgrade to a websocket, so a 403
/// ends the call, but reaching it proves the policy let the exec through.
async fn exec_reaches_the_api(client: K8sClient, handle: Handle, pod: Value) {
    let forbidden = (
        403,
        json!({ "kind": "Status", "apiVersion": "v1", "status": "Failure", "reason": "Forbidden", "code": 403, "message": "forbidden" }),
    );
    let server = serve(handle, vec![(200, pod), forbidden]);
    let result = handle_tool(
        &client,
        "exec_pod",
        &json!({ "namespace": "default", "name": "p", "command": ["id"], "timeout_secs": 5 }),
    )
    .await
    .unwrap();
    let err = result.unwrap_err();
    assert!(!err.contains("node-level access"), "{err}");
    let seen = finish(client, server).await;
    assert_eq!(seen[1].path, "/api/v1/namespaces/default/pods/p/exec");
    assert!(seen[1].query.contains("command=id"), "{}", seen[1].query);
}

#[tokio::test]
async fn exec_pod_runs_in_an_ordinary_pod() {
    let (client, handle) = strict_client();
    // Not node-level access: a host port, an NFS volume and an image from an
    // unlisted registry don't block exec (they only block creating the pod).
    let pod = running_pod(json!({
        "containers": [{ "name": "c", "image": "quay.io/x/y", "ports": [{ "containerPort": 80, "hostPort": 80 }] }],
        "volumes": [{ "name": "share", "nfs": { "server": "nfs", "path": "/" } }]
    }));
    exec_reaches_the_api(client, handle, pod).await;
}

#[tokio::test]
async fn exec_pod_into_a_privileged_pod_when_the_operator_allows_it() {
    let mut policy = WorkloadPolicy::default();
    policy.allow_privileged = true;
    let (client, handle) = client_with_policy(policy);
    let pod = running_pod(json!({
        "containers": [{ "name": "c", "image": "busybox", "securityContext": { "privileged": true } }]
    }));
    exec_reaches_the_api(client, handle, pod).await;
}

// ---------------------------------------------------------------------------
// Typed tools: request bodies and backward compatibility
// ---------------------------------------------------------------------------

#[tokio::test]
async fn create_pod_with_the_old_minimal_arguments() {
    let (client, handle) = strict_client();
    let created = json!({ "apiVersion": "v1", "kind": "Pod", "metadata": { "name": "dbg", "namespace": "default" }, "status": { "phase": "Pending" } });
    let server = serve(handle, vec![(201, created)]);
    let out = handle_tool(&client, "create_pod", &json!({ "namespace": "default", "name": "dbg", "image": "busybox", "command": ["sleep", "3600"] }))
        .await
        .unwrap()
        .unwrap();
    assert!(out.contains("Pending"), "{out}");
    let seen = finish(client, server).await;
    let body = &seen[0].body;
    assert_eq!(seen[0].path, "/api/v1/namespaces/default/pods");
    assert_eq!(body["metadata"]["labels"]["app"], "dbg");
    assert_eq!(
        body["metadata"]["labels"]["app.kubernetes.io/managed-by"],
        "mcp-k8s"
    );
    let spec = &body["spec"];
    assert_eq!(spec["restartPolicy"], "Never");
    assert_eq!(spec["containers"][0]["name"], "dbg");
    assert_eq!(spec["containers"][0]["image"], "busybox");
    assert_eq!(spec["containers"][0]["command"], json!(["sleep", "3600"]));
    // Secure defaults.
    assert_eq!(spec["securityContext"]["runAsNonRoot"], true);
    assert_eq!(
        spec["securityContext"]["seccompProfile"]["type"],
        "RuntimeDefault"
    );
    assert_eq!(spec["automountServiceAccountToken"], false);
    assert_eq!(
        spec["containers"][0]["securityContext"]["allowPrivilegeEscalation"],
        false
    );
    assert_eq!(
        spec["containers"][0]["securityContext"]["capabilities"]["drop"],
        json!(["ALL"])
    );
}

#[tokio::test]
async fn create_job_with_the_old_and_new_arguments() {
    let (client, handle) = strict_client();
    let created = json!({ "apiVersion": "batch/v1", "kind": "Job", "metadata": { "name": "j", "namespace": "default" } });
    let server = serve(handle, vec![(201, created)]);
    handle_tool(
        &client,
        "create_job",
        &json!({
            "namespace": "default", "name": "j", "image": "busybox", "command": ["true"],
            "restart_policy": "OnFailure", "backoff_limit": 2,
            "ttl_seconds_after_finished": 60, "active_deadline_seconds": 300, "completions": 3, "parallelism": 3,
            "completion_mode": "Indexed",
            "env": { "MODE": { "config_map_key_ref": { "name": "cfg", "key": "mode" } } },
            "resources": { "limits": { "memory": "64Mi", "ephemeral-storage": "100Mi" } }
        }),
    )
    .await
    .unwrap()
    .unwrap();
    let seen = finish(client, server).await;
    let spec = &seen[0].body["spec"];
    assert_eq!(
        (
            spec["backoffLimit"].clone(),
            spec["ttlSecondsAfterFinished"].clone()
        ),
        (json!(2), json!(60))
    );
    assert_eq!(
        (spec["completions"].clone(), spec["completionMode"].clone()),
        (json!(3), json!("Indexed"))
    );
    let pod = &spec["template"]["spec"];
    assert_eq!(pod["restartPolicy"], "OnFailure");
    assert_eq!(
        pod["containers"][0]["env"][0]["valueFrom"]["configMapKeyRef"]["key"],
        "mode"
    );
    assert_eq!(
        pod["containers"][0]["resources"]["limits"]["ephemeral-storage"],
        "100Mi"
    );
}

#[tokio::test]
async fn create_service_with_the_old_arguments() {
    let (client, handle) = strict_client();
    let created = json!({
        "apiVersion": "v1", "kind": "Service", "metadata": { "name": "web", "namespace": "default" },
        "spec": { "clusterIP": "10.43.0.10", "ports": [{ "port": 8080, "targetPort": 80, "protocol": "TCP" }] }
    });
    let server = serve(handle, vec![(201, created)]);
    let out = handle_tool(
        &client,
        "create_service",
        &json!({ "namespace": "default", "name": "web", "port": 8080, "target_port": 80 }),
    )
    .await
    .unwrap()
    .unwrap();
    let out: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(
        (
            out["port"].clone(),
            out["target_port"].clone(),
            out["cluster_ip"].clone()
        ),
        (json!(8080), json!(80), json!("10.43.0.10"))
    );
    let body = &finish(client, server).await[0].body;
    assert_eq!(body["spec"]["selector"], json!({ "app": "web" }));
    assert!(body["spec"].get("type").is_none(), "ClusterIP by default");
}

#[tokio::test]
async fn create_ingress_with_the_old_arguments_creates_the_backing_service() {
    let (client, handle) = strict_client();
    let svc = json!({ "apiVersion": "v1", "kind": "Service", "metadata": { "name": "web", "namespace": "default" } });
    let ing = json!({ "apiVersion": "networking.k8s.io/v1", "kind": "Ingress", "metadata": { "name": "site", "namespace": "default" } });
    let server = serve(handle, vec![not_found(), (201, svc), (201, ing)]);
    handle_tool(
        &client,
        "create_ingress",
        &json!({ "namespace": "default", "name": "site", "host": "site.example.com", "service_name": "web", "service_port": 8080, "ingress_class": "nginx" }),
    )
    .await
    .unwrap()
    .unwrap();
    let seen = finish(client, server).await;
    assert_eq!(
        (seen[0].method.as_str(), seen[0].path.as_str()),
        ("GET", "/api/v1/namespaces/default/services/web")
    );
    assert_eq!(seen[1].body["spec"]["ports"][0]["port"], 8080);
    let spec = &seen[2].body["spec"];
    assert_eq!(spec["ingressClassName"], "nginx");
    assert_eq!(spec["rules"][0]["host"], "site.example.com");
    assert_eq!(
        spec["rules"][0]["http"]["paths"][0]["backend"]["service"]["port"]["number"],
        8080
    );
}

#[tokio::test]
async fn relaxed_policy_lets_the_typed_tool_express_privileged_workloads() {
    let mut policy = WorkloadPolicy::default();
    policy.allow_privileged = true;
    policy.allow_host_path = true;
    let (client, handle) = client_with_policy(policy);
    let created = json!({ "apiVersion": "apps/v1", "kind": "DaemonSet", "metadata": { "name": "agent", "namespace": "default" } });
    let server = serve(handle, vec![(201, created)]);
    handle_tool(
        &client,
        "create_daemonset",
        &json!({
            "namespace": "default", "name": "agent", "image": "agent:1",
            "security_context": { "privileged": true, "run_as_non_root": false },
            "volumes": [{ "name": "logs", "host_path": { "path": "/var/log" } }],
            "volume_mounts": [{ "name": "logs", "mount_path": "/host/logs", "read_only": true }],
            "update_strategy": { "type": "RollingUpdate", "max_unavailable": "25%" }
        }),
    )
    .await
    .unwrap()
    .unwrap();
    let body = &finish(client, server).await[0].body;
    let c = &body["spec"]["template"]["spec"]["containers"][0];
    assert_eq!(c["securityContext"]["privileged"], true);
    assert!(
        c["securityContext"]
            .get("allowPrivilegeEscalation")
            .is_none(),
        "would be invalid with privileged"
    );
    assert_eq!(
        body["spec"]["updateStrategy"]["rollingUpdate"]["maxUnavailable"],
        "25%"
    );
}

// ---------------------------------------------------------------------------
// Update tools: dry run, policy check, then a guarded write
// ---------------------------------------------------------------------------

#[tokio::test]
async fn update_deployment_dry_runs_then_writes_with_a_resource_version_precondition() {
    let (client, handle) = strict_client();
    let server = serve(
        handle,
        vec![
            (200, clean_deployment("41")),
            (200, clean_deployment("41")),
            (200, clean_deployment("42")),
        ],
    );
    handle_tool(
        &client,
        "update_deployment",
        &json!({ "namespace": "default", "name": "web", "image": "nginx:1.28", "replicas": 3 }),
    )
    .await
    .unwrap()
    .unwrap();
    let seen = finish(client, server).await;
    assert_eq!(seen[0].method, http::Method::GET);
    let (dry, real) = (&seen[1], &seen[2]);
    assert!(dry.query.contains("dryRun=All"), "{}", dry.query);
    assert!(!real.query.contains("dryRun"), "{}", real.query);
    assert_eq!(dry.body["spec"]["replicas"], 3);
    assert_eq!(
        dry.body["spec"]["template"]["spec"]["containers"],
        json!([{ "name": "web", "image": "nginx:1.28" }])
    );
    assert_eq!(real.body["metadata"]["resourceVersion"], "41");
}

#[tokio::test]
async fn update_tools_refuse_to_modify_a_privileged_workload() {
    // Swapping the image of an existing privileged workload is an escalation.
    let (client, handle) = strict_client();
    let mut privileged = clean_deployment("5");
    privileged["spec"]["template"] = privileged_template();
    let server = serve(handle, vec![(200, privileged.clone()), (200, privileged)]);
    let err = handle_tool(
        &client,
        "update_deployment",
        &json!({ "namespace": "default", "name": "web", "image": "attacker/image" }),
    )
    .await
    .unwrap()
    .unwrap_err();
    assert!(err.contains("--allow-privileged"), "{err}");
    // GET + dry run only: the real write never happens.
    assert_eq!(finish(client, server).await.len(), 2);
}

#[tokio::test]
async fn update_service_cannot_switch_to_a_load_balancer() {
    let (client, handle) = strict_client();
    let lb = json!({ "apiVersion": "v1", "kind": "Service", "metadata": { "name": "s", "namespace": "default", "resourceVersion": "3" }, "spec": { "type": "LoadBalancer" } });
    let server = serve(handle, vec![(200, lb)]);
    let err = handle_tool(
        &client,
        "update_service",
        &json!({ "namespace": "default", "name": "s", "type": "LoadBalancer" }),
    )
    .await
    .unwrap()
    .unwrap_err();
    assert!(err.contains("--allow-loadbalancer"), "{err}");
    let seen = finish(client, server).await;
    assert!(seen[0].query.contains("dryRun=All"));
}

// ---------------------------------------------------------------------------
// Per-resource switches reach sub-tools and side effects
// ---------------------------------------------------------------------------

#[tokio::test]
async fn create_job_from_cronjob_obeys_job_create() {
    let perms = ActionPermissions::new(false, false, false, vec!["job-create".into()], true, true);
    let (client, handle) = client_with(perms, &[]);
    let err = rejected_offline(
        client,
        handle,
        "create_job_from_cronjob",
        json!({ "namespace": "default", "cronjob": "nightly" }),
    )
    .await;
    assert!(err.contains("not allowed"), "{err}");
}

#[tokio::test]
async fn create_ingress_backing_services_obey_service_create() {
    let perms = ActionPermissions::new(
        false,
        false,
        false,
        vec!["service-create".into()],
        true,
        true,
    );
    let (client, handle) = client_with(perms, &[]);
    let server = serve(handle, vec![not_found()]);
    let err = handle_tool(
        &client,
        "create_ingress",
        &json!({ "namespace": "default", "name": "i", "service_name": "web" }),
    )
    .await
    .unwrap()
    .unwrap_err();
    assert!(err.contains("service-create"), "{err}");
    // Only the existence check happened: no Service or Ingress was created.
    assert_eq!(finish(client, server).await.len(), 1);
}

#[tokio::test]
async fn create_job_from_cronjob_checks_the_template() {
    let (client, handle) = strict_client();
    let cronjob = json!({
        "apiVersion": "batch/v1", "kind": "CronJob",
        "metadata": { "name": "nightly", "namespace": "default", "uid": "u-1" },
        "spec": { "schedule": "@daily", "jobTemplate": { "spec": { "template": privileged_template() } } }
    });
    let server = serve(handle, vec![(200, cronjob)]);
    let err = handle_tool(
        &client,
        "create_job_from_cronjob",
        &json!({ "namespace": "default", "cronjob": "nightly" }),
    )
    .await
    .unwrap()
    .unwrap_err();
    assert!(err.contains("--allow-privileged"), "{err}");
    assert_eq!(
        finish(client, server).await.len(),
        1,
        "no Job may be created"
    );
}
