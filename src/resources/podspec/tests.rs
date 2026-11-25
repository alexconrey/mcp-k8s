//! Tests for the shared pod template builder.

use k8s_openapi::api::core::v1::{Container, PodSpec};
use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;
use serde_json::{json, Value};

use super::*;
use crate::policy::{check_object, Source, WorkloadPolicy};

fn policy() -> WorkloadPolicy {
    WorkloadPolicy::default()
}

fn spec(args: Value, kind: WorkloadKind) -> PodSpec {
    build_pod_spec(&args, "web", kind, &policy()).unwrap()
}

fn err(args: Value, kind: WorkloadKind) -> String {
    build_pod_spec(&args, "web", kind, &policy()).unwrap_err()
}

fn only(spec: &PodSpec) -> &Container {
    assert_eq!(spec.containers.len(), 1);
    &spec.containers[0]
}

const ALL_KINDS: [WorkloadKind; 6] = [
    WorkloadKind::Pod,
    WorkloadKind::Job,
    WorkloadKind::CronJob,
    WorkloadKind::Deployment,
    WorkloadKind::StatefulSet,
    WorkloadKind::DaemonSet,
];

// ---------------------------------------------------------------------------
// Backward compatibility: the original minimal arguments
// ---------------------------------------------------------------------------

#[test]
fn minimal_arguments_build_the_same_core_object() {
    for kind in ALL_KINDS {
        let s = spec(
            json!({ "image": "nginx:1.27", "command": ["nginx", "-g", "daemon off;"] }),
            kind,
        );
        let c = only(&s);
        assert_eq!(c.name, "web", "container is named after the workload");
        assert_eq!(c.image.as_deref(), Some("nginx:1.27"));
        assert_eq!(c.command.as_ref().unwrap()[0], "nginx");
        let expected_restart = match kind {
            WorkloadKind::Pod | WorkloadKind::Job => Some("Never"),
            WorkloadKind::CronJob => Some("OnFailure"),
            _ => None,
        };
        assert_eq!(s.restart_policy.as_deref(), expected_restart, "{kind:?}");
    }
}

#[test]
fn legacy_port_env_and_env_from_still_work() {
    let s = spec(
        json!({
            "image": "app",
            "port": 8080,
            "env": { "LOG": "debug", "WORKERS": 4, "DEBUG": false },
            "env_from": [{ "secret_ref": "db" }, { "config_map_ref": "settings" }]
        }),
        WorkloadKind::Deployment,
    );
    let c = only(&s);
    assert_eq!(c.ports.as_ref().unwrap()[0].container_port, 8080);
    let env = c.env.as_ref().unwrap();
    let get = |n: &str| env.iter().find(|e| e.name == n).unwrap().value.clone();
    assert_eq!(get("LOG").as_deref(), Some("debug"));
    // Numbers and booleans used to become ""; they are now rendered.
    assert_eq!(get("WORKERS").as_deref(), Some("4"));
    assert_eq!(get("DEBUG").as_deref(), Some("false"));
    let env_from = c.env_from.as_ref().unwrap();
    assert_eq!(env_from[0].secret_ref.as_ref().unwrap().name, "db");
    assert_eq!(
        env_from[1].config_map_ref.as_ref().unwrap().name,
        "settings"
    );
}

#[test]
fn restart_policy_is_validated_per_kind() {
    assert_eq!(
        spec(
            json!({ "image": "x", "restart_policy": "Always" }),
            WorkloadKind::Pod
        )
        .restart_policy
        .as_deref(),
        Some("Always")
    );
    assert_eq!(
        spec(
            json!({ "image": "x", "restart_policy": "OnFailure" }),
            WorkloadKind::Job
        )
        .restart_policy
        .as_deref(),
        Some("OnFailure")
    );
    assert!(err(
        json!({ "image": "x", "restart_policy": "Always" }),
        WorkloadKind::Job
    )
    .contains("restart_policy"));
    assert!(err(
        json!({ "image": "x", "restart_policy": "Never" }),
        WorkloadKind::Deployment
    )
    .contains("restart_policy"));
}

// ---------------------------------------------------------------------------
// Secure defaults
// ---------------------------------------------------------------------------

#[test]
fn secure_defaults_are_applied() {
    let s = spec(json!({ "image": "x" }), WorkloadKind::Deployment);
    let pod_sc = s.security_context.as_ref().unwrap();
    assert_eq!(pod_sc.run_as_non_root, Some(true));
    assert_eq!(
        pod_sc.seccomp_profile.as_ref().unwrap().type_,
        "RuntimeDefault"
    );
    assert_eq!(s.automount_service_account_token, Some(false));
    let sc = only(&s).security_context.as_ref().unwrap();
    assert_eq!(sc.allow_privilege_escalation, Some(false));
    assert_eq!(
        sc.capabilities.as_ref().unwrap().drop.as_deref(),
        Some(&["ALL".to_string()][..])
    );
    assert!(sc.privileged.is_none());
}

#[test]
fn secure_defaults_can_be_turned_off() {
    let mut p = policy();
    p.secure_defaults = false;
    let s = build_pod_spec(&json!({ "image": "x" }), "web", WorkloadKind::Pod, &p).unwrap();
    assert!(s.security_context.is_none());
    assert!(s.automount_service_account_token.is_none());
    assert!(only(&s).security_context.is_none());
}

#[test]
fn root_images_keep_the_runtime_capabilities() {
    let s = spec(
        json!({ "image": "nginx", "security_context": { "run_as_non_root": false } }),
        WorkloadKind::Deployment,
    );
    assert_eq!(
        s.security_context.as_ref().unwrap().run_as_non_root,
        Some(false)
    );
    let sc = only(&s).security_context.as_ref().unwrap();
    assert_eq!(
        sc.allow_privilege_escalation,
        Some(false),
        "still no privilege escalation"
    );
    assert!(sc.capabilities.is_none(), "no drop ALL for root images");

    let s = spec(
        json!({ "image": "x", "security_context": { "run_as_user": 0 } }),
        WorkloadKind::Pod,
    );
    assert!(
        s.security_context
            .as_ref()
            .unwrap()
            .run_as_non_root
            .is_none(),
        "runAsNonRoot would contradict UID 0"
    );
    assert!(only(&s)
        .security_context
        .as_ref()
        .unwrap()
        .capabilities
        .is_none());
}

#[test]
fn a_root_container_overrides_the_defaulted_pod_setting() {
    let s = spec(
        json!({ "containers": [
            { "name": "root", "image": "x", "security_context": { "run_as_user": 0 } },
            { "name": "app", "image": "y" }
        ] }),
        WorkloadKind::Pod,
    );
    assert_eq!(
        s.security_context.as_ref().unwrap().run_as_non_root,
        Some(true)
    );
    let root = s.containers[0].security_context.as_ref().unwrap();
    assert_eq!(root.run_as_non_root, Some(false));
    assert!(
        root.capabilities.is_none(),
        "root keeps the runtime capabilities"
    );
    let app = s.containers[1].security_context.as_ref().unwrap();
    assert!(app.run_as_non_root.is_none(), "inherits the pod setting");
    // An explicit pod-level runAsNonRoot is the caller's choice: no override.
    let s = spec(
        json!({ "security_context": { "run_as_non_root": true }, "containers": [{ "name": "root", "image": "x", "security_context": { "run_as_user": 0 } }] }),
        WorkloadKind::Pod,
    );
    assert!(s.containers[0]
        .security_context
        .as_ref()
        .unwrap()
        .run_as_non_root
        .is_none());
}

#[test]
fn explicit_settings_win_over_defaults() {
    let s = spec(
        json!({
            "image": "x",
            "security_context": {
                "run_as_user": 1000, "run_as_group": 3000, "fs_group": 2000,
                "seccomp_profile": { "type": "Localhost", "localhost_profile": "profiles/app.json" },
                "capabilities": { "add": ["NET_BIND_SERVICE"], "drop": ["NET_RAW"] },
                "read_only_root_filesystem": true
            }
        }),
        WorkloadKind::Deployment,
    );
    let pod_sc = s.security_context.as_ref().unwrap();
    assert_eq!(
        (pod_sc.run_as_user, pod_sc.run_as_group, pod_sc.fs_group),
        (Some(1000), Some(3000), Some(2000))
    );
    assert_eq!(pod_sc.run_as_non_root, Some(true));
    let seccomp = pod_sc.seccomp_profile.as_ref().unwrap();
    assert_eq!(
        (seccomp.type_.as_str(), seccomp.localhost_profile.as_deref()),
        ("Localhost", Some("profiles/app.json"))
    );
    let sc = only(&s).security_context.as_ref().unwrap();
    let caps = sc.capabilities.as_ref().unwrap();
    assert_eq!(
        caps.add.as_deref(),
        Some(&["NET_BIND_SERVICE".to_string()][..])
    );
    assert_eq!(
        caps.drop.as_deref(),
        Some(&["NET_RAW".to_string()][..]),
        "explicit drop is kept"
    );
    assert_eq!(sc.read_only_root_filesystem, Some(true));
}

#[test]
fn privileged_and_sys_admin_skip_the_escalation_default() {
    // Kubernetes rejects allowPrivilegeEscalation: false with these; the
    // policy still has to allow them.
    let s = spec(
        json!({ "image": "x", "security_context": { "privileged": true } }),
        WorkloadKind::Pod,
    );
    let sc = only(&s).security_context.as_ref().unwrap();
    assert_eq!(sc.privileged, Some(true));
    assert!(sc.allow_privilege_escalation.is_none());
    assert!(sc.capabilities.is_none());

    let s = spec(
        json!({ "image": "x", "security_context": { "capabilities": { "add": ["CAP_SYS_ADMIN"] } } }),
        WorkloadKind::Pod,
    );
    assert!(only(&s)
        .security_context
        .as_ref()
        .unwrap()
        .allow_privilege_escalation
        .is_none());
}

#[test]
fn service_account_keeps_its_token() {
    let s = spec(
        json!({ "image": "x", "service_account_name": "builder" }),
        WorkloadKind::Job,
    );
    assert_eq!(s.service_account_name.as_deref(), Some("builder"));
    assert!(s.automount_service_account_token.is_none());
    let s = spec(
        json!({ "image": "x", "automount_service_account_token": true }),
        WorkloadKind::Job,
    );
    assert_eq!(s.automount_service_account_token, Some(true));
}

#[test]
fn defaults_pass_the_policy_for_every_kind() {
    for kind in ALL_KINDS {
        let (_, template) =
            build_template(&json!({ "image": "x" }), "web", kind, &policy()).unwrap();
        let obj = json!({
            "apiVersion": "apps/v1", "kind": "Deployment", "metadata": { "name": "web" },
            "spec": { "template": serde_json::to_value(&template).unwrap() }
        });
        assert!(
            check_object(&policy(), &obj, Source::Typed).is_empty(),
            "{kind:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// Containers
// ---------------------------------------------------------------------------

#[test]
fn full_container_fields() {
    let s = spec(
        json!({
            "image": "ghcr.io/org/app:1",
            "args": ["--port", "8080"],
            "working_dir": "/srv",
            "image_pull_policy": "IfNotPresent",
            "env": [
                { "name": "PLAIN", "value": "x" },
                { "name": "PASSWORD", "value_from": { "secret_key_ref": { "name": "db", "key": "password" } } },
                { "name": "MODE", "value_from": { "config_map_key_ref": { "name": "cfg", "key": "mode", "optional": true } } },
                { "name": "POD_IP", "value_from": { "field_ref": { "field_path": "status.podIP" } } },
                { "name": "MEM", "value_from": { "resource_field_ref": { "resource": "limits.memory", "divisor": "1Mi" } } }
            ],
            "env_from": [{ "config_map_ref": "cfg", "prefix": "CFG_" }],
            "ports": [8080, { "container_port": 9090, "name": "metrics", "protocol": "TCP" }],
            "resources": {
                "requests": { "cpu": "100m", "memory": "128Mi", "ephemeral-storage": "1Gi" },
                "limits": { "cpu": 1, "memory": "256Mi", "nvidia.com/gpu": "1" }
            },
            "volume_mounts": [{ "name": "data", "mount_path": "/data", "read_only": true, "sub_path": "x" }],
            "liveness_probe": { "probe_type": "httpGet", "path": "/healthz", "port": "metrics", "scheme": "HTTPS", "http_headers": [{ "name": "X", "value": "y" }] },
            "readiness_probe": { "probe_type": "tcpSocket", "port": 8080 },
            "startup_probe": { "probe_type": "exec", "command": ["cat", "/ready"], "failure_threshold": 30 },
            "lifecycle": {
                "pre_stop": { "handler_type": "sleep", "seconds": 5 },
                "post_start": { "handler_type": "exec", "command": ["/bin/warm"] }
            },
            "volumes": [{ "name": "data", "empty_dir": {} }]
        }),
        WorkloadKind::Deployment,
    );
    let c = only(&s);
    assert_eq!(
        c.args.as_ref().unwrap(),
        &["--port".to_string(), "8080".to_string()]
    );
    assert_eq!(c.working_dir.as_deref(), Some("/srv"));
    assert_eq!(c.image_pull_policy.as_deref(), Some("IfNotPresent"));
    let env = c.env.as_ref().unwrap();
    assert_eq!(env.len(), 5);
    let from = |i: usize| env[i].value_from.as_ref().unwrap();
    assert_eq!(from(1).secret_key_ref.as_ref().unwrap().key, "password");
    assert_eq!(
        from(2).config_map_key_ref.as_ref().unwrap().optional,
        Some(true)
    );
    assert_eq!(
        from(3).field_ref.as_ref().unwrap().field_path,
        "status.podIP"
    );
    assert_eq!(
        from(4)
            .resource_field_ref
            .as_ref()
            .unwrap()
            .divisor
            .as_ref()
            .unwrap()
            .0,
        "1Mi"
    );
    assert_eq!(
        c.env_from.as_ref().unwrap()[0].prefix.as_deref(),
        Some("CFG_")
    );
    let ports = c.ports.as_ref().unwrap();
    assert_eq!(
        (ports[0].container_port, ports[1].name.as_deref()),
        (8080, Some("metrics"))
    );
    let res = c.resources.as_ref().unwrap();
    assert_eq!(res.requests.as_ref().unwrap()["ephemeral-storage"].0, "1Gi");
    assert_eq!(res.limits.as_ref().unwrap()["cpu"].0, "1");
    assert_eq!(res.limits.as_ref().unwrap()["nvidia.com/gpu"].0, "1");
    let mount = &c.volume_mounts.as_ref().unwrap()[0];
    assert_eq!(
        (
            mount.mount_path.as_str(),
            mount.read_only,
            mount.sub_path.as_deref()
        ),
        ("/data", Some(true), Some("x"))
    );
    let live = c
        .liveness_probe
        .as_ref()
        .unwrap()
        .http_get
        .as_ref()
        .unwrap();
    assert_eq!(live.port, IntOrString::String("metrics".into()));
    assert_eq!(live.scheme.as_deref(), Some("HTTPS"));
    assert_eq!(live.http_headers.as_ref().unwrap()[0].name, "X");
    assert_eq!(
        c.readiness_probe
            .as_ref()
            .unwrap()
            .tcp_socket
            .as_ref()
            .unwrap()
            .port,
        IntOrString::Int(8080)
    );
    let startup = c.startup_probe.as_ref().unwrap();
    assert_eq!(
        startup.exec.as_ref().unwrap().command.as_ref().unwrap()[0],
        "cat"
    );
    assert_eq!(startup.failure_threshold, Some(30));
    let lifecycle = c.lifecycle.as_ref().unwrap();
    assert_eq!(
        lifecycle
            .pre_stop
            .as_ref()
            .unwrap()
            .sleep
            .as_ref()
            .unwrap()
            .seconds,
        5
    );
    assert!(lifecycle.post_start.as_ref().unwrap().exec.is_some());
}

#[test]
fn env_object_form_accepts_sources() {
    let s = spec(
        json!({ "image": "x", "env": { "TOKEN": { "secret_key_ref": { "name": "api", "key": "token" } }, "NAME": { "field_ref": { "field_path": "metadata.name" } } } }),
        WorkloadKind::Pod,
    );
    let env = only(&s).env.as_ref().unwrap();
    assert!(env.iter().all(|e| e.value_from.is_some()));
    let e = err(
        json!({ "image": "x", "env": { "BAD": { "secret_key_ref": { "name": "a", "key": "b" }, "field_ref": { "field_path": "x" } } } }),
        WorkloadKind::Pod,
    );
    assert!(e.contains("exactly one"), "{e}");
    let e = err(
        json!({ "image": "x", "env": [{ "name": "X", "value": "a", "value_from": { "field_ref": { "field_path": "x" } } }] }),
        WorkloadKind::Pod,
    );
    assert!(e.contains("not both"), "{e}");
}

#[test]
fn multi_container_init_and_native_sidecars() {
    let s = spec(
        json!({
            "containers": [
                { "name": "app", "image": "app:1", "ports": [8080] },
                { "name": "proxy", "image": "envoy", "security_context": { "read_only_root_filesystem": true } }
            ],
            "init_containers": [
                { "name": "migrate", "image": "app:1", "command": ["migrate"] },
                { "name": "log-shipper", "image": "fluent-bit", "restart_policy": "Always" }
            ]
        }),
        WorkloadKind::Deployment,
    );
    assert_eq!(s.containers.len(), 2);
    let init = s.init_containers.as_ref().unwrap();
    assert_eq!(init.len(), 2);
    assert!(init[0].restart_policy.is_none());
    assert_eq!(init[1].restart_policy.as_deref(), Some("Always"));
    // Defaults reach every container, init containers included.
    for c in s.containers.iter().chain(init.iter()) {
        assert_eq!(
            c.security_context
                .as_ref()
                .unwrap()
                .allow_privilege_escalation,
            Some(false),
            "{}",
            c.name
        );
    }
    assert_eq!(
        s.containers[1]
            .security_context
            .as_ref()
            .unwrap()
            .read_only_root_filesystem,
        Some(true)
    );
}

#[test]
fn container_shape_errors() {
    let both = err(
        json!({ "image": "x", "containers": [{ "name": "a", "image": "y" }] }),
        WorkloadKind::Pod,
    );
    assert!(both.contains("not both"), "{both}");
    assert!(err(json!({}), WorkloadKind::Pod).contains("image"));
    assert!(err(json!({ "containers": [] }), WorkloadKind::Pod).contains("image"));
    assert!(err(
        json!({ "containers": [{ "name": "a" }] }),
        WorkloadKind::Pod
    )
    .contains("image is required"));
    assert!(err(
        json!({ "containers": [{ "image": "a" }] }),
        WorkloadKind::Pod
    )
    .contains("name is required"));
    let dup = err(
        json!({ "containers": [{ "name": "a", "image": "x" }], "init_containers": [{ "name": "a", "image": "y" }] }),
        WorkloadKind::Pod,
    );
    assert!(dup.contains("twice"), "{dup}");
    let sidecar_main = err(
        json!({ "containers": [{ "name": "a", "image": "x", "restart_policy": "Always" }] }),
        WorkloadKind::Pod,
    );
    assert!(sidecar_main.contains("init_containers"), "{sidecar_main}");
    let name_with_list = err(
        json!({ "container_name": "x", "containers": [{ "name": "a", "image": "x" }] }),
        WorkloadKind::Pod,
    );
    assert!(
        name_with_list.contains("container_name"),
        "{name_with_list}"
    );
    let pod_field_in_container = err(
        json!({ "containers": [{ "name": "a", "image": "x", "security_context": { "fs_group": 1 } }] }),
        WorkloadKind::Pod,
    );
    assert!(
        pod_field_in_container.contains("fs_group"),
        "{pod_field_in_container}"
    );
    assert!(err(
        json!({ "image": "x", "image_pull_policy": "Sometimes" }),
        WorkloadKind::Pod
    )
    .contains("image_pull_policy"));
}

#[test]
fn unknown_fields_are_rejected_and_camel_case_is_accepted() {
    let e = err(
        json!({ "image": "x", "liveness_probe": { "probe_type": "httpGet", "pth": "/" } }),
        WorkloadKind::Pod,
    );
    assert!(
        e.contains("unknown field `pth`") && e.contains("`path`"),
        "{e}"
    );
    let e = err(
        json!({ "image": "x", "volumes": [{ "name": "v", "emptyDirr": {} }] }),
        WorkloadKind::Pod,
    );
    assert!(e.contains("unknown field"), "{e}");
    // Kubernetes spellings are aliases.
    let s = spec(
        json!({
            "containers": [{
                "name": "a", "image": "x",
                "securityContext": { "runAsUser": 1000, "readOnlyRootFilesystem": true },
                "volumeMounts": [{ "name": "v", "mountPath": "/v" }],
                "env": [{ "name": "N", "valueFrom": { "fieldRef": { "fieldPath": "metadata.name" } } }]
            }],
            "volumes": [{ "name": "v", "configMap": { "name": "cm", "defaultMode": 420 } }]
        }),
        WorkloadKind::Pod,
    );
    let c = only(&s);
    assert_eq!(c.security_context.as_ref().unwrap().run_as_user, Some(1000));
    assert_eq!(c.volume_mounts.as_ref().unwrap()[0].mount_path, "/v");
    assert_eq!(
        s.volumes.as_ref().unwrap()[0]
            .config_map
            .as_ref()
            .unwrap()
            .default_mode,
        Some(420)
    );
}

#[test]
fn probe_variants_and_errors() {
    let p = probe_from_args(
        &json!({ "p": { "probe_type": "grpc", "port": 50051, "service": "health" } }),
        "p",
    )
    .unwrap()
    .unwrap();
    assert_eq!(p.grpc.as_ref().unwrap().port, 50051);
    assert_eq!(p.grpc.as_ref().unwrap().service.as_deref(), Some("health"));
    let p = probe_from_args(&json!({ "p": { "probe_type": "httpGet" } }), "p")
        .unwrap()
        .unwrap();
    assert_eq!(
        p.http_get.as_ref().unwrap().port,
        IntOrString::Int(80),
        "default port stays 80"
    );
    assert!(probe_from_args(&json!({}), "p").unwrap().is_none());
    assert!(
        probe_from_args(&json!({ "p": { "probe_type": "exec" } }), "p")
            .unwrap_err()
            .contains("command")
    );
    assert!(probe_from_args(
        &json!({ "p": { "probe_type": "grpc", "port": "grpc" } }),
        "p"
    )
    .unwrap_err()
    .contains("numeric"));
    assert!(probe_from_args(
        &json!({ "p": { "probe_type": "httpGet", "scheme": "FTP" } }),
        "p"
    )
    .is_err());
}

// ---------------------------------------------------------------------------
// Pod fields
// ---------------------------------------------------------------------------

#[test]
fn every_volume_type() {
    let s = spec(
        json!({
            "image": "x",
            "volumes": [
                { "name": "cm", "config_map": { "name": "cfg", "items": [{ "key": "a", "path": "a.conf" }] } },
                { "name": "sec", "secret": { "secret_name": "tls", "default_mode": 256 } },
                { "name": "secret-by-name", "secret": { "name": "tls" } },
                { "name": "tmp", "empty_dir": { "medium": "Memory", "size_limit": "64Mi" } },
                { "name": "data", "persistent_volume_claim": { "claim_name": "data", "read_only": true } },
                { "name": "proj", "projected": { "sources": [
                    { "service_account_token": { "path": "token", "audience": "vault", "expiration_seconds": 3600 } },
                    { "config_map": { "name": "cfg" } },
                    { "secret": { "name": "tls" } },
                    { "downward_api": { "items": [{ "path": "labels", "field_ref": { "field_path": "metadata.labels" } }] } }
                ] } },
                { "name": "info", "downward_api": { "items": [{ "path": "cpu", "resource_field_ref": { "resource": "limits.cpu", "container_name": "web" } }] } },
                { "name": "host", "host_path": { "path": "/var/log", "type": "Directory" } },
                { "name": "share", "nfs": { "server": "nfs", "path": "/x" } },
                { "name": "inline", "csi": { "driver": "secrets-store.csi.k8s.io", "read_only": true, "volume_attributes": { "a": "b" }, "node_publish_secret_ref": "creds" } }
            ]
        }),
        WorkloadKind::Pod,
    );
    let v = s.volumes.as_ref().unwrap();
    assert_eq!(
        v[0].config_map.as_ref().unwrap().items.as_ref().unwrap()[0].path,
        "a.conf"
    );
    assert_eq!(v[1].secret.as_ref().unwrap().default_mode, Some(256));
    assert_eq!(
        v[2].secret.as_ref().unwrap().secret_name.as_deref(),
        Some("tls")
    );
    let tmp = v[3].empty_dir.as_ref().unwrap();
    assert_eq!(
        (
            tmp.medium.as_deref(),
            tmp.size_limit.as_ref().map(|q| q.0.as_str())
        ),
        (Some("Memory"), Some("64Mi"))
    );
    assert_eq!(
        v[4].persistent_volume_claim.as_ref().unwrap().read_only,
        Some(true)
    );
    let sources = v[5].projected.as_ref().unwrap().sources.as_ref().unwrap();
    assert_eq!(
        sources[0]
            .service_account_token
            .as_ref()
            .unwrap()
            .audience
            .as_deref(),
        Some("vault")
    );
    assert!(sources[3].downward_api.is_some());
    assert!(v[6].downward_api.is_some());
    assert_eq!(
        v[7].host_path.as_ref().unwrap().type_.as_deref(),
        Some("Directory")
    );
    assert!(v[8].nfs.is_some());
    assert_eq!(
        v[9].csi
            .as_ref()
            .unwrap()
            .node_publish_secret_ref
            .as_ref()
            .unwrap()
            .name,
        "creds"
    );

    // Building is separate from policy: the risky ones are rejected there.
    let pod = json!({ "apiVersion": "v1", "kind": "Pod", "metadata": { "name": "p" }, "spec": serde_json::to_value(&s).unwrap() });
    let violations = check_object(&policy(), &pod, Source::Typed);
    let mut flags: Vec<_> = violations.iter().map(|v| v.flag().cli).collect();
    flags.sort_unstable();
    flags.dedup();
    assert_eq!(flags, vec!["--allow-host-path", "--allow-volume-types"]);

    let e = err(
        json!({ "image": "x", "volumes": [{ "name": "v", "empty_dir": {}, "secret": { "secret_name": "s" } }] }),
        WorkloadKind::Pod,
    );
    assert!(e.contains("exactly one"), "{e}");
    let e = err(
        json!({ "image": "x", "volumes": [{ "name": "v" }] }),
        WorkloadKind::Pod,
    );
    assert!(e.contains("exactly one"), "{e}");
}

#[test]
fn scheduling_and_pod_settings() {
    let s = spec(
        json!({
            "image": "x",
            "service_account_name": "app",
            "image_pull_secrets": ["regcred", { "name": "other" }],
            "node_selector": { "disktype": "ssd" },
            "tolerations": [{ "key": "gpu", "operator": "Exists", "effect": "NoSchedule", "toleration_seconds": 60 }],
            "affinity": { "node_affinity": { "required_during_scheduling_ignored_during_execution": { "node_selector_terms": [{ "match_expressions": [{ "key": "zone", "operator": "In", "values": ["a"] }] }] } } },
            "topology_spread_constraints": [{ "maxSkew": 1, "topologyKey": "kubernetes.io/hostname", "whenUnsatisfiable": "DoNotSchedule", "labelSelector": { "matchLabels": { "app_name": "web" } } }],
            "priority_class_name": "high",
            "termination_grace_period_seconds": 45,
            "dns_policy": "None",
            "dns_config": { "nameservers": ["1.1.1.1"], "searches": ["svc.cluster.local"], "options": [{ "name": "ndots", "value": "2" }] },
            "host_aliases": [{ "ip": "10.0.0.1", "hostnames": ["db.local"] }],
            "runtime_class_name": "gvisor",
            "hostname": "db-0", "subdomain": "db", "share_process_namespace": true, "enable_service_links": false,
            "security_context": { "supplemental_groups": [5], "fs_group_change_policy": "OnRootMismatch", "sysctls": [{ "name": "net.ipv4.tcp_syncookies", "value": "1" }] }
        }),
        WorkloadKind::Deployment,
    );
    assert_eq!(s.service_account_name.as_deref(), Some("app"));
    let secrets: Vec<_> = s
        .image_pull_secrets
        .as_ref()
        .unwrap()
        .iter()
        .map(|r| r.name.as_str())
        .collect();
    assert_eq!(secrets, vec!["regcred", "other"]);
    assert_eq!(s.node_selector.as_ref().unwrap()["disktype"], "ssd");
    assert_eq!(
        s.tolerations.as_ref().unwrap()[0].toleration_seconds,
        Some(60)
    );
    // snake_case affinity keys are converted.
    let terms = &s
        .affinity
        .as_ref()
        .unwrap()
        .node_affinity
        .as_ref()
        .unwrap()
        .required_during_scheduling_ignored_during_execution
        .as_ref()
        .unwrap()
        .node_selector_terms;
    assert_eq!(terms[0].match_expressions.as_ref().unwrap()[0].key, "zone");
    let tsc = &s.topology_spread_constraints.as_ref().unwrap()[0];
    assert_eq!(tsc.topology_key, "kubernetes.io/hostname");
    // matchLabels keys are left alone.
    assert!(tsc
        .label_selector
        .as_ref()
        .unwrap()
        .match_labels
        .as_ref()
        .unwrap()
        .contains_key("app_name"));
    assert_eq!(s.priority_class_name.as_deref(), Some("high"));
    assert_eq!(s.termination_grace_period_seconds, Some(45));
    assert_eq!(s.dns_policy.as_deref(), Some("None"));
    assert_eq!(
        s.dns_config.as_ref().unwrap().options.as_ref().unwrap()[0]
            .name
            .as_deref(),
        Some("ndots")
    );
    assert_eq!(
        s.host_aliases.as_ref().unwrap()[0]
            .hostnames
            .as_ref()
            .unwrap()[0],
        "db.local"
    );
    assert_eq!(s.runtime_class_name.as_deref(), Some("gvisor"));
    assert_eq!(
        (s.hostname.as_deref(), s.subdomain.as_deref()),
        (Some("db-0"), Some("db"))
    );
    assert_eq!(
        (s.share_process_namespace, s.enable_service_links),
        (Some(true), Some(false))
    );
    let sc = s.security_context.as_ref().unwrap();
    assert_eq!(sc.supplemental_groups.as_deref(), Some(&[5][..]));
    assert_eq!(
        sc.sysctls.as_ref().unwrap()[0].name,
        "net.ipv4.tcp_syncookies"
    );
    assert!(err(
        json!({ "image": "x", "dns_policy": "Random" }),
        WorkloadKind::Pod
    )
    .contains("dns_policy"));
}

#[test]
fn host_namespace_flags_are_built_and_then_policed() {
    let s = spec(
        json!({ "image": "x", "host_network": true, "host_pid": true }),
        WorkloadKind::DaemonSet,
    );
    assert_eq!(
        (s.host_network, s.host_pid, s.host_ipc),
        (Some(true), Some(true), None)
    );
}

// ---------------------------------------------------------------------------
// Metadata
// ---------------------------------------------------------------------------

#[test]
fn labels_and_annotations() {
    let meta = build_meta(
        &json!({ "labels": { "tier": "web", "app.kubernetes.io/managed-by": "someone" }, "annotations": { "a": "1" }, "pod_annotations": { "prometheus.io/scrape": "true" } }),
        "web",
        WorkloadKind::Deployment,
    )
    .unwrap();
    assert_eq!(meta.labels["app"], "web");
    assert_eq!(meta.labels["tier"], "web");
    assert_eq!(
        meta.labels["app.kubernetes.io/managed-by"], "mcp-k8s",
        "managed-by can't be overridden"
    );
    assert_eq!(meta.annotations.as_ref().unwrap()["a"], "1");
    assert_eq!(meta.template.labels.as_ref().unwrap()["tier"], "web");
    assert_eq!(
        meta.template.annotations.as_ref().unwrap()["prometheus.io/scrape"],
        "true"
    );
    assert_eq!(
        meta.selector,
        BTreeMap::from([("app".to_string(), "web".to_string())])
    );

    let e = build_meta(
        &json!({ "labels": { "app": "other" } }),
        "web",
        WorkloadKind::Deployment,
    )
    .err()
    .unwrap();
    assert!(e.contains("selector"), "{e}");
    // A custom selector: its labels are added to the pods.
    let meta = build_meta(
        &json!({ "selector": { "app.kubernetes.io/name": "shop" }, "labels": { "app": "legacy" } }),
        "web",
        WorkloadKind::Deployment,
    )
    .unwrap();
    assert_eq!(
        meta.selector,
        BTreeMap::from([("app.kubernetes.io/name".to_string(), "shop".to_string())])
    );
    assert_eq!(
        meta.template.labels.as_ref().unwrap()["app.kubernetes.io/name"],
        "shop"
    );
    assert_eq!(
        meta.labels["app"], "legacy",
        "app is an ordinary label then"
    );
    let e = build_meta(
        &json!({ "selector": { "tier": "a" }, "labels": { "tier": "b" } }),
        "web",
        WorkloadKind::StatefulSet,
    )
    .err()
    .unwrap();
    assert!(e.contains("selector needs"), "{e}");
    assert!(build_meta(
        &json!({ "selector": { "app.kubernetes.io/managed-by": "x" } }),
        "web",
        WorkloadKind::DaemonSet
    )
    .is_err());
    // Pods, Jobs and CronJobs have no selector argument.
    let meta = build_meta(
        &json!({ "selector": { "x": "y" } }),
        "web",
        WorkloadKind::Job,
    )
    .unwrap();
    assert_eq!(
        meta.selector,
        BTreeMap::from([("app".to_string(), "web".to_string())])
    );
    // Pods have no selector: the app label may differ.
    let meta = build_meta(
        &json!({ "labels": { "app": "debug" } }),
        "web",
        WorkloadKind::Pod,
    )
    .unwrap();
    assert_eq!(meta.labels["app"], "debug");
    assert!(build_meta(
        &json!({ "pod_annotations": { "a": "b" } }),
        "p",
        WorkloadKind::Pod
    )
    .is_err());
}

// ---------------------------------------------------------------------------
// Top-level argument normalization
// ---------------------------------------------------------------------------

#[test]
fn normalize_args_accepts_camel_case_and_rejects_unknown_arguments() {
    let schema = workload_schema(
        WorkloadKind::Deployment,
        json!({ "replicas": { "type": "integer" } }),
        &["namespace", "name"],
    );
    let args = normalize_args(&json!({ "namespace": "a", "name": "b", "image": "x", "serviceAccountName": "sa", "imagePullPolicy": "Always", "replicas": 2 }), &schema).unwrap();
    assert_eq!(args["service_account_name"], "sa");
    assert_eq!(args["image_pull_policy"], "Always");
    // deckwatch's create_pod spelling.
    let args = normalize_args(
        &json!({ "namespace": "a", "name": "b", "image": "x", "service_account": "sa" }),
        &schema,
    )
    .unwrap();
    assert_eq!(args["service_account_name"], "sa");

    let e = normalize_args(
        &json!({ "namespace": "a", "name": "b", "imagee": "x" }),
        &schema,
    )
    .unwrap_err();
    assert!(
        e.contains("unknown argument 'imagee'") && e.contains("image"),
        "{e}"
    );
    let e = normalize_args(
        &json!({ "service_account_name": "a", "serviceAccountName": "b" }),
        &schema,
    )
    .unwrap_err();
    assert!(e.contains("twice"), "{e}");
    assert_eq!(normalize_args(&Value::Null, &schema).unwrap(), json!({}));
    assert!(normalize_args(&json!([1]), &schema).is_err());
}

#[test]
fn camelize_keys_skips_match_labels() {
    let v = camelize_keys(
        &json!({ "label_selector": { "match_labels": { "my_label": "x" }, "match_expressions": [{ "key": "k_1", "operator": "In" }] } }),
    );
    assert_eq!(v["labelSelector"]["matchLabels"]["my_label"], "x");
    assert_eq!(
        v["labelSelector"]["matchExpressions"][0]["key"], "k_1",
        "values are untouched"
    );
}

// ---------------------------------------------------------------------------
// Update patches
// ---------------------------------------------------------------------------

#[test]
fn update_patch_targets_the_container_and_adds_no_defaults() {
    let patch = build_template_patch(
        &json!({ "image": "nginx:1.28", "env": { "A": "1" } }),
        Some("web"),
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        patch,
        json!({ "spec": { "containers": [{ "name": "web", "image": "nginx:1.28", "env": [{ "name": "A", "value": "1" }] }] } })
    );
}

#[test]
fn update_patch_pod_fields_security_and_annotations() {
    let patch = build_template_patch(
        &json!({
            "volumes": [{ "name": "cache", "empty_dir": {} }],
            "node_selector": { "pool": "a" },
            "security_context": { "fs_group": 2000, "read_only_root_filesystem": true },
            "pod_annotations": { "x": "y" },
            "init_containers": [{ "name": "wait", "image": "busybox", "command": ["true"] }]
        }),
        Some("app"),
    )
    .unwrap()
    .unwrap();
    assert_eq!(patch["metadata"]["annotations"]["x"], "y");
    let spec = &patch["spec"];
    assert_eq!(spec["volumes"][0]["name"], "cache");
    assert_eq!(spec["nodeSelector"]["pool"], "a");
    assert_eq!(spec["securityContext"], json!({ "fsGroup": 2000 }));
    assert_eq!(
        spec["containers"],
        json!([{ "name": "app", "securityContext": { "readOnlyRootFilesystem": true } }])
    );
    assert_eq!(spec["initContainers"][0]["name"], "wait");
    assert!(spec.get("restartPolicy").is_none());
    assert!(spec.get("automountServiceAccountToken").is_none());
}

#[test]
fn update_patch_is_none_when_nothing_changes() {
    assert!(build_template_patch(&json!({ "replicas": 3 }), None)
        .unwrap()
        .is_none());
    assert!(build_template_patch(&json!({}), Some("web"))
        .unwrap()
        .is_none());
    assert!(touches_container(&json!({ "image": "x" })));
    assert!(touches_container(
        &json!({ "security_context": { "privileged": true } })
    ));
    assert!(!touches_container(
        &json!({ "security_context": { "fs_group": 1 }, "replicas": 2 })
    ));
}

#[test]
fn resolve_container_picks_the_right_one() {
    let pod = |names: &[&str]| PodSpec {
        containers: names
            .iter()
            .map(|n| Container {
                name: n.to_string(),
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    };
    assert_eq!(
        resolve_container(Some(&pod(&["web", "sidecar"])), None, "web").unwrap(),
        "web"
    );
    assert_eq!(
        resolve_container(Some(&pod(&["main"])), None, "web").unwrap(),
        "main"
    );
    assert_eq!(
        resolve_container(Some(&pod(&["a", "b"])), Some("b"), "web").unwrap(),
        "b"
    );
    assert!(resolve_container(Some(&pod(&["a", "b"])), None, "web")
        .unwrap_err()
        .contains("several"));
    assert!(resolve_container(Some(&pod(&["a"])), Some("z"), "web")
        .unwrap_err()
        .contains("not found"));
}

// ---------------------------------------------------------------------------
// Schemas
// ---------------------------------------------------------------------------

fn assert_strict_objects(schema: &Value, path: &str) {
    if let Some(obj) = schema.as_object() {
        if obj.contains_key("properties")
            && obj.get("additionalProperties") != Some(&Value::Bool(false))
        {
            // Env values may be a string or a source object.
            assert!(
                path.ends_with(".env.additionalProperties"),
                "{path} allows unknown properties"
            );
        }
        for (k, v) in obj {
            assert_strict_objects(v, &format!("{path}.{k}"));
        }
    } else if let Some(items) = schema.as_array() {
        for (i, v) in items.iter().enumerate() {
            assert_strict_objects(v, &format!("{path}[{i}]"));
        }
    }
}

#[test]
fn workload_schemas_share_the_container_and_pod_fields() {
    for kind in ALL_KINDS {
        let schema = workload_schema(kind, json!({}), &["namespace", "name"]);
        let props = schema["properties"].as_object().unwrap();
        for key in CONTAINER_SHORTHAND_KEYS.iter().chain(POD_KEYS.iter()) {
            let expected = match *key {
                "restart_policy" => matches!(
                    kind,
                    WorkloadKind::Pod | WorkloadKind::Job | WorkloadKind::CronJob
                ),
                "pod_annotations" => kind != WorkloadKind::Pod,
                _ => true,
            };
            assert_eq!(props.contains_key(*key), expected, "{kind:?}: {key}");
        }
        assert_eq!(schema["additionalProperties"], false);
        assert_strict_objects(&schema, &format!("{kind:?}"));
        // Every property is documented.
        for (k, v) in props {
            assert!(
                v.get("description").is_some()
                    || v.get("properties").is_some()
                    || v.get("items").is_some(),
                "{kind:?}.{k} has no description"
            );
        }
    }
}

#[test]
fn update_schemas_drop_creation_only_fields() {
    let schema = workload_update_schema(WorkloadKind::Deployment, json!({}));
    let props = schema["properties"].as_object().unwrap();
    assert!(props.contains_key("container"));
    for key in [
        "labels",
        "annotations",
        "restart_policy",
        "container_name",
        "selector",
    ] {
        assert!(!props.contains_key(key), "{key}");
    }
    assert_eq!(schema["required"], json!(["namespace", "name"]));
    let create = workload_schema(WorkloadKind::StatefulSet, json!({}), &["namespace", "name"]);
    assert!(create["properties"].get("selector").is_some());
    let pod = workload_schema(WorkloadKind::Pod, json!({}), &["namespace", "name"]);
    assert!(pod["properties"].get("selector").is_none());
}

#[test]
fn container_items_require_name_and_image() {
    let schema = workload_schema(WorkloadKind::Pod, json!({}), &["namespace", "name"]);
    assert_eq!(
        schema["properties"]["containers"]["items"]["required"],
        json!(["name", "image"])
    );
    let init = &schema["properties"]["init_containers"]["items"]["properties"];
    assert_eq!(init["restart_policy"]["enum"], json!(["Always"]));
    assert!(schema["properties"]["containers"]["items"]["properties"]
        .get("restart_policy")
        .is_none());
}
