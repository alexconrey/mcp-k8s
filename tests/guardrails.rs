//! Guardrail tests through the full MCP tool dispatch path, against a mock
//! Kubernetes API server (`tower_test::mock`).
//!
//! The rejection tests check that a request breaking the workload policy is
//! refused *before* anything reaches the API server, for the typed tools and
//! for `apply_manifest` alike (parity). The acceptance tests check the request
//! bodies the typed tools send, the old minimal arguments, and the dry-run
//! sequence update tools use.

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
// apply_manifest parity: rejected before any API call
// ---------------------------------------------------------------------------

#[tokio::test]
async fn apply_manifest_rejects_a_privileged_pod() {
    let (client, handle) = strict_client();
    let manifest = json!({
        "apiVersion": "v1", "kind": "Pod", "metadata": { "name": "p", "namespace": "default" },
        "spec": { "containers": [{ "name": "c", "image": "busybox", "securityContext": { "privileged": true } }] }
    });
    let err = rejected_offline(
        client,
        handle,
        "apply_manifest",
        json!({ "manifest": manifest.to_string() }),
    )
    .await;
    assert!(err.contains("--allow-privileged"), "{err}");
    assert!(
        err.contains("spec.containers[0] (c).securityContext"),
        "{err}"
    );
}

#[tokio::test]
async fn apply_manifest_rejects_a_privileged_deployment_template() {
    let (client, handle) = strict_client();
    let manifest = json!({
        "apiVersion": "apps/v1", "kind": "Deployment", "metadata": { "name": "web", "namespace": "default" },
        "spec": { "selector": { "matchLabels": { "app": "web" } }, "template": privileged_template() }
    });
    let err = rejected_offline(
        client,
        handle,
        "apply_manifest",
        json!({ "manifest": manifest.to_string() }),
    )
    .await;
    assert!(
        err.contains("--allow-privileged") && err.contains("spec.template.spec.containers[0]"),
        "{err}"
    );
}

#[tokio::test]
async fn apply_manifest_rejects_host_path_in_every_workload_kind() {
    let template = json!({
        "metadata": { "labels": { "app": "x" } },
        "spec": { "containers": [{ "name": "x", "image": "busybox" }], "volumes": [{ "name": "root", "hostPath": { "path": "/" } }] }
    });
    for (api_version, kind, spec) in [
        (
            "apps/v1",
            "DaemonSet",
            json!({ "selector": { "matchLabels": { "app": "x" } }, "template": template }),
        ),
        (
            "apps/v1",
            "StatefulSet",
            json!({ "serviceName": "x", "selector": { "matchLabels": { "app": "x" } }, "template": template }),
        ),
        (
            "apps/v1",
            "ReplicaSet",
            json!({ "selector": { "matchLabels": { "app": "x" } }, "template": template }),
        ),
        ("batch/v1", "Job", json!({ "template": template })),
        (
            "batch/v1",
            "CronJob",
            json!({ "schedule": "@daily", "jobTemplate": { "spec": { "template": template } } }),
        ),
    ] {
        let (client, handle) = strict_client();
        let manifest = json!({ "apiVersion": api_version, "kind": kind, "metadata": { "name": "x", "namespace": "default" }, "spec": spec });
        let err = rejected_offline(
            client,
            handle,
            "apply_manifest",
            json!({ "manifest": manifest.to_string() }),
        )
        .await;
        assert!(err.contains("--allow-host-path"), "{kind}: {err}");
    }
}

#[tokio::test]
async fn apply_manifest_rejects_snippet_annotations() {
    let (client, handle) = strict_client();
    let manifest = json!({
        "apiVersion": "networking.k8s.io/v1", "kind": "Ingress",
        "metadata": { "name": "i", "namespace": "default", "annotations": { "nginx.ingress.kubernetes.io/server-snippet": "location /x { return 200; }" } },
        "spec": { "rules": [{ "http": { "paths": [{ "path": "/", "pathType": "Prefix", "backend": { "service": { "name": "s", "port": { "number": 80 } } } }] } }] }
    });
    let err = rejected_offline(
        client,
        handle,
        "apply_manifest",
        json!({ "manifest": manifest.to_string() }),
    )
    .await;
    assert!(err.contains("--allow-ingress-snippets"), "{err}");
}

#[tokio::test]
async fn apply_manifest_rejects_rbac_and_cluster_scoped_kinds() {
    for (api_version, kind, namespaced) in [
        ("rbac.authorization.k8s.io/v1", "ClusterRole", false),
        ("rbac.authorization.k8s.io/v1", "ClusterRoleBinding", false),
        ("rbac.authorization.k8s.io/v1", "Role", true),
        ("rbac.authorization.k8s.io/v1", "RoleBinding", true),
        (
            "admissionregistration.k8s.io/v1",
            "MutatingWebhookConfiguration",
            false,
        ),
        ("apiextensions.k8s.io/v1", "CustomResourceDefinition", false),
        ("v1", "Namespace", false),
        ("v1", "Endpoints", true),
    ] {
        let (client, handle) = strict_client();
        let mut manifest =
            json!({ "apiVersion": api_version, "kind": kind, "metadata": { "name": "x" } });
        if namespaced {
            manifest["metadata"]["namespace"] = json!("default");
        }
        let err = rejected_offline(
            client,
            handle,
            "apply_manifest",
            json!({ "manifest": manifest.to_string() }),
        )
        .await;
        assert!(err.contains("--allow-manifest-kinds"), "{kind}: {err}");
    }
}

#[tokio::test]
async fn apply_manifest_rejects_custom_resources_unless_their_group_is_allowlisted() {
    let (client, handle) = strict_client();
    let manifest = json!({
        "apiVersion": "helm.cattle.io/v1", "kind": "HelmChart",
        "metadata": { "name": "x", "namespace": "kube-system" },
        "spec": { "chart": "anything", "repo": "https://example.com" }
    });
    let err = rejected_offline(
        client,
        handle,
        "apply_manifest",
        json!({ "manifest": manifest.to_string() }),
    )
    .await;
    assert!(
        err.contains("--allow-custom-resource-groups helm.cattle.io"),
        "{err}"
    );
}

#[tokio::test]
async fn apply_manifest_enforces_the_namespace_allowlist() {
    let (client, handle) = client_with(ActionPermissions::default(), &["team-a"]);
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

// ---------------------------------------------------------------------------
// apply_manifest: allowed path, permission parity, managed-by label
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
    let (client, handle) = strict_client();
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
async fn create_deployment_rejects_what_apply_manifest_rejects() {
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
