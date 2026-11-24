//! RBAC, service-account token and namespace security-label rules.

use serde_json::Value;

use super::{flags, Remedy, Violation, WorkloadPolicy};

/// Verbs that let a subject grant itself (or others) more than it has.
const ESCALATION_VERBS: &[&str] = &["escalate", "bind", "impersonate"];

/// Subjects that must never receive grants from an agent.
const ANONYMOUS_SUBJECTS: &[&str] = &["system:anonymous", "system:unauthenticated"];

fn strings<'a>(v: &'a Value, key: &str) -> Vec<&'a str> {
    v.get(key)
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default()
}

fn escalation(location: String, reason: String) -> Violation {
    Violation::new(location, reason, Remedy::Enable(&flags::ALLOW_RBAC_ESCALATION))
}

/// Check RBAC policy rules (the Kubernetes JSON form, `apiGroups`,
/// `resources`, `verbs`, `nonResourceURLs`). `path` is where the rules live,
/// e.g. `rules`.
pub fn check_rules(policy: &WorkloadPolicy, rules: &[Value], path: &str, out: &mut Vec<Violation>) {
    if policy.allow_rbac_escalation {
        return;
    }
    for (i, rule) in rules.iter().enumerate() {
        let loc = format!("{path}[{i}]");
        let verbs = strings(rule, "verbs");
        if verbs.contains(&"*") {
            out.push(escalation(loc.clone(), "wildcard verb \"*\" grants every verb, including escalate, bind and impersonate".into()));
        }
        for verb in verbs.iter().filter(|v| ESCALATION_VERBS.contains(v)) {
            out.push(escalation(loc.clone(), format!("verb {verb:?} is a privilege escalation primitive")));
        }
        if strings(rule, "apiGroups").contains(&"*") {
            out.push(escalation(loc.clone(), "wildcard apiGroup \"*\" covers every API group, including RBAC".into()));
        }
        if let Some(r) = strings(rule, "resources")
            .into_iter()
            .find(|r| *r == "*" || r.ends_with("/*"))
        {
            out.push(escalation(loc.clone(), format!("wildcard resource {r:?} covers resources and subresources nobody reviewed")));
        }
        if let Some(u) = strings(rule, "nonResourceURLs")
            .into_iter()
            .find(|u| *u == "*" || u.ends_with('*'))
        {
            out.push(escalation(loc.clone(), format!("wildcard nonResourceURL {u:?}")));
        }
    }
}

/// Extra ClusterRole rules: aggregation pulls in rules nobody reviewed, and the
/// `aggregate-to-*` labels push this role's rules into the built-in
/// admin/edit/view roles.
pub(crate) fn check_cluster_role_extras(policy: &WorkloadPolicy, role: &Value, out: &mut Vec<Violation>) {
    if policy.allow_rbac_escalation {
        return;
    }
    if role.get("aggregationRule").is_some_and(|a| !a.is_null()) {
        out.push(escalation(
            "aggregationRule".into(),
            "an aggregated ClusterRole inherits the rules of every matching role".into(),
        ));
    }
    if let Some(labels) = role
        .get("metadata")
        .and_then(|m| m.get("labels"))
        .and_then(Value::as_object)
    {
        for key in labels.keys() {
            if key.starts_with("rbac.authorization.k8s.io/aggregate-to-") {
                out.push(escalation(
                    format!("metadata.labels[{key:?}]"),
                    "this label adds the role's rules to a built-in role every binding of it receives".into(),
                ));
            }
        }
    }
}

/// Check a RoleBinding or ClusterRoleBinding (Kubernetes JSON form): binding
/// `cluster-admin`, or binding anything to anonymous subjects.
pub fn check_binding(policy: &WorkloadPolicy, binding: &Value, out: &mut Vec<Violation>) {
    if policy.allow_rbac_escalation {
        return;
    }
    let role_ref = binding.get("roleRef").cloned().unwrap_or(Value::Null);
    let ref_kind = role_ref.get("kind").and_then(Value::as_str).unwrap_or("");
    let ref_name = role_ref.get("name").and_then(Value::as_str).unwrap_or("");
    if ref_kind == "ClusterRole" && ref_name == "cluster-admin" {
        out.push(escalation(
            "roleRef".into(),
            "binding cluster-admin grants unrestricted access".into(),
        ));
    }
    let subjects = binding
        .get("subjects")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for (i, subject) in subjects.iter().enumerate() {
        let name = subject.get("name").and_then(Value::as_str).unwrap_or("");
        if ANONYMOUS_SUBJECTS.contains(&name) {
            out.push(escalation(
                format!("subjects[{i}]"),
                format!("binding {name} grants the role to unauthenticated requests"),
            ));
        }
    }
}

/// Check the rules of the role a binding refers to (fetched by the caller).
pub(crate) fn check_bound_role(
    policy: &WorkloadPolicy,
    role_kind: &str,
    role_name: &str,
    role: &Value,
    out: &mut Vec<Violation>,
) {
    if policy.allow_rbac_escalation {
        return;
    }
    let mut inner = Vec::new();
    let rules = role
        .get("rules")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    check_rules(policy, &rules, "rules", &mut inner);
    if role_kind == "ClusterRole" {
        check_cluster_role_extras(policy, role, &mut inner);
    }
    for v in inner {
        out.push(Violation::new(
            "roleRef",
            format!("the bound {role_kind} {role_name:?} has an escalation rule ({}: {})", v.location, v.reason),
            v.remedy,
        ));
    }
}

/// Service-account token Secrets mint a long-lived credential for any service
/// account in the namespace.
pub fn check_secret(policy: &WorkloadPolicy, secret: &Value, out: &mut Vec<Violation>) {
    if policy.allow_rbac_escalation {
        return;
    }
    if secret.get("type").and_then(Value::as_str) == Some("kubernetes.io/service-account-token") {
        let sa = secret
            .get("metadata")
            .and_then(|m| m.get("annotations"))
            .and_then(|a| a.get("kubernetes.io/service-account.name"))
            .and_then(Value::as_str)
            .unwrap_or("?");
        out.push(escalation(
            "type".into(),
            format!("a kubernetes.io/service-account-token Secret mints a long-lived token for service account {sa:?}"),
        ));
    }
}

/// Labelling a namespace `pod-security.kubernetes.io/<mode>=privileged` turns
/// off Pod Security Admission there for everyone.
pub fn check_namespace_labels(policy: &WorkloadPolicy, labels: &Value, out: &mut Vec<Violation>) {
    if policy.allow_privileged {
        return;
    }
    let Some(labels) = labels.as_object() else {
        return;
    };
    for mode in ["enforce", "audit", "warn"] {
        let key = format!("pod-security.kubernetes.io/{mode}");
        if labels.get(&key).and_then(Value::as_str) == Some("privileged") {
            out.push(Violation::new(
                format!("metadata.labels[{key:?}]"),
                "the privileged Pod Security level turns off pod security admission in this namespace",
                Remedy::Enable(&flags::ALLOW_PRIVILEGED),
            ));
        }
    }
}
