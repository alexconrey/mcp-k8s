//! Pod spec rules, applied to the Kubernetes JSON form of a `PodSpec`.

use serde_json::Value;

use super::{flags, normalize_capability, Remedy, Violation, WorkloadPolicy};

/// Capabilities container runtimes grant by default (the Pod Security
/// Standards "baseline" list). Adding them back after `drop: [ALL]` never gives
/// a container more than a default container has, so they are always allowed.
pub const BASELINE_CAPABILITIES: &[&str] = &[
    "AUDIT_WRITE",
    "CHOWN",
    "DAC_OVERRIDE",
    "FOWNER",
    "FSETID",
    "KILL",
    "MKNOD",
    "NET_BIND_SERVICE",
    "SETFCAP",
    "SETGID",
    "SETPCAP",
    "SETUID",
    "SYS_CHROOT",
];

/// Volume types allowed without opt-in, by Kubernetes field name. These only
/// expose data from the pod's own namespace or scratch space. Everything else
/// (`nfs`, `csi` inline volumes, `iscsi`, `gitRepo`, cloud disks, …) needs
/// [`WorkloadPolicy::allowed_volume_types`]; `hostPath` needs
/// [`WorkloadPolicy::allow_host_path`].
pub const SAFE_VOLUME_TYPES: &[&str] = &[
    "configMap",
    "secret",
    "emptyDir",
    "persistentVolumeClaim",
    "projected",
    "downwardAPI",
    "ephemeral",
];

/// Namespaced sysctls the Pod Security Standards "baseline" profile allows.
const SAFE_SYSCTLS: &[&str] = &[
    "kernel.shm_rmid_forced",
    "net.ipv4.ip_local_port_range",
    "net.ipv4.ip_local_reserved_ports",
    "net.ipv4.ip_unprivileged_port_start",
    "net.ipv4.ping_group_range",
    "net.ipv4.tcp_fin_timeout",
    "net.ipv4.tcp_keepalive_intvl",
    "net.ipv4.tcp_keepalive_probes",
    "net.ipv4.tcp_keepalive_time",
    "net.ipv4.tcp_syncookies",
];

/// SELinux types the "baseline" profile allows.
const SAFE_SELINUX_TYPES: &[&str] = &[
    "",
    "container_t",
    "container_init_t",
    "container_kvm_t",
    "container_engine_t",
];

/// Normalize an image reference so registry prefixes can be compared:
/// `nginx` → `docker.io/library/nginx`, `user/app:1` → `docker.io/user/app:1`.
pub fn normalize_image(image: &str) -> String {
    let image = image.trim();
    let (first, rest) = match image.split_once('/') {
        Some(parts) => parts,
        None => return format!("docker.io/library/{image}"),
    };
    let is_registry = first.contains('.') || first.contains(':') || first == "localhost";
    if !is_registry {
        return format!("docker.io/{image}");
    }
    let registry = match first {
        "index.docker.io" | "registry-1.docker.io" => "docker.io",
        other => other,
    };
    if registry == "docker.io" && !rest.contains('/') {
        return format!("docker.io/library/{rest}");
    }
    format!("{registry}/{rest}")
}

fn is_true(v: &Value, key: &str) -> bool {
    v.get(key).and_then(Value::as_bool) == Some(true)
}

fn str_at<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

fn items<'a>(v: &'a Value, key: &str) -> &'a [Value] {
    v.get(key)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

/// Check a pod spec (`spec` of a Pod, or `template.spec` of a workload) and,
/// if given, the pod (template) metadata for security annotations. `path` is
/// the JSON path of `spec` within the checked object, for messages.
pub fn check_pod_spec(
    policy: &WorkloadPolicy,
    spec: &Value,
    metadata: Option<&Value>,
    path: &str,
    out: &mut Vec<Violation>,
) {
    for key in ["hostNetwork", "hostPID", "hostIPC"] {
        if is_true(spec, key) && !policy.allow_host_namespaces {
            out.push(Violation::new(
                format!("{path}.{key}"),
                format!("{key}: true shares the node's namespaces with the pod"),
                Remedy::Enable(&flags::ALLOW_HOST_NAMESPACES),
            ));
        }
    }

    if let Some(sc) = spec.get("securityContext") {
        check_security_options(policy, sc, &format!("{path}.securityContext"), out);
        for sysctl in items(sc, "sysctls") {
            let name = str_at(sysctl, "name").unwrap_or_default();
            if !SAFE_SYSCTLS.contains(&name) && !policy.allow_unconfined {
                out.push(Violation::new(
                    format!("{path}.securityContext.sysctls"),
                    format!("sysctl {name:?} is not in the safe set"),
                    Remedy::Enable(&flags::ALLOW_UNCONFINED),
                ));
            }
        }
    }

    for (i, volume) in items(spec, "volumes").iter().enumerate() {
        check_volume(policy, volume, &format!("{path}.volumes[{i}]"), out);
    }

    for key in ["initContainers", "containers", "ephemeralContainers"] {
        for (i, container) in items(spec, key).iter().enumerate() {
            let name = str_at(container, "name").unwrap_or("?");
            check_container(
                policy,
                container,
                &format!("{path}.{key}[{i}] ({name})"),
                out,
            );
        }
    }

    if let Some(annotations) = metadata
        .and_then(|m| m.get("annotations"))
        .and_then(Value::as_object)
    {
        let meta_path = path
            .strip_suffix(".spec")
            .map(|p| format!("{p}.metadata.annotations"))
            .unwrap_or_else(|| "metadata.annotations".to_string());
        for (key, value) in annotations {
            let legacy_profile = key.starts_with("container.apparmor.security.beta.kubernetes.io/")
                || key == "seccomp.security.alpha.kubernetes.io/pod"
                || key.starts_with("container.seccomp.security.alpha.kubernetes.io/");
            if legacy_profile
                && value
                    .as_str()
                    .is_some_and(|v| v.eq_ignore_ascii_case("unconfined"))
                && !policy.allow_unconfined
            {
                out.push(Violation::new(
                    format!("{meta_path}[{key:?}]"),
                    "an unconfined security profile annotation turns off seccomp/AppArmor confinement",
                    Remedy::Enable(&flags::ALLOW_UNCONFINED),
                ));
            }
        }
    }
}

/// Check one volume.
fn check_volume(policy: &WorkloadPolicy, volume: &Value, path: &str, out: &mut Vec<Violation>) {
    let Some(map) = volume.as_object() else {
        return;
    };
    let name = map.get("name").and_then(Value::as_str).unwrap_or("?");
    for source in map.keys().filter(|k| k.as_str() != "name") {
        if policy.volume_type_allowed(source) {
            continue;
        }
        if source == "hostPath" {
            let host_path = map
                .get("hostPath")
                .and_then(|h| h.get("path"))
                .and_then(Value::as_str)
                .unwrap_or("?");
            out.push(Violation::new(
                format!("{path} ({name})"),
                format!("hostPath volume {host_path:?} exposes the node's filesystem"),
                Remedy::Enable(&flags::ALLOW_HOST_PATH),
            ));
        } else {
            out.push(Violation::new(
                format!("{path} ({name})"),
                format!(
                    "volume type {source} is not in the safe set ({})",
                    SAFE_VOLUME_TYPES.join(", ")
                ),
                Remedy::AddTo(&flags::ALLOW_VOLUME_TYPES, source.clone()),
            ));
        }
    }
}

/// Check one container (also used for container-shaped objects embedded in
/// custom resources).
pub(crate) fn check_container(
    policy: &WorkloadPolicy,
    container: &Value,
    path: &str,
    out: &mut Vec<Violation>,
) {
    if let Some(image) = str_at(container, "image") {
        if !policy.image_allowed(image) {
            out.push(Violation::new(
                path,
                format!(
                    "image {image:?} is not from an allowed registry (allowed: {})",
                    policy.allowed_registries.join(", ")
                ),
                Remedy::AddTo(&flags::ALLOWED_REGISTRIES, registry_of(image)),
            ));
        }
    }

    if let Some(sc) = container.get("securityContext") {
        let sc_path = format!("{path}.securityContext");
        if is_true(sc, "privileged") && !policy.allow_privileged {
            out.push(Violation::new(
                &sc_path,
                "privileged: true gives the container full access to the node",
                Remedy::Enable(&flags::ALLOW_PRIVILEGED),
            ));
        }
        if is_true(sc, "allowPrivilegeEscalation") && !policy.allow_privilege_escalation {
            out.push(Violation::new(
                &sc_path,
                "allowPrivilegeEscalation: true lets processes gain more privileges than their parent",
                Remedy::Enable(&flags::ALLOW_PRIVILEGE_ESCALATION),
            ));
        }
        if let Some(caps) = sc.get("capabilities") {
            for cap in items(caps, "add").iter().filter_map(Value::as_str) {
                if !policy.capability_allowed(cap) {
                    out.push(Violation::new(
                        format!("{sc_path}.capabilities.add"),
                        format!(
                            "capability {} is not in the allowed set ({})",
                            normalize_capability(cap),
                            BASELINE_CAPABILITIES.join(", ")
                        ),
                        Remedy::AddTo(&flags::ALLOW_CAPABILITIES, normalize_capability(cap)),
                    ));
                }
            }
        }
        if str_at(sc, "procMount") == Some("Unmasked") && !policy.allow_unconfined {
            out.push(Violation::new(
                &sc_path,
                "procMount: Unmasked exposes the host's /proc",
                Remedy::Enable(&flags::ALLOW_UNCONFINED),
            ));
        }
        check_security_options(policy, sc, &sc_path, out);
    }

    for (i, port) in items(container, "ports").iter().enumerate() {
        let host_port = port.get("hostPort").and_then(Value::as_i64).unwrap_or(0);
        if host_port != 0 && !policy.allow_host_ports {
            out.push(Violation::new(
                format!("{path}.ports[{i}]"),
                format!("hostPort {host_port} binds a port on the node"),
                Remedy::Enable(&flags::ALLOW_HOST_PORTS),
            ));
        }
    }

    for probe in ["livenessProbe", "readinessProbe", "startupProbe"] {
        if let Some(handler) = container.get(probe) {
            check_handler_host(policy, handler, &format!("{path}.{probe}"), out);
        }
    }
    if let Some(lifecycle) = container.get("lifecycle") {
        for hook in ["postStart", "preStop"] {
            if let Some(handler) = lifecycle.get(hook) {
                check_handler_host(policy, handler, &format!("{path}.lifecycle.{hook}"), out);
            }
        }
    }
}

/// Rules shared by pod- and container-level security contexts.
fn check_security_options(
    policy: &WorkloadPolicy,
    sc: &Value,
    path: &str,
    out: &mut Vec<Violation>,
) {
    if policy.allow_unconfined && policy.allow_privileged {
        return;
    }
    for key in ["seccompProfile", "appArmorProfile"] {
        if sc.get(key).and_then(|p| str_at(p, "type")) == Some("Unconfined")
            && !policy.allow_unconfined
        {
            out.push(Violation::new(
                format!("{path}.{key}"),
                format!("{key}.type: Unconfined turns off syscall/LSM confinement"),
                Remedy::Enable(&flags::ALLOW_UNCONFINED),
            ));
        }
    }
    if let Some(se) = sc.get("seLinuxOptions") {
        let custom_type = !SAFE_SELINUX_TYPES.contains(&str_at(se, "type").unwrap_or(""));
        let custom_identity = ["user", "role"]
            .iter()
            .any(|k| str_at(se, k).is_some_and(|v| !v.is_empty()));
        if (custom_type || custom_identity) && !policy.allow_unconfined {
            out.push(Violation::new(
                format!("{path}.seLinuxOptions"),
                "custom SELinux type, user or role",
                Remedy::Enable(&flags::ALLOW_UNCONFINED),
            ));
        }
    }
    if sc
        .get("windowsOptions")
        .is_some_and(|w| is_true(w, "hostProcess"))
        && !policy.allow_privileged
    {
        out.push(Violation::new(
            format!("{path}.windowsOptions"),
            "hostProcess: true runs a privileged Windows host process",
            Remedy::Enable(&flags::ALLOW_PRIVILEGED),
        ));
    }
}

/// Probes and lifecycle hooks run from the kubelet: a `host` other than the
/// pod itself makes the node send requests anywhere (SSRF).
fn check_handler_host(
    policy: &WorkloadPolicy,
    handler: &Value,
    path: &str,
    out: &mut Vec<Violation>,
) {
    if policy.allow_unconfined {
        return;
    }
    for key in ["httpGet", "tcpSocket"] {
        if let Some(host) = handler.get(key).and_then(|h| str_at(h, "host")) {
            if !matches!(host, "" | "localhost" | "127.0.0.1" | "::1") {
                out.push(Violation::new(
                    format!("{path}.{key}.host"),
                    format!("host {host:?} makes the kubelet connect to another address"),
                    Remedy::Enable(&flags::ALLOW_UNCONFINED),
                ));
            }
        }
    }
}

/// The registry part of an image reference, for remedy hints.
fn registry_of(image: &str) -> String {
    let normalized = normalize_image(image);
    match normalized.split_once('/') {
        Some(("docker.io", rest)) => {
            // docker.io/library/nginx → docker.io/library
            let org = rest.split('/').next().unwrap_or("library");
            format!("docker.io/{org}")
        }
        Some((registry, _)) => registry.to_string(),
        None => normalized,
    }
}
