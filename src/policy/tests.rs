//! Unit tests for every policy rule: each rejection and the opt-in that lifts it.

use serde_json::{json, Value};

use super::*;

fn strict() -> WorkloadPolicy {
    WorkloadPolicy::default()
}

fn pod(spec: Value) -> Value {
    json!({ "apiVersion": "v1", "kind": "Pod", "metadata": { "name": "p", "namespace": "ns" }, "spec": spec })
}

fn pod_with_container(container: Value) -> Value {
    let mut c = json!({ "name": "app", "image": "nginx" });
    if let (Some(base), Some(extra)) = (c.as_object_mut(), container.as_object()) {
        base.extend(extra.clone());
    }
    pod(json!({ "containers": [c] }))
}

fn check(policy: &WorkloadPolicy, object: &Value) -> Vec<Violation> {
    check_object(policy, object, Source::Typed)
}

fn manifest(policy: &WorkloadPolicy, object: &Value) -> Vec<Violation> {
    check_object(policy, object, Source::Manifest)
}

/// Assert exactly the given flags were hit (in any order, de-duplicated).
#[track_caller]
fn assert_flags(violations: &[Violation], expected: &[&str]) {
    let mut got: Vec<&str> = violations.iter().map(|v| v.flag().cli).collect();
    got.sort_unstable();
    got.dedup();
    let mut want = expected.to_vec();
    want.sort_unstable();
    assert_eq!(got, want, "violations: {violations:#?}");
}

// ---------------------------------------------------------------------------
// Defaults
// ---------------------------------------------------------------------------

#[test]
fn default_policy_is_restrictive_with_secure_defaults() {
    let p = strict();
    assert!(!p.allow_privileged && !p.allow_privilege_escalation && !p.allow_host_namespaces);
    assert!(!p.allow_host_path && !p.allow_host_ports && !p.allow_unconfined);
    assert!(!p.allow_node_port && !p.allow_load_balancer && !p.allow_external_services);
    assert!(!p.allow_ingress_snippets && !p.allow_rbac_escalation);
    assert!(p.allowed_capabilities.is_empty() && p.allowed_volume_types.is_empty());
    assert!(p.allowed_manifest_kinds.is_empty() && p.allowed_custom_resource_groups.is_empty());
    assert!(
        p.allowed_registries.is_empty(),
        "no registry restriction by default"
    );
    assert!(p.secure_defaults);
}

#[test]
fn a_plain_pod_passes() {
    let clean = pod(json!({
        "securityContext": { "runAsNonRoot": true, "seccompProfile": { "type": "RuntimeDefault" } },
        "containers": [{
            "name": "app", "image": "nginx:1.27",
            "securityContext": { "allowPrivilegeEscalation": false, "capabilities": { "drop": ["ALL"], "add": ["NET_BIND_SERVICE"] } },
            "ports": [{ "containerPort": 80 }],
            "livenessProbe": { "httpGet": { "path": "/", "port": 80 } }
        }],
        "volumes": [
            { "name": "a", "configMap": { "name": "cm" } },
            { "name": "b", "secret": { "secretName": "s" } },
            { "name": "c", "emptyDir": { "medium": "Memory" } },
            { "name": "d", "persistentVolumeClaim": { "claimName": "pvc" } },
            { "name": "e", "projected": { "sources": [] } },
            { "name": "f", "downwardAPI": { "items": [] } },
            { "name": "g", "ephemeral": { "volumeClaimTemplate": {} } }
        ]
    }));
    assert!(check(&strict(), &clean).is_empty());
    assert!(manifest(&strict(), &clean).is_empty());
}

#[test]
fn permissive_policy_allows_everything() {
    let nasty = pod(json!({
        "hostNetwork": true, "hostPID": true, "hostIPC": true,
        "securityContext": { "seccompProfile": { "type": "Unconfined" }, "sysctls": [{ "name": "kernel.msgmax", "value": "1" }] },
        "containers": [{
            "name": "app", "image": "evil.example.com/x",
            "securityContext": { "privileged": true, "allowPrivilegeEscalation": true, "capabilities": { "add": ["SYS_ADMIN"] }, "procMount": "Unmasked" },
            "ports": [{ "containerPort": 80, "hostPort": 80 }]
        }],
        "volumes": [{ "name": "root", "hostPath": { "path": "/" } }, { "name": "n", "nfs": { "server": "s", "path": "/" } }]
    }));
    assert!(!check(&strict(), &nasty).is_empty());
    assert!(check(&WorkloadPolicy::permissive(), &nasty).is_empty());
    assert!(!WorkloadPolicy::permissive().secure_defaults);
}

// ---------------------------------------------------------------------------
// Container security
// ---------------------------------------------------------------------------

#[test]
fn privileged_rejected_unless_allowed() {
    let obj = pod_with_container(json!({ "securityContext": { "privileged": true } }));
    let v = check(&strict(), &obj);
    assert_flags(&v, &["--allow-privileged"]);
    assert_eq!(v[0].location, "spec.containers[0] (app).securityContext");
    let mut p = strict();
    p.allow_privileged = true;
    assert!(check(&p, &obj).is_empty());
    // privileged: false is fine.
    assert!(check(
        &strict(),
        &pod_with_container(json!({ "securityContext": { "privileged": false } }))
    )
    .is_empty());
}

#[test]
fn privilege_escalation_rejected_unless_allowed() {
    let obj =
        pod_with_container(json!({ "securityContext": { "allowPrivilegeEscalation": true } }));
    assert_flags(&check(&strict(), &obj), &["--allow-privilege-escalation"]);
    let mut p = strict();
    p.allow_privilege_escalation = true;
    assert!(check(&p, &obj).is_empty());
}

#[test]
fn capabilities_beyond_the_baseline_need_an_allowlist_entry() {
    let add = |caps: Value| {
        pod_with_container(json!({ "securityContext": { "capabilities": { "add": caps } } }))
    };
    // The runtime's default set is always fine, with or without CAP_.
    assert!(check(
        &strict(),
        &add(json!(["CHOWN", "cap_setuid", "NET_BIND_SERVICE"]))
    )
    .is_empty());

    let v = check(&strict(), &add(json!(["NET_ADMIN"])));
    assert_flags(&v, &["--allow-capabilities"]);
    assert_eq!(
        v[0].remedy,
        Remedy::AddTo(&flags::ALLOW_CAPABILITIES, "NET_ADMIN".into())
    );
    assert!(
        v[0].to_string().contains("--allow-capabilities NET_ADMIN"),
        "{}",
        v[0]
    );
    assert_flags(
        &check(&strict(), &add(json!(["CAP_SYS_ADMIN"]))),
        &["--allow-capabilities"],
    );
    assert_flags(
        &check(&strict(), &add(json!(["ALL"]))),
        &["--allow-capabilities"],
    );

    let mut p = strict();
    p.allowed_capabilities = vec!["net_admin".into()];
    assert!(check(&p, &add(json!(["CAP_NET_ADMIN"]))).is_empty());
    assert!(!check(&p, &add(json!(["SYS_ADMIN"]))).is_empty());
    p.allowed_capabilities = vec!["ALL".into()];
    assert!(check(&p, &add(json!(["SYS_ADMIN", "ALL"]))).is_empty());
}

#[test]
fn host_ports_rejected_unless_allowed() {
    let obj = pod_with_container(json!({ "ports": [{ "containerPort": 80, "hostPort": 8080 }] }));
    assert_flags(&check(&strict(), &obj), &["--allow-host-ports"]);
    let mut p = strict();
    p.allow_host_ports = true;
    assert!(check(&p, &obj).is_empty());
    assert!(check(
        &strict(),
        &pod_with_container(json!({ "ports": [{ "containerPort": 80, "hostPort": 0 }] }))
    )
    .is_empty());
}

#[test]
fn unconfined_profiles_and_unmasked_proc_rejected_unless_allowed() {
    let cases = [
        pod(
            json!({ "securityContext": { "seccompProfile": { "type": "Unconfined" } }, "containers": [{ "name": "a", "image": "x" }] }),
        ),
        pod(
            json!({ "securityContext": { "appArmorProfile": { "type": "Unconfined" } }, "containers": [{ "name": "a", "image": "x" }] }),
        ),
        pod_with_container(
            json!({ "securityContext": { "seccompProfile": { "type": "Unconfined" } } }),
        ),
        pod_with_container(
            json!({ "securityContext": { "appArmorProfile": { "type": "Unconfined" } } }),
        ),
        pod_with_container(json!({ "securityContext": { "procMount": "Unmasked" } })),
        pod_with_container(json!({ "securityContext": { "seLinuxOptions": { "type": "spc_t" } } })),
        pod_with_container(
            json!({ "securityContext": { "seLinuxOptions": { "user": "system_u" } } }),
        ),
        pod(
            json!({ "securityContext": { "sysctls": [{ "name": "kernel.msgmax", "value": "65536" }] }, "containers": [{ "name": "a", "image": "x" }] }),
        ),
        pod_with_container(
            json!({ "livenessProbe": { "httpGet": { "host": "169.254.169.254", "port": 80 } } }),
        ),
        pod_with_container(
            json!({ "lifecycle": { "preStop": { "tcpSocket": { "host": "10.0.0.1", "port": 22 } } } }),
        ),
        json!({
            "apiVersion": "v1", "kind": "Pod",
            "metadata": { "name": "p", "annotations": { "container.apparmor.security.beta.kubernetes.io/app": "unconfined" } },
            "spec": { "containers": [{ "name": "app", "image": "x" }] }
        }),
    ];
    let mut p = strict();
    p.allow_unconfined = true;
    for case in &cases {
        assert_flags(&check(&strict(), case), &["--allow-unconfined"]);
        assert!(check(&p, case).is_empty(), "{case}");
    }
    // Confined settings are fine.
    let fine = [
        pod_with_container(
            json!({ "securityContext": { "seccompProfile": { "type": "Localhost", "localhostProfile": "p.json" } } }),
        ),
        pod_with_container(
            json!({ "securityContext": { "seLinuxOptions": { "type": "container_t", "level": "s0:c1" } } }),
        ),
        pod(
            json!({ "securityContext": { "sysctls": [{ "name": "net.ipv4.ip_unprivileged_port_start", "value": "0" }] }, "containers": [{ "name": "a", "image": "x" }] }),
        ),
        pod_with_container(
            json!({ "readinessProbe": { "httpGet": { "host": "localhost", "port": 80 } } }),
        ),
    ];
    for case in &fine {
        assert!(check(&strict(), case).is_empty(), "{case}");
    }
}

#[test]
fn windows_host_process_needs_allow_privileged() {
    let obj = pod_with_container(
        json!({ "securityContext": { "windowsOptions": { "hostProcess": true } } }),
    );
    assert_flags(&check(&strict(), &obj), &["--allow-privileged"]);
    let mut p = strict();
    p.allow_privileged = true;
    assert!(check(&p, &obj).is_empty());
}

#[test]
fn init_and_ephemeral_containers_are_checked_too() {
    let obj = pod(json!({
        "initContainers": [{ "name": "init", "image": "x", "securityContext": { "privileged": true } }],
        "ephemeralContainers": [{ "name": "debug", "image": "x", "securityContext": { "capabilities": { "add": ["SYS_PTRACE"] } } }],
        "containers": [{ "name": "app", "image": "x" }]
    }));
    let v = check(&strict(), &obj);
    assert_flags(&v, &["--allow-privileged", "--allow-capabilities"]);
    assert!(v
        .iter()
        .any(|v| v.location.contains("initContainers[0] (init)")));
    assert!(v
        .iter()
        .any(|v| v.location.contains("ephemeralContainers[0] (debug)")));
}

// ---------------------------------------------------------------------------
// Pod-level
// ---------------------------------------------------------------------------

#[test]
fn host_namespaces_rejected_unless_allowed() {
    for key in ["hostNetwork", "hostPID", "hostIPC"] {
        let mut spec = json!({ "containers": [{ "name": "a", "image": "x" }] });
        spec[key] = json!(true);
        let obj = pod(spec);
        let v = check(&strict(), &obj);
        assert_flags(&v, &["--allow-host-namespaces"]);
        assert_eq!(v[0].location, format!("spec.{key}"));
        let mut p = strict();
        p.allow_host_namespaces = true;
        assert!(check(&p, &obj).is_empty());
    }
}

#[test]
fn host_path_rejected_unless_allowed() {
    let obj = pod(
        json!({ "containers": [{ "name": "a", "image": "x" }], "volumes": [{ "name": "docker", "hostPath": { "path": "/var/run/docker.sock" } }] }),
    );
    let v = check(&strict(), &obj);
    assert_flags(&v, &["--allow-host-path"]);
    assert!(v[0].reason.contains("/var/run/docker.sock"));
    // "any volume type" does not include hostPath.
    let mut p = strict();
    p.allowed_volume_types = vec!["*".into()];
    assert_flags(&check(&p, &obj), &["--allow-host-path"]);
    p.allow_host_path = true;
    assert!(check(&p, &obj).is_empty());
}

#[test]
fn risky_volume_types_need_an_allowlist_entry() {
    for (field, source) in [
        (
            "nfs",
            json!({ "server": "nfs.example.com", "path": "/export" }),
        ),
        ("csi", json!({ "driver": "secrets-store.csi.k8s.io" })),
        (
            "gitRepo",
            json!({ "repository": "https://example.com/x.git" }),
        ),
        (
            "iscsi",
            json!({ "targetPortal": "1.2.3.4", "iqn": "x", "lun": 0 }),
        ),
    ] {
        let mut volume = json!({ "name": "v" });
        volume[field] = source;
        let obj =
            pod(json!({ "containers": [{ "name": "a", "image": "x" }], "volumes": [volume] }));
        let v = check(&strict(), &obj);
        assert_flags(&v, &["--allow-volume-types"]);
        assert_eq!(
            v[0].remedy,
            Remedy::AddTo(&flags::ALLOW_VOLUME_TYPES, field.into())
        );
        let mut p = strict();
        p.allowed_volume_types = vec![field.to_ascii_uppercase()];
        assert!(
            check(&p, &obj).is_empty(),
            "{field} should be allowed case-insensitively"
        );
        p.allowed_volume_types = vec!["*".into()];
        assert!(check(&p, &obj).is_empty());
    }
}

// ---------------------------------------------------------------------------
// Images
// ---------------------------------------------------------------------------

#[test]
fn image_normalization() {
    assert_eq!(normalize_image("nginx"), "docker.io/library/nginx");
    assert_eq!(
        normalize_image("nginx:1.27"),
        "docker.io/library/nginx:1.27"
    );
    assert_eq!(
        normalize_image("bitnami/redis:7"),
        "docker.io/bitnami/redis:7"
    );
    assert_eq!(
        normalize_image("docker.io/nginx"),
        "docker.io/library/nginx"
    );
    assert_eq!(
        normalize_image("index.docker.io/library/nginx"),
        "docker.io/library/nginx"
    );
    assert_eq!(
        normalize_image("ghcr.io/org/app@sha256:abc"),
        "ghcr.io/org/app@sha256:abc"
    );
    assert_eq!(normalize_image("localhost:5000/app"), "localhost:5000/app");
    assert_eq!(normalize_image("localhost/app"), "localhost/app");
}

#[test]
fn registry_allowlist() {
    let obj = |image: &str| pod(json!({ "containers": [{ "name": "a", "image": image }] }));
    // Empty allowlist: anything goes.
    assert!(check(&strict(), &obj("evil.example.com/x")).is_empty());

    let mut p = strict();
    p.allowed_registries = vec!["ghcr.io/myorg".into(), "docker.io/library/".into()];
    assert!(check(&p, &obj("ghcr.io/myorg/app:1")).is_empty());
    assert!(check(&p, &obj("ghcr.io/myorg/team/app@sha256:abc")).is_empty());
    assert!(check(&p, &obj("nginx")).is_empty());
    for bad in [
        "ghcr.io/myorgevil/app",
        "ghcr.io/other/app",
        "bitnami/redis",
        "quay.io/x",
    ] {
        let v = check(&p, &obj(bad));
        assert_flags(&v, &["--allowed-registries"]);
    }
    let v = check(&p, &obj("bitnami/redis"));
    assert_eq!(
        v[0].remedy,
        Remedy::AddTo(&flags::ALLOWED_REGISTRIES, "docker.io/bitnami".into())
    );
    // Init containers count too.
    let init = pod(
        json!({ "initContainers": [{ "name": "i", "image": "quay.io/x" }], "containers": [{ "name": "a", "image": "nginx" }] }),
    );
    assert_flags(&check(&p, &init), &["--allowed-registries"]);
}

// ---------------------------------------------------------------------------
// Every pod-bearing kind gets the same rules (apply_manifest parity)
// ---------------------------------------------------------------------------

fn privileged_spec() -> Value {
    json!({ "containers": [{ "name": "app", "image": "x", "securityContext": { "privileged": true } }] })
}

#[test]
fn every_pod_bearing_kind_is_checked() {
    let template = json!({ "metadata": { "labels": { "app": "x" } }, "spec": privileged_spec() });
    let cases = [
        (
            "v1",
            "Pod",
            json!({ "spec": privileged_spec() }),
            "spec.containers[0]",
        ),
        (
            "v1",
            "PodTemplate",
            json!({ "template": template }),
            "template.spec.containers[0]",
        ),
        (
            "v1",
            "ReplicationController",
            json!({ "spec": { "template": template } }),
            "spec.template.spec.containers[0]",
        ),
        (
            "apps/v1",
            "ReplicaSet",
            json!({ "spec": { "template": template } }),
            "spec.template.spec.containers[0]",
        ),
        (
            "apps/v1",
            "Deployment",
            json!({ "spec": { "template": template } }),
            "spec.template.spec.containers[0]",
        ),
        (
            "apps/v1",
            "StatefulSet",
            json!({ "spec": { "template": template } }),
            "spec.template.spec.containers[0]",
        ),
        (
            "apps/v1",
            "DaemonSet",
            json!({ "spec": { "template": template } }),
            "spec.template.spec.containers[0]",
        ),
        (
            "batch/v1",
            "Job",
            json!({ "spec": { "template": template } }),
            "spec.template.spec.containers[0]",
        ),
        (
            "batch/v1",
            "CronJob",
            json!({ "spec": { "schedule": "* * * * *", "jobTemplate": { "spec": { "template": template } } } }),
            "spec.jobTemplate.spec.template.spec.containers[0]",
        ),
    ];
    for (api_version, kind, body, location) in cases {
        let mut obj = body;
        obj["apiVersion"] = json!(api_version);
        obj["kind"] = json!(kind);
        obj["metadata"] = json!({ "name": "x", "namespace": "ns" });
        for v in [check(&strict(), &obj), manifest(&strict(), &obj)] {
            assert_flags(&v, &["--allow-privileged"]);
            assert!(
                v[0].location.starts_with(location),
                "{kind}: {}",
                v[0].location
            );
        }
    }
}

#[test]
fn pod_template_annotations_are_checked() {
    let obj = json!({
        "apiVersion": "apps/v1", "kind": "Deployment", "metadata": { "name": "x" },
        "spec": { "template": {
            "metadata": { "annotations": { "seccomp.security.alpha.kubernetes.io/pod": "unconfined" } },
            "spec": { "containers": [{ "name": "a", "image": "x" }] }
        } }
    });
    let v = check(&strict(), &obj);
    assert_flags(&v, &["--allow-unconfined"]);
    assert!(
        v[0].location
            .starts_with("spec.template.metadata.annotations"),
        "{}",
        v[0].location
    );
}

// ---------------------------------------------------------------------------
// Services
// ---------------------------------------------------------------------------

fn service(spec: Value, annotations: Value) -> Value {
    json!({ "apiVersion": "v1", "kind": "Service", "metadata": { "name": "s", "annotations": annotations }, "spec": spec })
}

#[test]
fn service_types_are_gated() {
    let none = json!({});
    assert!(check(
        &strict(),
        &service(json!({ "ports": [{ "port": 80 }] }), none.clone())
    )
    .is_empty());
    assert!(check(
        &strict(),
        &service(
            json!({ "type": "ClusterIP", "clusterIP": "None" }),
            none.clone()
        )
    )
    .is_empty());

    let np = service(
        json!({ "type": "NodePort", "ports": [{ "port": 80, "nodePort": 30080 }] }),
        none.clone(),
    );
    assert_flags(&check(&strict(), &np), &["--allow-nodeport"]);
    let mut p = strict();
    p.allow_node_port = true;
    assert!(check(&p, &np).is_empty());

    let lb = service(json!({ "type": "LoadBalancer" }), none.clone());
    assert_flags(&check(&strict(), &lb), &["--allow-loadbalancer"]);
    let mut p = strict();
    p.allow_load_balancer = true;
    assert!(check(&p, &lb).is_empty());

    let ext = service(
        json!({ "type": "ExternalName", "externalName": "db.example.com" }),
        none.clone(),
    );
    assert_flags(&check(&strict(), &ext), &["--allow-external-services"]);
    let ips = service(json!({ "externalIPs": ["1.2.3.4"] }), none);
    assert_flags(&check(&strict(), &ips), &["--allow-external-services"]);
    let mut p = strict();
    p.allow_external_services = true;
    assert!(check(&p, &ext).is_empty() && check(&p, &ips).is_empty());
}

#[test]
fn load_balancer_settings_and_annotations_need_the_load_balancer_opt_in() {
    for spec in [
        json!({ "loadBalancerSourceRanges": ["0.0.0.0/0"] }),
        json!({ "loadBalancerClass": "x" }),
        json!({ "loadBalancerIP": "1.2.3.4" }),
        json!({ "allocateLoadBalancerNodePorts": false }),
    ] {
        assert_flags(
            &check(&strict(), &service(spec, json!({}))),
            &["--allow-loadbalancer"],
        );
    }
    for key in [
        "service.beta.kubernetes.io/aws-load-balancer-scheme",
        "service.beta.kubernetes.io/azure-load-balancer-internal",
        "cloud.google.com/load-balancer-type",
        "networking.gke.io/load-balancer-type",
        "metallb.universe.tf/address-pool",
    ] {
        let mut ann = json!({});
        ann[key] = json!("x");
        let svc = service(json!({}), ann);
        assert_flags(&check(&strict(), &svc), &["--allow-loadbalancer"]);
        let mut p = strict();
        p.allow_load_balancer = true;
        assert!(check(&p, &svc).is_empty());
    }
    // Unrelated annotations are fine.
    assert!(check(
        &strict(),
        &service(json!({}), json!({ "prometheus.io/scrape": "true" }))
    )
    .is_empty());
}

// ---------------------------------------------------------------------------
// Ingresses
// ---------------------------------------------------------------------------

fn ingress(annotations: Value) -> Value {
    json!({
        "apiVersion": "networking.k8s.io/v1", "kind": "Ingress",
        "metadata": { "name": "i", "annotations": annotations },
        "spec": { "rules": [{ "http": { "paths": [{ "path": "/", "pathType": "Prefix", "backend": { "service": { "name": "s", "port": { "number": 80 } } } }] } }] }
    })
}

fn one(key: &str, value: &str) -> Value {
    let mut a = json!({});
    a[key] = json!(value);
    a
}

#[test]
fn snippet_annotations_rejected_unless_allowed() {
    for key in [
        "nginx.ingress.kubernetes.io/configuration-snippet",
        "nginx.ingress.kubernetes.io/server-snippet",
        "nginx.ingress.kubernetes.io/auth-snippet",
        "nginx.ingress.kubernetes.io/stream-snippet",
        "nginx.ingress.kubernetes.io/modsecurity-snippet",
        "nginx.org/server-snippets",
        "nginx.org/location-snippets",
        "haproxy.org/backend-config-snippet",
    ] {
        let obj = ingress(one(key, "return 200;"));
        assert_flags(&check(&strict(), &obj), &["--allow-ingress-snippets"]);
        let mut p = strict();
        p.allow_ingress_snippets = true;
        assert!(check(&p, &obj).is_empty(), "{key}");
    }
}

#[test]
fn nginx_annotation_values_cannot_inject_configuration() {
    let bad = [
        (
            "nginx.ingress.kubernetes.io/auth-url",
            "http://auth.example.com/#;\nload_module /tmp/evil.so;",
        ),
        (
            "nginx.ingress.kubernetes.io/auth-url",
            "http://auth.example.com/check;",
        ),
        (
            "nginx.ingress.kubernetes.io/auth-url",
            "http://auth.example.com/#frag",
        ),
        (
            "nginx.ingress.kubernetes.io/auth-url",
            "javascript:alert(1)",
        ),
        ("nginx.ingress.kubernetes.io/auth-signin", "https://x/ y"),
        (
            "nginx.ingress.kubernetes.io/auth-tls-match-cn",
            "CN=a\"}\nlocation",
        ),
        ("nginx.ingress.kubernetes.io/mirror-target", "https://x{"),
        ("nginx.ingress.kubernetes.io/rewrite-target", "/x'"),
        (
            "nginx.ingress.kubernetes.io/server-alias",
            "x access_by_lua_block",
        ),
        ("nginx.ingress.kubernetes.io/upstream-vhost", "x\\y"),
        (
            "nginx.ingress.kubernetes.io/proxy-redirect-from",
            "content_by_lua",
        ),
    ];
    for (key, value) in bad {
        let obj = ingress(one(key, value));
        assert_flags(&check(&strict(), &obj), &["--allow-ingress-snippets"]);
    }
    let good = [
        (
            "nginx.ingress.kubernetes.io/auth-url",
            "https://auth.example.com/oauth2/auth?rd=$escaped_request_uri",
        ),
        (
            "nginx.ingress.kubernetes.io/auth-signin",
            "https://auth.example.com/oauth2/start?rd=$scheme://$host$request_uri",
        ),
        ("nginx.ingress.kubernetes.io/rewrite-target", "/$2"),
        (
            "nginx.ingress.kubernetes.io/whitelist-source-range",
            "10.0.0.0/8,192.168.0.0/16",
        ),
        ("nginx.ingress.kubernetes.io/proxy-body-size", "8m"),
        ("cert-manager.io/cluster-issuer", "letsencrypt"),
        ("alb.ingress.kubernetes.io/scheme", "internal"),
    ];
    for (key, value) in good {
        assert!(
            check(&strict(), &ingress(one(key, value))).is_empty(),
            "{key}={value}"
        );
    }
}

#[test]
fn ingress_annotation_allowlist_and_denylist() {
    let mut p = strict();
    p.ingress_annotation_allowlist = vec![
        "cert-manager.io/*".into(),
        "nginx.ingress.kubernetes.io/rewrite-target".into(),
    ];
    assert!(check(&p, &ingress(one("cert-manager.io/cluster-issuer", "le"))).is_empty());
    assert!(check(
        &p,
        &ingress(one("nginx.ingress.kubernetes.io/rewrite-target", "/"))
    )
    .is_empty());
    let v = check(
        &p,
        &ingress(one("nginx.ingress.kubernetes.io/proxy-body-size", "1m")),
    );
    assert_flags(&v, &["--ingress-annotation-allowlist"]);

    let mut p = strict();
    p.ingress_annotation_denylist = vec!["nginx.ingress.kubernetes.io/auth-*".into()];
    let v = check(
        &p,
        &ingress(one(
            "nginx.ingress.kubernetes.io/auth-url",
            "https://a.example.com",
        )),
    );
    assert_flags(&v, &["--ingress-annotation-denylist"]);
    assert!(
        v[0].to_string().contains(
            "nginx.ingress.kubernetes.io/auth-* is listed in --ingress-annotation-denylist"
        ),
        "{}",
        v[0]
    );
    // The denylist wins even when snippets are allowed.
    p.allow_ingress_snippets = true;
    assert_flags(
        &check(
            &p,
            &ingress(one("nginx.ingress.kubernetes.io/auth-url", "x")),
        ),
        &["--ingress-annotation-denylist"],
    );
}

#[test]
fn ingress_paths_cannot_inject_configuration() {
    let with_path = |path: &str, path_type: &str| {
        json!({
            "apiVersion": "networking.k8s.io/v1", "kind": "Ingress", "metadata": { "name": "i" },
            "spec": { "rules": [{ "http": { "paths": [{ "path": path, "pathType": path_type, "backend": { "service": { "name": "s", "port": { "number": 80 } } } }] } }] }
        })
    };
    assert_flags(
        &check(&strict(), &with_path("/a;b", "Prefix")),
        &["--allow-ingress-snippets"],
    );
    assert_flags(
        &check(&strict(), &with_path("/{x}", "Exact")),
        &["--allow-ingress-snippets"],
    );
    assert_flags(
        &check(&strict(), &with_path("/a b", "ImplementationSpecific")),
        &["--allow-ingress-snippets"],
    );
    assert!(check(
        &strict(),
        &with_path("/api/v[0-9]{1,2}(/|$)(.*)", "ImplementationSpecific")
    )
    .is_empty());
    assert!(check(&strict(), &with_path("/static/", "Prefix")).is_empty());
}

// ---------------------------------------------------------------------------
// RBAC, secrets, namespaces
// ---------------------------------------------------------------------------

fn role(rules: Value) -> Value {
    json!({ "apiVersion": "rbac.authorization.k8s.io/v1", "kind": "Role", "metadata": { "name": "r" }, "rules": rules })
}

#[test]
fn rbac_escalation_primitives_rejected_unless_allowed() {
    let cases = [
        json!([{ "apiGroups": [""], "resources": ["pods"], "verbs": ["*"] }]),
        json!([{ "apiGroups": ["*"], "resources": ["pods"], "verbs": ["get"] }]),
        json!([{ "apiGroups": [""], "resources": ["*"], "verbs": ["get"] }]),
        json!([{ "apiGroups": [""], "resources": ["pods/*"], "verbs": ["get"] }]),
        json!([{ "apiGroups": ["rbac.authorization.k8s.io"], "resources": ["roles"], "verbs": ["escalate"] }]),
        json!([{ "apiGroups": ["rbac.authorization.k8s.io"], "resources": ["clusterroles"], "verbs": ["bind"] }]),
        json!([{ "apiGroups": [""], "resources": ["serviceaccounts"], "verbs": ["impersonate"] }]),
        json!([{ "nonResourceURLs": ["*"], "verbs": ["get"] }]),
        json!([{ "nonResourceURLs": ["/api/*"], "verbs": ["get"] }]),
    ];
    let mut p = strict();
    p.allow_rbac_escalation = true;
    for rules in cases {
        let obj = role(rules.clone());
        assert_flags(&check(&strict(), &obj), &["--allow-rbac-escalation"]);
        assert!(check(&p, &obj).is_empty(), "{rules}");
    }
    let fine = role(
        json!([{ "apiGroups": ["", "apps"], "resources": ["pods", "deployments", "pods/log"], "verbs": ["get", "list", "watch"], "resourceNames": ["x"] }]),
    );
    assert!(check(&strict(), &fine).is_empty());
}

#[test]
fn cluster_role_aggregation_is_an_escalation() {
    let aggregated = json!({
        "apiVersion": "rbac.authorization.k8s.io/v1", "kind": "ClusterRole", "metadata": { "name": "c" },
        "aggregationRule": { "clusterRoleSelectors": [{ "matchLabels": { "x": "y" } }] }
    });
    assert_flags(&check(&strict(), &aggregated), &["--allow-rbac-escalation"]);
    let pushes = json!({
        "apiVersion": "rbac.authorization.k8s.io/v1", "kind": "ClusterRole",
        "metadata": { "name": "c", "labels": { "rbac.authorization.k8s.io/aggregate-to-view": "true" } },
        "rules": [{ "apiGroups": [""], "resources": ["secrets"], "verbs": ["get"] }]
    });
    assert_flags(&check(&strict(), &pushes), &["--allow-rbac-escalation"]);
    let mut p = strict();
    p.allow_rbac_escalation = true;
    assert!(check(&p, &aggregated).is_empty() && check(&p, &pushes).is_empty());
}

#[test]
fn bindings_to_cluster_admin_or_anonymous_rejected_unless_allowed() {
    let binding = |kind: &str, role_ref: Value, subjects: Value| json!({ "apiVersion": "rbac.authorization.k8s.io/v1", "kind": kind, "metadata": { "name": "b" }, "roleRef": role_ref, "subjects": subjects });
    let sa = json!([{ "kind": "ServiceAccount", "name": "app", "namespace": "ns" }]);
    let admin = json!({ "apiGroup": "rbac.authorization.k8s.io", "kind": "ClusterRole", "name": "cluster-admin" });
    let view =
        json!({ "apiGroup": "rbac.authorization.k8s.io", "kind": "ClusterRole", "name": "view" });
    let mut p = strict();
    p.allow_rbac_escalation = true;
    for kind in ["RoleBinding", "ClusterRoleBinding"] {
        let obj = binding(kind, admin.clone(), sa.clone());
        assert_flags(&check(&strict(), &obj), &["--allow-rbac-escalation"]);
        assert!(check(&p, &obj).is_empty());
        for anon in ["system:anonymous", "system:unauthenticated"] {
            let obj = binding(
                kind,
                view.clone(),
                json!([{ "kind": "Group", "name": anon }]),
            );
            assert_flags(&check(&strict(), &obj), &["--allow-rbac-escalation"]);
            assert!(check(&p, &obj).is_empty());
        }
        assert!(check(&strict(), &binding(kind, view.clone(), sa.clone())).is_empty());
    }
}

#[test]
fn bound_role_rules_are_checked() {
    let mut out = Vec::new();
    let edit_like = json!({ "rules": [{ "apiGroups": [""], "resources": ["serviceaccounts"], "verbs": ["impersonate"] }] });
    rbac::check_bound_role(&strict(), "ClusterRole", "edit", &edit_like, &mut out);
    assert_flags(&out, &["--allow-rbac-escalation"]);
    assert!(out[0].reason.contains("\"edit\""), "{}", out[0].reason);
    // The built-in view role is aggregated and labelled aggregate-to-edit;
    // only its materialized rules count for a binding.
    let mut out = Vec::new();
    let view_like = json!({
        "metadata": { "labels": { "rbac.authorization.k8s.io/aggregate-to-edit": "true" } },
        "aggregationRule": { "clusterRoleSelectors": [{ "matchLabels": { "rbac.authorization.k8s.io/aggregate-to-view": "true" } }] },
        "rules": [{ "apiGroups": [""], "resources": ["pods"], "verbs": ["get", "list"] }]
    });
    rbac::check_bound_role(&strict(), "ClusterRole", "view", &view_like, &mut out);
    assert!(out.is_empty());
    let mut out = Vec::new();
    let mut p = strict();
    p.allow_rbac_escalation = true;
    rbac::check_bound_role(&p, "ClusterRole", "edit", &edit_like, &mut out);
    assert!(out.is_empty());
}

#[test]
fn service_account_token_secrets_rejected_unless_allowed() {
    let token = json!({
        "apiVersion": "v1", "kind": "Secret",
        "metadata": { "name": "t", "annotations": { "kubernetes.io/service-account.name": "admin" } },
        "type": "kubernetes.io/service-account-token"
    });
    let v = check(&strict(), &token);
    assert_flags(&v, &["--allow-rbac-escalation"]);
    assert!(v[0].reason.contains("\"admin\""));
    let mut p = strict();
    p.allow_rbac_escalation = true;
    assert!(check(&p, &token).is_empty());
    let opaque = json!({ "apiVersion": "v1", "kind": "Secret", "metadata": { "name": "s" }, "type": "Opaque" });
    assert!(check(&strict(), &opaque).is_empty());
}

#[test]
fn privileged_pod_security_namespace_labels_need_allow_privileged() {
    let ns = |level: &str| json!({ "apiVersion": "v1", "kind": "Namespace", "metadata": { "name": "n", "labels": { "pod-security.kubernetes.io/enforce": level } } });
    assert_flags(
        &check(&strict(), &ns("privileged")),
        &["--allow-privileged"],
    );
    assert!(check(&strict(), &ns("restricted")).is_empty());
    assert!(check(&strict(), &ns("baseline")).is_empty());
    let mut p = strict();
    p.allow_privileged = true;
    assert!(check(&p, &ns("privileged")).is_empty());
}

// ---------------------------------------------------------------------------
// Raw manifest gates
// ---------------------------------------------------------------------------

#[test]
fn sensitive_and_cluster_scoped_kinds_are_rejected_from_manifests() {
    let cases = [
        ("rbac.authorization.k8s.io/v1", "Role"),
        ("rbac.authorization.k8s.io/v1", "RoleBinding"),
        ("rbac.authorization.k8s.io/v1", "ClusterRole"),
        ("rbac.authorization.k8s.io/v1", "ClusterRoleBinding"),
        (
            "admissionregistration.k8s.io/v1",
            "ValidatingWebhookConfiguration",
        ),
        (
            "admissionregistration.k8s.io/v1",
            "MutatingWebhookConfiguration",
        ),
        (
            "admissionregistration.k8s.io/v1",
            "ValidatingAdmissionPolicy",
        ),
        ("apiextensions.k8s.io/v1", "CustomResourceDefinition"),
        ("apiregistration.k8s.io/v1", "APIService"),
        ("v1", "Endpoints"),
        ("discovery.k8s.io/v1", "EndpointSlice"),
        ("apps/v1", "ControllerRevision"),
        ("v1", "Namespace"),
        ("v1", "PersistentVolume"),
        ("storage.k8s.io/v1", "StorageClass"),
        ("scheduling.k8s.io/v1", "PriorityClass"),
        ("certificates.k8s.io/v1", "CertificateSigningRequest"),
    ];
    for (api_version, kind) in cases {
        let obj = json!({ "apiVersion": api_version, "kind": kind, "metadata": { "name": "x" } });
        let v = manifest(&strict(), &obj);
        assert_flags(&v, &["--allow-manifest-kinds"]);
        assert_eq!(
            v[0].remedy,
            Remedy::AddTo(&flags::ALLOW_MANIFEST_KINDS, kind.into())
        );
        // Typed tools aren't gated by kind.
        assert!(check(&strict(), &obj).is_empty(), "{kind}");
        // Allowlisted by kind, by group/kind, or with "*".
        let group = api_version
            .rsplit_once('/')
            .map(|(g, _)| g)
            .unwrap_or("core");
        for entry in [kind.to_string(), format!("{group}/{kind}"), "*".to_string()] {
            let mut p = strict();
            p.allowed_manifest_kinds = vec![entry.clone()];
            assert!(manifest(&p, &obj).is_empty(), "{kind} with {entry}");
        }
        let mut p = strict();
        p.allowed_manifest_kinds = vec!["other.example.com/Role".into()];
        assert!(!manifest(&p, &obj).is_empty(), "group must match: {kind}");
    }
}

#[test]
fn allowlisted_rbac_manifests_still_get_the_rbac_rules() {
    let mut p = strict();
    p.allowed_manifest_kinds = vec!["ClusterRole".into()];
    let wildcard = json!({
        "apiVersion": "rbac.authorization.k8s.io/v1", "kind": "ClusterRole", "metadata": { "name": "x" },
        "rules": [{ "apiGroups": ["*"], "resources": ["*"], "verbs": ["*"] }]
    });
    assert_flags(&manifest(&p, &wildcard), &["--allow-rbac-escalation"]);
}

#[test]
fn rejected_kind_messages_point_to_typed_tools() {
    let obj = json!({ "apiVersion": "rbac.authorization.k8s.io/v1", "kind": "ClusterRole", "metadata": { "name": "x" } });
    let v = manifest(&strict(), &obj);
    assert!(
        v[0].reason.contains("create_clusterrole"),
        "{}",
        v[0].reason
    );
}

#[test]
fn custom_resources_need_an_allowlisted_group() {
    let cr = json!({ "apiVersion": "cert-manager.io/v1", "kind": "Certificate", "metadata": { "name": "c" }, "spec": { "secretName": "x" } });
    let v = manifest(&strict(), &cr);
    assert_flags(&v, &["--allow-custom-resource-groups"]);
    assert_eq!(
        v[0].remedy,
        Remedy::AddTo(
            &flags::ALLOW_CUSTOM_RESOURCE_GROUPS,
            "cert-manager.io".into()
        )
    );
    let mut p = strict();
    p.allowed_custom_resource_groups = vec!["cert-manager.io".into()];
    assert!(manifest(&p, &cr).is_empty());
    p.allowed_custom_resource_groups = vec!["*".into()];
    assert!(manifest(&p, &cr).is_empty());
    assert!(
        is_builtin_group("") && is_builtin_group("apps") && is_builtin_group("networking.k8s.io")
    );
    assert!(!is_builtin_group("cert-manager.io") && !is_builtin_group("gateway.networking.k8s.io"));
}

#[test]
fn pod_specs_embedded_in_custom_resources_are_checked() {
    let mut p = strict();
    p.allowed_custom_resource_groups = vec!["*".into()];
    // Argo Workflow: templates[].container is a lone container.
    let workflow = json!({
        "apiVersion": "argoproj.io/v1alpha1", "kind": "Workflow", "metadata": { "name": "w" },
        "spec": { "entrypoint": "main", "templates": [{ "name": "main", "container": { "image": "alpine", "securityContext": { "privileged": true } } }],
                  "volumes": [{ "name": "host", "hostPath": { "path": "/" } }] }
    });
    let v = manifest(&p, &workflow);
    assert_flags(&v, &["--allow-privileged", "--allow-host-path"]);
    assert!(
        v.iter()
            .any(|v| v.location.starts_with("spec.templates[0].container")),
        "{v:#?}"
    );
    // Knative-style pod spec with containers.
    let ksvc = json!({
        "apiVersion": "serving.knative.dev/v1", "kind": "Service", "metadata": { "name": "k" },
        "spec": { "template": { "spec": { "hostNetwork": true, "containers": [{ "image": "x" }] } } }
    });
    assert_flags(&manifest(&p, &ksvc), &["--allow-host-namespaces"]);
    // Even with the group allowlisted, the deep scan applies to update results.
    assert_flags(
        &check_object(&p, &ksvc, Source::Manifest),
        &["--allow-host-namespaces"],
    );
}

#[test]
fn manifests_embedded_in_custom_resources_are_checked() {
    let mut p = strict();
    p.allowed_custom_resource_groups = vec!["kubernetes.crossplane.io".into()];
    let object = json!({
        "apiVersion": "kubernetes.crossplane.io/v1alpha2", "kind": "Object", "metadata": { "name": "o" },
        "spec": { "forProvider": { "manifest": {
            "apiVersion": "rbac.authorization.k8s.io/v1", "kind": "ClusterRoleBinding",
            "metadata": { "name": "pwn" },
            "roleRef": { "apiGroup": "rbac.authorization.k8s.io", "kind": "ClusterRole", "name": "cluster-admin" },
            "subjects": [{ "kind": "User", "name": "me" }]
        } } }
    });
    let v = manifest(&p, &object);
    assert_flags(&v, &["--allow-manifest-kinds", "--allow-rbac-escalation"]);
    assert!(
        v[0].location.contains("embedded ClusterRoleBinding"),
        "{}",
        v[0].location
    );
    // References ({apiVersion, kind, name}) are not manifests.
    let scaled = json!({
        "apiVersion": "keda.sh/v1alpha1", "kind": "ScaledObject", "metadata": { "name": "s" },
        "spec": { "scaleTargetRef": { "apiVersion": "rbac.authorization.k8s.io/v1", "kind": "ClusterRole", "name": "x" } }
    });
    p.allowed_custom_resource_groups = vec!["keda.sh".into()];
    assert!(manifest(&p, &scaled).is_empty());
}

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

#[test]
fn error_messages_name_the_flag_env_and_field() {
    let obj = pod(json!({
        "hostNetwork": true,
        "containers": [{ "name": "app", "image": "x", "securityContext": { "privileged": true } }]
    }));
    let err = into_result(check(&strict(), &obj)).unwrap_err();
    let text = err.to_string();
    assert!(
        text.starts_with("rejected by the mcp-k8s workload policy (2 violations):"),
        "{text}"
    );
    assert!(text.contains("--allow-privileged (env MCP_K8S_ALLOW_PRIVILEGED=true; library: WorkloadPolicy.allow_privileged)"), "{text}");
    assert!(text.contains("--allow-host-namespaces"), "{text}");
    assert!(text.contains("including apply_manifest"), "{text}");
}

#[test]
fn violations_are_deduplicated() {
    let v = Violation::new("spec", "x", Remedy::Enable(&flags::ALLOW_PRIVILEGED));
    let err = into_result(vec![v.clone(), v.clone()]).unwrap_err();
    assert_eq!(err.violations.len(), 1);
    assert!(into_result(Vec::new()).is_ok());
}

#[test]
fn every_flag_is_documented_with_a_matching_env_var() {
    for f in flags::ALL {
        assert!(f.cli.starts_with("--"), "{f:?}");
        let expected_env = format!(
            "MCP_K8S_{}",
            f.cli
                .trim_start_matches("--")
                .replace('-', "_")
                .to_ascii_uppercase()
        );
        assert_eq!(f.env, expected_env, "{f:?}");
    }
    let mut clis: Vec<_> = flags::ALL.iter().map(|f| f.cli).collect();
    clis.sort_unstable();
    clis.dedup();
    assert_eq!(clis.len(), flags::ALL.len());
}

#[test]
fn helper_predicates() {
    let mut p = strict();
    assert!(
        p.capability_allowed("chown")
            && !p.capability_allowed("NET_RAW")
            && !p.capability_allowed("ALL")
    );
    assert!(
        p.volume_type_allowed("projected")
            && !p.volume_type_allowed("hostPath")
            && !p.volume_type_allowed("nfs")
    );
    assert!(p.image_allowed("anything"));
    assert!(!p.manifest_kind_allowed("rbac.authorization.k8s.io", "Role"));
    assert!(!p.custom_resource_group_allowed("x.io"));
    p.allowed_manifest_kinds = vec!["core/Endpoints".into()];
    assert!(p.manifest_kind_allowed("", "endpoints"));
    assert!(!p.manifest_kind_allowed("discovery.k8s.io", "Endpoints"));
    assert!(
        pattern_matches("a/*", "a/b")
            && !pattern_matches("a/b", "a/bc")
            && pattern_matches("a/b", "a/b")
    );
    assert_eq!(normalize_capability(" cap_net_raw "), "NET_RAW");
}
