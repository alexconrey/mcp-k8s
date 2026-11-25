//! k3s integration tests for the typed workload tools and the guardrail
//! policy, against a real API server.
//!
//! Ignored by default (they need Docker). Run only this file:
//!
//! ```sh
//! cargo test --test k3s_workloads -- --ignored --test-threads=1
//! ```
//!
//! Never run `cargo test -- --ignored` without `--test`: other ignored tests
//! use the local kubeconfig.
//!
//! The k3s image defaults to [`DEFAULT_K3S_TAG`]; set `MCP_K8S_K3S_TAG` to test
//! another release (Kubernetes 1.33 or newer), e.g. `v1.33.6-k3s1`. Pods use
//! small public images (busybox, pause) pulled by k3s itself.

use std::time::Duration;

use mcp_k8s::mcp::handle_tool;
use mcp_k8s::permissions::ActionPermissions;
use mcp_k8s::policy::WorkloadPolicy;
use mcp_k8s::K8sClient;
use serde_json::{json, Value};
use testcontainers::runners::AsyncRunner;
use testcontainers::{ContainerAsync, ImageExt};
use testcontainers_modules::k3s::{K3s, KUBE_SECURE_PORT};

const DEFAULT_K3S_TAG: &str = "v1.35.10-k3s1";
const BUSYBOX: &str = "busybox:1.36";
const PAUSE: &str = "registry.k8s.io/pause:3.10";

fn k3s_tag() -> String {
    std::env::var("MCP_K8S_K3S_TAG").unwrap_or_else(|_| DEFAULT_K3S_TAG.to_string())
}

async fn start_k3s() -> (kube::Client, ContainerAsync<K3s>) {
    // Docker occasionally reports a fresh container without its port mapping;
    // try a few times before giving up.
    let mut last_err = String::new();
    for _ in 0..3 {
        match try_start_k3s().await {
            Ok(started) => return started,
            Err(e) => last_err = e,
        }
    }
    panic!("failed to start k3s: {last_err}");
}

async fn try_start_k3s() -> Result<(kube::Client, ContainerAsync<K3s>), String> {
    let conf_dir = tempfile::tempdir().map_err(|e| e.to_string())?;
    let container = K3s::default()
        .with_conf_mount(conf_dir.path())
        .with_tag(k3s_tag())
        .with_privileged(true)
        .with_userns_mode("host")
        .start()
        .await
        .map_err(|e| format!("start: {e}"))?;
    let kube_conf = container
        .image()
        .read_kube_config()
        .map_err(|e| format!("kubeconfig: {e}"))?;
    let port = container
        .get_host_port_ipv4(KUBE_SECURE_PORT)
        .await
        .map_err(|e| format!("port: {e}"))?;
    let kube_conf = kube_conf.replace(
        "https://127.0.0.1:6443",
        &format!("https://127.0.0.1:{port}"),
    );
    let _ = rustls::crypto::ring::default_provider().install_default();
    let kubeconfig = kube::config::Kubeconfig::from_yaml(&kube_conf).map_err(|e| e.to_string())?;
    let config = kube::Config::from_custom_kubeconfig(
        kubeconfig,
        &kube::config::KubeConfigOptions::default(),
    )
    .await
    .map_err(|e| e.to_string())?;
    let client = kube::Client::try_from(config).map_err(|e| e.to_string())?;
    Ok((client, container))
}

async fn call(client: &K8sClient, tool: &str, args: Value) -> Result<String, String> {
    handle_tool(client, tool, &args)
        .await
        .unwrap_or_else(|| panic!("{tool} should be a known tool"))
}

async fn ok(client: &K8sClient, tool: &str, args: Value) -> Value {
    let text = call(client, tool, args)
        .await
        .unwrap_or_else(|e| panic!("{tool} failed: {e}"));
    serde_json::from_str(&text).unwrap_or(Value::String(text))
}

async fn get_json(
    client: &K8sClient,
    api_version: &str,
    kind: &str,
    ns: &str,
    name: &str,
) -> Value {
    ok(
        client,
        "get_resource_json",
        json!({ "api_version": api_version, "kind": kind, "name": name, "namespace": ns }),
    )
    .await
}

/// Poll `tool` until `done` accepts its output (up to `secs`).
async fn wait_for(
    client: &K8sClient,
    tool: &str,
    args: Value,
    secs: u64,
    done: impl Fn(&Value) -> bool,
) -> Value {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(secs);
    loop {
        let out = ok(client, tool, args.clone()).await;
        if done(&out) || tokio::time::Instant::now() >= deadline {
            return out;
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

/// The default service account must exist before pods can be created.
async fn wait_for_default_sa(client: &K8sClient, ns: &str) {
    let out = wait_for(
        client,
        "list_serviceaccounts",
        json!({ "namespace": ns }),
        90,
        |v| v.to_string().contains("\"default\""),
    )
    .await;
    assert!(
        out.to_string().contains("\"default\""),
        "no default service account in {ns}"
    );
}

#[tokio::test]
#[ignore] // requires Docker
async fn k3s_typed_workload_tools() {
    let (kube_client, _container) = start_k3s().await;
    let client = K8sClient::new(kube_client, vec![], ActionPermissions::default());
    let ns = "workloads";
    ok(&client, "create_namespace", json!({ "name": ns })).await;
    wait_for_default_sa(&client, ns).await;

    // Inputs the pods consume.
    ok(&client, "create_configmap", json!({ "namespace": ns, "name": "settings", "data": { "MODE": "test", "app.conf": "x=1" } })).await;
    ok(
        &client,
        "create_secret",
        json!({ "namespace": ns, "name": "creds", "string_data": { "password": "s3cret" } }),
    )
    .await;

    // Deployment: env from secret/configmap/fields, env_from, probes,
    // resources, volumes of every safe type, an init container and a native
    // sidecar; secure defaults; pods run as nobody.
    ok(
        &client,
        "create_deployment",
        json!({
            "namespace": ns, "name": "web", "replicas": 2,
            "image": BUSYBOX, "command": ["sh", "-c", "touch /tmp/ready; exec sleep 3600"],
            "env": {
                "PASSWORD": { "secret_key_ref": { "name": "creds", "key": "password" } },
                "MODE": { "config_map_key_ref": { "name": "settings", "key": "MODE" } },
                "POD_NAME": { "field_ref": { "field_path": "metadata.name" } },
                "MEM_LIMIT": { "resource_field_ref": { "resource": "limits.memory" } },
                "PLAIN": "value"
            },
            "env_from": [{ "config_map_ref": "settings", "prefix": "CFG_" }],
            "resources": { "requests": { "cpu": "10m", "memory": "16Mi", "ephemeral-storage": "10Mi" }, "limits": { "memory": "64Mi", "ephemeral-storage": "100Mi" } },
            "readiness_probe": { "probe_type": "exec", "command": ["cat", "/tmp/ready"], "period_seconds": 2 },
            "liveness_probe": { "probe_type": "exec", "command": ["true"], "period_seconds": 10 },
            "lifecycle": { "pre_stop": { "handler_type": "sleep", "seconds": 1 } },
            "volume_mounts": [
                { "name": "config", "mount_path": "/etc/app", "read_only": true },
                { "name": "creds", "mount_path": "/etc/creds", "read_only": true },
                { "name": "scratch", "mount_path": "/scratch" },
                { "name": "token", "mount_path": "/var/run/token", "read_only": true },
                { "name": "info", "mount_path": "/etc/info", "read_only": true }
            ],
            "volumes": [
                { "name": "config", "config_map": { "name": "settings" } },
                { "name": "creds", "secret": { "secret_name": "creds" } },
                { "name": "scratch", "empty_dir": { "medium": "Memory", "size_limit": "16Mi" } },
                { "name": "token", "projected": { "sources": [{ "service_account_token": { "path": "token", "audience": "test", "expiration_seconds": 3600 } }] } },
                { "name": "info", "downward_api": { "items": [{ "path": "labels", "field_ref": { "field_path": "metadata.labels" } }] } }
            ],
            "init_containers": [
                { "name": "prepare", "image": BUSYBOX, "command": ["sh", "-c", "echo ok"] },
                { "name": "sidecar", "image": PAUSE, "restart_policy": "Always" }
            ],
            "security_context": { "run_as_user": 65534, "run_as_group": 65534, "fs_group": 65534 },
            "strategy": { "type": "RollingUpdate", "max_surge": 1, "max_unavailable": 0 },
            "revision_history_limit": 3,
            "pod_annotations": { "example.com/scrape": "true" },
            "labels": { "tier": "frontend" }
        }),
    )
    .await;
    let dep = wait_for(
        &client,
        "get_deployment",
        json!({ "namespace": ns, "name": "web" }),
        240,
        |v| v["detail"]["replicas"]["ready"] == json!(2),
    )
    .await;
    let raw = get_json(&client, "apps/v1", "Deployment", ns, "web").await;
    let pod_spec = &raw["spec"]["template"]["spec"];
    assert_eq!(
        pod_spec["securityContext"]["runAsNonRoot"], true,
        "{pod_spec}"
    );
    assert_eq!(
        pod_spec["securityContext"]["seccompProfile"]["type"],
        "RuntimeDefault"
    );
    assert_eq!(pod_spec["automountServiceAccountToken"], false);
    assert_eq!(
        pod_spec["containers"][0]["securityContext"]["capabilities"]["drop"],
        json!(["ALL"])
    );
    assert_eq!(pod_spec["initContainers"][1]["restartPolicy"], "Always");
    assert_eq!(
        raw["metadata"]["labels"]["app.kubernetes.io/managed-by"],
        "mcp-k8s"
    );
    let ready = raw["status"]["readyReplicas"].as_i64().unwrap_or(0);
    assert_eq!(ready, 2, "deployment never became ready: {dep}\n{raw}");

    // Update through the dry-run path: image, env, probe and replicas.
    ok(
        &client,
        "update_deployment",
        json!({ "namespace": ns, "name": "web", "replicas": 1, "env": { "PLAIN": "changed" }, "startup_probe": { "probe_type": "exec", "command": ["true"] } }),
    )
    .await;
    let raw = get_json(&client, "apps/v1", "Deployment", ns, "web").await;
    assert_eq!(raw["spec"]["replicas"], 1);
    let env = raw["spec"]["template"]["spec"]["containers"][0]["env"]
        .as_array()
        .unwrap()
        .clone();
    assert!(
        env.iter()
            .any(|e| e["name"] == "PLAIN" && e["value"] == "changed"),
        "{env:?}"
    );
    assert!(
        env.iter().any(|e| e["name"] == "PASSWORD"),
        "env merges by name: {env:?}"
    );
    ok(&client, "update_deployment_affinity", json!({
        "namespace": ns, "name": "web",
        "required_node_affinity": [{ "match_expressions": [{ "key": "kubernetes.io/os", "operator": "In", "values": ["linux"] }] }]
    }))
    .await;
    let raw = get_json(&client, "apps/v1", "Deployment", ns, "web").await;
    let terms = &raw["spec"]["template"]["spec"]["affinity"]["nodeAffinity"]
        ["requiredDuringSchedulingIgnoredDuringExecution"]["nodeSelectorTerms"];
    assert_eq!(
        terms[0]["matchExpressions"][0]["key"], "kubernetes.io/os",
        "affinity terms must reach the API: {terms}"
    );

    // Job: indexed, TTL, deadline; it completes.
    ok(
        &client,
        "create_job",
        json!({
            "namespace": ns, "name": "batch", "image": BUSYBOX, "command": ["sh", "-c", "echo $JOB_COMPLETION_INDEX"],
            "completions": 2, "parallelism": 2, "completion_mode": "Indexed",
            "backoff_limit": 1, "ttl_seconds_after_finished": 600, "active_deadline_seconds": 300,
            "security_context": { "run_as_user": 65534 }
        }),
    )
    .await;
    let job = wait_for(
        &client,
        "get_job",
        json!({ "namespace": ns, "name": "batch" }),
        180,
        |v| v["status"] == "Succeeded" || v["status"] == "Failed",
    )
    .await;
    assert_eq!(job["status"], "Succeeded", "{job}");

    // CronJob, then run it now.
    ok(
        &client,
        "create_cronjob",
        json!({
            "namespace": ns, "name": "nightly", "schedule": "0 3 * * *", "time_zone": "Etc/UTC",
            "concurrency_policy": "Forbid", "suspend": true, "successful_jobs_history_limit": 1, "failed_jobs_history_limit": 1,
            "starting_deadline_seconds": 600, "backoff_limit": 0,
            "image": BUSYBOX, "command": ["true"], "security_context": { "run_as_user": 65534 }
        }),
    )
    .await;
    let raw = get_json(&client, "batch/v1", "CronJob", ns, "nightly").await;
    assert_eq!(raw["spec"]["suspend"], true);
    assert!(
        raw["spec"]["jobTemplate"]["spec"].get("suspend").is_none(),
        "suspend belongs to the CronJob"
    );
    ok(&client, "update_cronjob", json!({ "namespace": ns, "name": "nightly", "schedule": "0 4 * * *", "image": "busybox:1.37" })).await;
    let raw = get_json(&client, "batch/v1", "CronJob", ns, "nightly").await;
    assert_eq!(raw["spec"]["schedule"], "0 4 * * *");
    assert_eq!(
        raw["spec"]["jobTemplate"]["spec"]["template"]["spec"]["containers"][0]["image"],
        "busybox:1.37"
    );
    let manual = ok(
        &client,
        "create_job_from_cronjob",
        json!({ "namespace": ns, "cronjob": "nightly", "name": "nightly-now" }),
    )
    .await;
    assert_eq!(manual["name"], "nightly-now");
    let raw = get_json(&client, "batch/v1", "Job", ns, "nightly-now").await;
    assert_eq!(raw["metadata"]["ownerReferences"][0]["kind"], "CronJob");
    assert_eq!(
        raw["metadata"]["annotations"]["cronjob.kubernetes.io/instantiate"],
        "manual"
    );

    // StatefulSet with a headless Service and per-pod volumes (local-path).
    ok(&client, "create_service", json!({ "namespace": ns, "name": "db", "headless": true, "ports": [{ "port": 5432, "name": "pg" }] })).await;
    ok(
        &client,
        "create_statefulset",
        json!({
            "namespace": ns, "name": "db", "service_name": "db", "replicas": 1,
            "image": PAUSE, "pod_management_policy": "Parallel",
            "volume_claim_templates": [{ "name": "data", "storage": "16Mi" }],
            "volume_mounts": [{ "name": "data", "mount_path": "/data" }],
            "update_strategy": { "type": "RollingUpdate", "partition": 0 },
            "persistent_volume_claim_retention_policy": { "when_deleted": "Delete", "when_scaled": "Retain" }
        }),
    )
    .await;
    let raw = get_json(&client, "v1", "Service", ns, "db").await;
    assert_eq!(raw["spec"]["clusterIP"], "None");
    let raw = get_json(&client, "apps/v1", "StatefulSet", ns, "db").await;
    assert_eq!(
        raw["spec"]["volumeClaimTemplates"][0]["spec"]["resources"]["requests"]["storage"],
        "16Mi"
    );
    ok(
        &client,
        "update_statefulset",
        json!({ "namespace": ns, "name": "db", "replicas": 2 }),
    )
    .await;

    // DaemonSet with an update strategy.
    ok(
        &client,
        "create_daemonset",
        json!({ "namespace": ns, "name": "agent", "image": PAUSE, "update_strategy": { "type": "RollingUpdate", "max_unavailable": 1 }, "tolerations": [{ "operator": "Exists" }] }),
    )
    .await;
    ok(
        &client,
        "update_daemonset",
        json!({ "namespace": ns, "name": "agent", "update_strategy": { "type": "OnDelete" } }),
    )
    .await;
    let raw = get_json(&client, "apps/v1", "DaemonSet", ns, "agent").await;
    assert_eq!(raw["spec"]["updateStrategy"]["type"], "OnDelete");
    assert!(raw["spec"]["updateStrategy"].get("rollingUpdate").is_none());

    // Pod with the old minimal arguments.
    ok(
        &client,
        "create_pod",
        json!({ "namespace": ns, "name": "debug", "image": PAUSE }),
    )
    .await;

    // Service + Ingress with several rules and TLS.
    ok(
        &client,
        "create_service",
        json!({ "namespace": ns, "name": "web", "ports": [{ "port": 80, "target_port": 8080, "name": "http" }, { "port": 9090, "name": "metrics" }], "session_affinity": "ClientIP" }),
    )
    .await;
    ok(
        &client,
        "create_ingress",
        json!({
            "namespace": ns, "name": "site", "ingress_class": "traefik",
            "rules": [
                { "host": "a.example.com", "paths": [{ "path": "/", "service_name": "web", "service_port_name": "http" }] },
                { "host": "b.example.com", "paths": [{ "path": "/metrics", "path_type": "Exact", "service_name": "web", "service_port": 9090 }] }
            ],
            "tls": [{ "hosts": ["a.example.com"], "secret_name": "site-tls" }],
            "annotations": { "traefik.ingress.kubernetes.io/router.entrypoints": "websecure" }
        }),
    )
    .await;
    ok(&client, "update_ingress", json!({ "namespace": ns, "name": "site", "annotations": { "example.com/owner": "team-a" } })).await;
    let raw = get_json(&client, "networking.k8s.io/v1", "Ingress", ns, "site").await;
    assert_eq!(raw["spec"]["rules"].as_array().unwrap().len(), 2);
    assert_eq!(
        raw["metadata"]["annotations"]["example.com/owner"],
        "team-a"
    );
    ok(
        &client,
        "update_service",
        json!({ "namespace": ns, "name": "web", "session_affinity": "None" }),
    )
    .await;
}

#[tokio::test]
#[ignore] // requires Docker
async fn k3s_guardrails() {
    let (kube_client, _container) = start_k3s().await;
    let client = K8sClient::new(kube_client.clone(), vec![], ActionPermissions::default());
    let admin = K8sClient::new(
        kube_client.clone(),
        vec![],
        ActionPermissions::default().with_workload_policy(WorkloadPolicy::permissive()),
    );
    let ns = "guarded";
    ok(&client, "create_namespace", json!({ "name": ns })).await;
    wait_for_default_sa(&client, ns).await;

    // The typed tool refuses a privileged pod.
    let err = call(&client, "create_pod", json!({ "namespace": ns, "name": "evil", "image": BUSYBOX, "security_context": { "privileged": true } }))
        .await
        .unwrap_err();
    assert!(err.contains("--allow-privileged"), "{err}");

    // apply_manifest is off by default...
    let cm = json!({ "apiVersion": "v1", "kind": "ConfigMap", "metadata": { "name": "applied", "namespace": ns }, "data": { "a": "1" } });
    let err = call(
        &client,
        "apply_manifest",
        json!({ "manifest": cm.to_string() }),
    )
    .await
    .unwrap_err();
    assert!(err.contains("--enable-apply-manifest"), "{err}");

    // ... and when the operator enables it, it's a raw server-side apply that
    // skips the workload policy.
    let raw = K8sClient::new(
        kube_client.clone(),
        vec![],
        ActionPermissions::default().with_apply_manifest(true),
    );
    let privileged = json!({
        "apiVersion": "v1", "kind": "Pod", "metadata": { "name": "privileged", "namespace": ns },
        "spec": { "containers": [{ "name": "c", "image": BUSYBOX, "command": ["sleep", "3600"], "securityContext": { "privileged": true } }] }
    });
    ok(
        &raw,
        "apply_manifest",
        json!({ "manifest": privileged.to_string() }),
    )
    .await;
    let pod = get_json(&client, "v1", "Pod", ns, "privileged").await;
    assert_eq!(
        pod["spec"]["containers"][0]["securityContext"]["privileged"],
        true
    );

    // A clean manifest applies too (discovery, existence check, SSA).
    ok(
        &raw,
        "apply_manifest",
        json!({ "manifest": cm.to_string() }),
    )
    .await;
    let applied = get_json(&client, "v1", "ConfigMap", ns, "applied").await;
    assert_eq!(
        applied["metadata"]["labels"]["app.kubernetes.io/managed-by"],
        "mcp-k8s"
    );
    // The operator's switches still hold: no update when updates are disabled.
    let no_update = K8sClient::new(
        kube_client.clone(),
        vec![],
        ActionPermissions::new(false, true, false, vec![], true, true),
    );
    let err = call(
        &no_update,
        "apply_manifest",
        json!({ "manifest": cm.to_string() }),
    )
    .await
    .unwrap_err();
    assert!(err.contains("would update the existing ConfigMap"), "{err}");

    // Raw manifests aren't limited to namespaced kinds or by the RBAC rules:
    // a ClusterRoleBinding to cluster-admin applies as given.
    let crb = json!({
        "apiVersion": "rbac.authorization.k8s.io/v1", "kind": "ClusterRoleBinding", "metadata": { "name": "raw-admins" },
        "roleRef": { "apiGroup": "rbac.authorization.k8s.io", "kind": "ClusterRole", "name": "cluster-admin" },
        "subjects": [{ "kind": "Group", "name": "platform-admins", "apiGroup": "rbac.authorization.k8s.io" }]
    });
    ok(
        &raw,
        "apply_manifest",
        json!({ "manifest": crb.to_string() }),
    )
    .await;

    // Typed RBAC tools: the bound role's rules count. edit can impersonate
    // service accounts; view can't.
    let edit = call(&client, "create_rolebinding", json!({ "namespace": ns, "name": "editors", "role_ref": { "kind": "ClusterRole", "name": "edit" }, "subjects": [{ "kind": "ServiceAccount", "name": "default", "namespace": ns }] }))
        .await
        .unwrap_err();
    assert!(edit.contains("impersonate"), "{edit}");
    ok(&client, "create_rolebinding", json!({ "namespace": ns, "name": "viewers", "role_ref": { "kind": "ClusterRole", "name": "view" }, "subjects": [{ "kind": "ServiceAccount", "name": "default", "namespace": ns }] })).await;

    // Custom resources: k3s ships helm.cattle.io, whose HelmChart runs any chart
    // as cluster-admin. Custom resource groups aren't allowlisted by default.
    let crds = wait_for(&client, "list_crds", json!({}), 120, |v| {
        v.to_string().contains("helmchartconfigs.helm.cattle.io")
    })
    .await;
    assert!(
        crds.to_string().contains("helmchartconfigs.helm.cattle.io"),
        "{crds}"
    );
    let chart = json!({ "metadata": { "name": "x" }, "spec": { "chart": "nginx", "repo": "https://charts.example.com" } });
    let err = call(&client, "create_custom_resource", json!({ "group": "helm.cattle.io", "version": "v1", "kind": "HelmChart", "namespace": ns, "manifest": chart }))
        .await
        .unwrap_err();
    assert!(
        err.contains("--allow-custom-resource-groups helm.cattle.io"),
        "{err}"
    );
    // With the group allowlisted, writes work (HelmChartConfig only overlays
    // values onto a kube-system HelmChart of the same name: harmless here).
    let mut allow_helm = WorkloadPolicy::default();
    allow_helm.allowed_custom_resource_groups = vec!["helm.cattle.io".into()];
    let helm_client = K8sClient::new(
        kube_client.clone(),
        vec![],
        ActionPermissions::default().with_workload_policy(allow_helm),
    );
    let config =
        json!({ "metadata": { "name": "demo" }, "spec": { "valuesContent": "replicas: 1" } });
    ok(&helm_client, "create_custom_resource", json!({ "group": "helm.cattle.io", "version": "v1", "kind": "HelmChartConfig", "namespace": ns, "manifest": config })).await;
    ok(&helm_client, "update_custom_resource", json!({ "group": "helm.cattle.io", "version": "v1", "kind": "HelmChartConfig", "namespace": ns, "name": "demo", "patch": { "spec": { "valuesContent": "replicas: 2" } } })).await;
    let raw = get_json(&client, "helm.cattle.io/v1", "HelmChartConfig", ns, "demo").await;
    assert_eq!(raw["spec"]["valuesContent"], "replicas: 2");
    assert_eq!(
        raw["metadata"]["labels"]["app.kubernetes.io/managed-by"],
        "mcp-k8s"
    );
    // Built-in kinds can't go through the custom resource tools.
    let err = call(&helm_client, "create_custom_resource", json!({ "group": "", "version": "v1", "kind": "Pod", "namespace": ns, "manifest": { "metadata": { "name": "p" }, "spec": { "containers": [{ "name": "c", "image": BUSYBOX }] } } }))
        .await
        .unwrap_err();
    assert!(err.contains("built-in kind"), "{err}");

    // An existing privileged DaemonSet (created with a permissive policy) can't
    // be modified through the strict client: that would be an image swap on a
    // privileged workload.
    ok(&admin, "create_daemonset", json!({ "namespace": ns, "name": "node-agent", "image": PAUSE, "security_context": { "privileged": true, "run_as_non_root": false }, "volumes": [{ "name": "host", "host_path": { "path": "/var/log" } }], "volume_mounts": [{ "name": "host", "mount_path": "/host" }] })).await;
    let err = call(
        &client,
        "update_daemonset",
        json!({ "namespace": ns, "name": "node-agent", "image": "registry.k8s.io/pause:3.9" }),
    )
    .await
    .unwrap_err();
    assert!(
        err.contains("--allow-privileged") && err.contains("--allow-host-path"),
        "{err}"
    );
    let raw = get_json(&client, "apps/v1", "DaemonSet", ns, "node-agent").await;
    assert_eq!(
        raw["spec"]["template"]["spec"]["containers"][0]["image"], PAUSE,
        "the real write must not happen"
    );

    // Service and Ingress rules against the real API.
    let err = call(
        &client,
        "create_service",
        json!({ "namespace": ns, "name": "lb", "type": "LoadBalancer" }),
    )
    .await
    .unwrap_err();
    assert!(err.contains("--allow-loadbalancer"), "{err}");
    ok(
        &client,
        "create_service",
        json!({ "namespace": ns, "name": "plain" }),
    )
    .await;
    let err = call(
        &client,
        "update_service",
        json!({ "namespace": ns, "name": "plain", "type": "NodePort" }),
    )
    .await
    .unwrap_err();
    assert!(err.contains("--allow-nodeport"), "{err}");
    let raw = get_json(&client, "v1", "Service", ns, "plain").await;
    assert_eq!(
        raw["spec"]["type"], "ClusterIP",
        "the dry run must not have changed the Service"
    );
    let err = call(&client, "create_ingress", json!({ "namespace": ns, "name": "i", "service_name": "plain", "annotations": { "nginx.ingress.kubernetes.io/auth-url": "http://x/#;\nload_module /tmp/x.so;" } })).await.unwrap_err();
    assert!(err.contains("--allow-ingress-snippets"), "{err}");

    // Pod security labels on namespaces.
    let err = call(
        &client,
        "update_namespace",
        json!({ "name": ns, "labels": { "pod-security.kubernetes.io/enforce": "privileged" } }),
    )
    .await
    .unwrap_err();
    assert!(err.contains("--allow-privileged"), "{err}");
    ok(
        &client,
        "update_namespace",
        json!({ "name": ns, "labels": { "pod-security.kubernetes.io/enforce": "baseline" } }),
    )
    .await;
}

#[tokio::test]
#[ignore] // requires Docker
async fn k3s_typed_resource_tools() {
    let (kube_client, _container) = start_k3s().await;
    let client = K8sClient::new(kube_client, vec![], ActionPermissions::default());
    let ns = "resources";
    ok(&client, "create_namespace", json!({ "name": ns })).await;
    wait_for_default_sa(&client, ns).await;

    ok(&client, "create_configmap", json!({ "namespace": ns, "name": "cfg", "data": { "a": "1" }, "binary_data": { "b.bin": "AAEC" }, "immutable": true, "labels": { "team": "web" } })).await;
    let raw = get_json(&client, "v1", "ConfigMap", ns, "cfg").await;
    assert_eq!(
        (raw["immutable"].clone(), raw["binaryData"]["b.bin"].clone()),
        (json!(true), json!("AAEC"))
    );

    ok(&client, "create_secret", json!({ "namespace": ns, "name": "s", "data": { "token": "c2VjcmV0" }, "immutable": true, "annotations": { "owner": "me" } })).await;
    let raw = get_json(&client, "v1", "Secret", ns, "s").await;
    assert_eq!(raw["immutable"], true);
    assert_eq!(raw["metadata"]["annotations"]["owner"], "me");
    // Secret values are opt-in: redacted by default, refused through get_secret.
    assert!(
        raw["data"]["token"].as_str().unwrap().contains("redacted"),
        "{raw}"
    );
    let err = call(
        &client,
        "get_secret",
        json!({ "namespace": ns, "name": "s", "decode": true }),
    )
    .await
    .unwrap_err();
    assert!(err.contains("--enable-secret-decode"), "{err}");
    let decoding = K8sClient::new(
        client.inner().clone(),
        vec![],
        ActionPermissions::default().with_secret_decode(true),
    );
    let raw = get_json(&decoding, "v1", "Secret", ns, "s").await;
    assert_eq!(raw["data"]["token"], "c2VjcmV0");

    ok(&client, "create_serviceaccount", json!({ "namespace": ns, "name": "builder", "image_pull_secrets": ["regcred"], "labels": { "team": "ci" } })).await;
    let raw = get_json(&client, "v1", "ServiceAccount", ns, "builder").await;
    assert_eq!(raw["imagePullSecrets"][0]["name"], "regcred");

    ok(&client, "create_pvc", json!({ "namespace": ns, "name": "data", "storage": "16Mi", "volume_mode": "Filesystem", "labels": { "app": "db" } })).await;
    let raw = get_json(&client, "v1", "PersistentVolumeClaim", ns, "data").await;
    assert_eq!(raw["spec"]["volumeMode"], "Filesystem");

    ok(
        &client,
        "create_deployment",
        json!({ "namespace": ns, "name": "web", "image": PAUSE }),
    )
    .await;
    ok(
        &client,
        "create_hpa",
        json!({
            "namespace": ns, "name": "web",
            "scale_target_kind": "Deployment", "scale_target_name": "web", "scale_target_api_version": "apps/v1",
            "min_replicas": 1, "max_replicas": 5,
            "target_cpu_utilization": 70, "target_memory_utilization": 80,
            "behavior": { "scale_down": { "stabilization_window_seconds": 120 } }
        }),
    )
    .await;
    let raw = get_json(
        &client,
        "autoscaling/v2",
        "HorizontalPodAutoscaler",
        ns,
        "web",
    )
    .await;
    assert_eq!(raw["spec"]["metrics"].as_array().unwrap().len(), 2);
    assert_eq!(
        raw["spec"]["behavior"]["scaleDown"]["stabilizationWindowSeconds"],
        120
    );
    ok(
        &client,
        "update_hpa",
        json!({ "namespace": ns, "name": "web", "target_memory_utilization": 60 }),
    )
    .await;
    let raw = get_json(
        &client,
        "autoscaling/v2",
        "HorizontalPodAutoscaler",
        ns,
        "web",
    )
    .await;
    assert_eq!(raw["spec"]["metrics"][0]["resource"]["name"], "memory");

    ok(&client, "create_pdb", json!({ "namespace": ns, "name": "web", "min_available": 1, "selector": { "app": "web" }, "unhealthy_pod_eviction_policy": "AlwaysAllow" })).await;
    let raw = get_json(&client, "policy/v1", "PodDisruptionBudget", ns, "web").await;
    assert_eq!(raw["spec"]["unhealthyPodEvictionPolicy"], "AlwaysAllow");

    ok(&client, "create_role", json!({ "namespace": ns, "name": "reader", "rules": [{ "api_groups": [""], "resources": ["pods"], "verbs": ["get"], "resource_names": ["web"] }], "labels": { "team": "a" } })).await;
    let raw = get_json(
        &client,
        "rbac.authorization.k8s.io/v1",
        "Role",
        ns,
        "reader",
    )
    .await;
    assert_eq!(raw["rules"][0]["resourceNames"][0], "web");
    assert_eq!(raw["metadata"]["labels"]["team"], "a");
}
