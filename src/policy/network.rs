//! Service and Ingress rules.

use serde_json::Value;

use super::{flags, pattern_matches, Remedy, Violation, WorkloadPolicy};

/// Annotation prefixes that configure cloud or bare-metal load balancers.
const LOAD_BALANCER_ANNOTATION_PREFIXES: &[&str] = &[
    "service.beta.kubernetes.io/",
    "service.kubernetes.io/ibm-load-balancer",
    "cloud.google.com/load-balancer",
    "networking.gke.io/load-balancer",
    "networking.gke.io/internal-load-balancer",
    "metallb.universe.tf/",
    "metallb.io/",
    "kube-vip.io/",
    "lbipam.cilium.io/",
];

/// `spec` fields that only matter for (and configure) load balancers.
const LOAD_BALANCER_FIELDS: &[&str] = &[
    "loadBalancerIP",
    "loadBalancerSourceRanges",
    "loadBalancerClass",
    "allocateLoadBalancerNodePorts",
];

/// Characters that let an ingress-nginx annotation value break out of the
/// directive it is templated into (IngressNightmare: CVE-2025-24514,
/// CVE-2025-1097, CVE-2025-1098).
const NGINX_VALUE_FORBIDDEN_CHARS: &[char] = &['\n', '\r', ';', '{', '}', '\'', '"', '\\', '`'];

/// Words ingress-nginx's own `annotation-value-word-blocklist` rejects that
/// have no business in an annotation value.
const NGINX_VALUE_FORBIDDEN_WORDS: &[&str] = &[
    "load_module",
    "lua_package",
    "_by_lua",
    "proxy_pass",
    "serviceaccount",
];

/// ingress-nginx annotations whose value is a URL templated into the config.
const NGINX_URL_ANNOTATIONS: &[&str] = &[
    "auth-url",
    "auth-signin",
    "mirror-target",
    "permanent-redirect",
    "temporal-redirect",
];

const NGINX_PREFIX: &str = "nginx.ingress.kubernetes.io/";

fn str_at<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

/// Check a Service (its Kubernetes JSON form).
pub fn check_service(policy: &WorkloadPolicy, service: &Value, out: &mut Vec<Violation>) {
    let spec = service.get("spec").cloned().unwrap_or(Value::Null);
    match str_at(&spec, "type").unwrap_or("ClusterIP") {
        "NodePort" if !policy.allow_node_port => out.push(Violation::new(
            "spec.type",
            "NodePort Services open a port on every node",
            Remedy::Enable(&flags::ALLOW_NODEPORT),
        )),
        "LoadBalancer" if !policy.allow_load_balancer => out.push(Violation::new(
            "spec.type",
            "LoadBalancer Services provision an externally reachable load balancer",
            Remedy::Enable(&flags::ALLOW_LOADBALANCER),
        )),
        "ExternalName" if !policy.allow_external_services => out.push(Violation::new(
            "spec.type",
            "ExternalName Services point cluster DNS names at arbitrary hosts",
            Remedy::Enable(&flags::ALLOW_EXTERNAL_SERVICES),
        )),
        _ => {}
    }

    let ports = spec.get("ports").and_then(Value::as_array).cloned().unwrap_or_default();
    for (i, port) in ports.iter().enumerate() {
        let node_port = port.get("nodePort").and_then(Value::as_i64).unwrap_or(0);
        if node_port != 0 && !policy.allow_node_port && !policy.allow_load_balancer {
            out.push(Violation::new(
                format!("spec.ports[{i}].nodePort"),
                format!("nodePort {node_port} opens a port on every node"),
                Remedy::Enable(&flags::ALLOW_NODEPORT),
            ));
        }
    }

    if spec
        .get("externalIPs")
        .and_then(Value::as_array)
        .is_some_and(|ips| !ips.is_empty())
        && !policy.allow_external_services
    {
        out.push(Violation::new(
            "spec.externalIPs",
            "externalIPs can intercept traffic to arbitrary addresses (CVE-2020-8554)",
            Remedy::Enable(&flags::ALLOW_EXTERNAL_SERVICES),
        ));
    }

    if !policy.allow_load_balancer {
        for field in LOAD_BALANCER_FIELDS {
            if spec.get(*field).is_some_and(|v| !v.is_null()) {
                out.push(Violation::new(
                    format!("spec.{field}"),
                    "load balancer settings are only allowed when LoadBalancer Services are",
                    Remedy::Enable(&flags::ALLOW_LOADBALANCER),
                ));
            }
        }
        if let Some(annotations) = service
            .get("metadata")
            .and_then(|m| m.get("annotations"))
            .and_then(Value::as_object)
        {
            for key in annotations.keys() {
                if LOAD_BALANCER_ANNOTATION_PREFIXES.iter().any(|p| key.starts_with(p)) {
                    out.push(Violation::new(
                        format!("metadata.annotations[{key:?}]"),
                        "load balancer annotations are only allowed when LoadBalancer Services are",
                        Remedy::Enable(&flags::ALLOW_LOADBALANCER),
                    ));
                }
            }
        }
    }
}

/// Check an Ingress (its Kubernetes JSON form).
pub fn check_ingress(policy: &WorkloadPolicy, ingress: &Value, out: &mut Vec<Violation>) {
    if let Some(annotations) = ingress
        .get("metadata")
        .and_then(|m| m.get("annotations"))
        .and_then(Value::as_object)
    {
        for (key, value) in annotations {
            check_ingress_annotation(policy, key, value.as_str().unwrap_or(""), out);
        }
    }

    let rules = ingress
        .get("spec")
        .and_then(|s| s.get("rules"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for (i, rule) in rules.iter().enumerate() {
        let paths = rule
            .get("http")
            .and_then(|h| h.get("paths"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for (j, path) in paths.iter().enumerate() {
            let p = str_at(path, "path").unwrap_or("");
            let path_type = str_at(path, "pathType").unwrap_or("ImplementationSpecific");
            // Regex paths (ImplementationSpecific) may use {n,m} quantifiers.
            let braces_ok = path_type == "ImplementationSpecific";
            let bad = p.chars().any(|c| {
                c.is_whitespace()
                    || matches!(c, ';' | '\'' | '"' | '\\' | '`')
                    || (!braces_ok && matches!(c, '{' | '}'))
            });
            if bad && !policy.allow_ingress_snippets {
                out.push(Violation::new(
                    format!("spec.rules[{i}].http.paths[{j}].path"),
                    format!("path {p:?} contains characters that can inject proxy configuration"),
                    Remedy::Enable(&flags::ALLOW_INGRESS_SNIPPETS),
                ));
            }
        }
    }
}

fn check_ingress_annotation(policy: &WorkloadPolicy, key: &str, value: &str, out: &mut Vec<Violation>) {
    let location = format!("metadata.annotations[{key:?}]");

    if !policy.ingress_annotation_allowlist.is_empty()
        && !policy
            .ingress_annotation_allowlist
            .iter()
            .any(|p| pattern_matches(p, key))
    {
        out.push(Violation::new(
            &location,
            "annotation is not in the ingress annotation allowlist",
            Remedy::AddTo(&flags::INGRESS_ANNOTATION_ALLOWLIST, key.to_string()),
        ));
    }
    if let Some(pattern) = policy
        .ingress_annotation_denylist
        .iter()
        .find(|p| pattern_matches(p, key))
    {
        out.push(Violation::new(
            &location,
            "annotation is in the ingress annotation denylist",
            Remedy::RemoveFrom(&flags::INGRESS_ANNOTATION_DENYLIST, pattern.clone()),
        ));
    }

    if policy.allow_ingress_snippets {
        return;
    }
    let name = key.rsplit('/').next().unwrap_or(key);
    if name.ends_with("-snippet") || name.ends_with("-snippets") || name == "snippet" {
        out.push(Violation::new(
            &location,
            "snippet annotations inject raw proxy configuration (IngressNightmare)",
            Remedy::Enable(&flags::ALLOW_INGRESS_SNIPPETS),
        ));
        return;
    }
    let Some(nginx_name) = key.strip_prefix(NGINX_PREFIX) else {
        return;
    };
    if let Some(c) = value.chars().find(|c| NGINX_VALUE_FORBIDDEN_CHARS.contains(c)) {
        out.push(Violation::new(
            &location,
            format!("value contains {c:?}, which can inject nginx configuration"),
            Remedy::Enable(&flags::ALLOW_INGRESS_SNIPPETS),
        ));
        return;
    }
    let lower = value.to_ascii_lowercase();
    if let Some(word) = NGINX_VALUE_FORBIDDEN_WORDS.iter().find(|w| lower.contains(*w)) {
        out.push(Violation::new(
            &location,
            format!("value contains {word:?}, which ingress-nginx blocks in annotation values"),
            Remedy::Enable(&flags::ALLOW_INGRESS_SNIPPETS),
        ));
        return;
    }
    if NGINX_URL_ANNOTATIONS.contains(&nginx_name) && !is_plain_http_url(value) {
        out.push(Violation::new(
            &location,
            "value must be a plain http(s) URL without whitespace or '#'",
            Remedy::Enable(&flags::ALLOW_INGRESS_SNIPPETS),
        ));
    }
}

/// `http(s)://host[...]` with no whitespace, control characters or `#`.
fn is_plain_http_url(value: &str) -> bool {
    let rest = value
        .strip_prefix("https://")
        .or_else(|| value.strip_prefix("http://"));
    match rest {
        Some(rest) => {
            !rest.is_empty()
                && !rest.starts_with('/')
                && !value.chars().any(|c| c.is_whitespace() || c.is_control() || c == '#')
        }
        None => false,
    }
}
