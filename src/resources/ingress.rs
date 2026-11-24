use std::collections::BTreeMap;

use k8s_openapi::api::networking::v1::{
    HTTPIngressPath, HTTPIngressRuleValue, Ingress, IngressBackend, IngressRule,
    IngressServiceBackend, IngressSpec, IngressTLS, ServiceBackendPort,
};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::api::{DeleteParams, PostParams};
use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::client::K8sClient;
use crate::extract::ingress_detail;
use crate::policy::{PatchKind, Source, MANAGED_BY_LABEL, MANAGED_BY_VALUE};
use crate::resources::podspec;
use crate::types::IngressDetail;

fn api(client: &K8sClient, ns: &str) -> Result<kube::Api<Ingress>, String> {
    if !client.is_namespace_allowed(ns) {
        return Err(format!("Namespace '{ns}' is not in the allowed list"));
    }
    Ok(kube::Api::namespaced(client.inner().clone(), ns))
}

fn backend_properties() -> Value {
    json!({
        "service_name": { "type": "string", "description": "Backend Service name" },
        "service_port": { "type": "integer", "description": "Backend Service port number (default: 80)" },
        "service_port_name": { "type": "string", "description": "Backend Service port name (instead of service_port)" }
    })
}

fn ingress_properties(create: bool) -> Map<String, Value> {
    let mut path_props = backend_properties();
    path_props["path"] = json!({ "type": "string", "description": "URL path (default: /)" });
    path_props["path_type"] = json!({ "type": "string", "enum": ["Prefix", "Exact", "ImplementationSpecific"], "description": "Default: Prefix" });
    let mut props = json!({
        "namespace": { "type": "string", "description": "Kubernetes namespace" },
        "name": { "type": "string", "description": "Ingress name" },
        "ingress_class": { "type": "string", "description": "IngressClass name (ingressClassName), e.g. nginx, alb, traefik" },
        "rules": {
            "type": "array",
            "description": "Routing rules (replace all rules on update)",
            "items": {
                "type": "object",
                "properties": {
                    "host": { "type": "string", "description": "Hostname, e.g. app.example.com (omit for any host)" },
                    "paths": {
                        "type": "array",
                        "items": { "type": "object", "properties": path_props, "required": ["service_name"], "additionalProperties": false }
                    }
                },
                "required": ["paths"],
                "additionalProperties": false
            }
        },
        "tls": {
            "type": "array",
            "description": "TLS termination: hosts covered by a certificate Secret in the same namespace",
            "items": {
                "type": "object",
                "properties": {
                    "hosts": { "type": "array", "items": { "type": "string" } },
                    "secret_name": { "type": "string", "description": "kubernetes.io/tls Secret with the certificate" }
                },
                "additionalProperties": false
            }
        },
        "default_backend": { "type": "object", "description": "Backend for requests no rule matches", "properties": backend_properties(), "required": ["service_name"], "additionalProperties": false },
        "annotations": { "type": "object", "description": "Ingress annotations. *-snippet annotations and nginx config syntax in ingress-nginx values are rejected unless the server allows them (--allow-ingress-snippets); the operator may also set an annotation allowlist/denylist", "additionalProperties": { "type": "string" } },
        "labels": { "type": "object", "additionalProperties": { "type": "string" } },
        "host": { "type": "string", "description": "Shorthand for a single rule: hostname" },
        "service_name": { "type": "string", "description": "Shorthand for a single rule: backend Service" },
        "service_port": { "type": "integer", "description": "Shorthand for a single rule: backend port (default: 80)" },
        "path": { "type": "string", "description": "Shorthand for a single rule: path (default: /)" },
        "path_type": { "type": "string", "enum": ["Prefix", "Exact", "ImplementationSpecific"], "description": "Shorthand for a single rule: path type (default: Prefix)" }
    });
    if create {
        props["create_missing_services"] = json!({ "type": "boolean", "description": "Create a ClusterIP Service (selecting app=<service_name>) for each backend that doesn't exist yet (default: true)" });
    }
    props.as_object().cloned().unwrap_or_default()
}

fn create_ingress_schema() -> Value {
    json!({ "type": "object", "properties": ingress_properties(true), "required": ["namespace", "name"], "additionalProperties": false })
}

fn update_ingress_schema() -> Value {
    json!({ "type": "object", "properties": ingress_properties(false), "required": ["namespace", "name"], "additionalProperties": false })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BackendArgs {
    #[serde(alias = "serviceName")]
    service_name: String,
    #[serde(alias = "servicePort")]
    service_port: Option<i32>,
    #[serde(alias = "servicePortName")]
    service_port_name: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PathArgs {
    path: Option<String>,
    #[serde(alias = "pathType")]
    path_type: Option<String>,
    #[serde(alias = "serviceName")]
    service_name: String,
    #[serde(alias = "servicePort")]
    service_port: Option<i32>,
    #[serde(alias = "servicePortName")]
    service_port_name: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RuleArgs {
    host: Option<String>,
    paths: Vec<PathArgs>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TlsArgs {
    hosts: Option<Vec<String>>,
    #[serde(alias = "secretName")]
    secret_name: Option<String>,
}

fn backend(name: String, port: Option<i32>, port_name: Option<String>, what: &str) -> Result<IngressBackend, String> {
    let port = match (port, port_name) {
        (Some(_), Some(_)) => {
            return Err(format!("{what}: set service_port or service_port_name, not both"))
        }
        (_, Some(n)) => ServiceBackendPort {
            name: Some(n),
            number: None,
        },
        (p, None) => ServiceBackendPort {
            number: Some(p.unwrap_or(80)),
            name: None,
        },
    };
    Ok(IngressBackend {
        service: Some(IngressServiceBackend {
            name,
            port: Some(port),
        }),
        ..Default::default()
    })
}

fn path_type(v: Option<String>, what: &str) -> Result<String, String> {
    let v = v.unwrap_or_else(|| "Prefix".to_string());
    podspec::check_enum(&v, &["Prefix", "Exact", "ImplementationSpecific"], what)?;
    Ok(v)
}

const SHORTHAND_KEYS: &[&str] = &["host", "service_name", "service_port", "path", "path_type"];

/// `rules`, or the single-rule shorthand. `None` when neither is given.
fn rules_from_args(args: &Value) -> Result<Option<Vec<IngressRule>>, String> {
    let shorthand = SHORTHAND_KEYS.iter().any(|k| args.get(*k).is_some());
    let listed = podspec::opt::<Vec<RuleArgs>>(args, "rules")?;
    let rules = match (listed, shorthand) {
        (Some(_), true) => {
            return Err("use either rules or the single-rule shorthand (host, service_name, service_port, path, path_type), not both".to_string())
        }
        (Some(rules), false) => rules,
        (None, true) => vec![RuleArgs {
            host: args["host"].as_str().map(str::to_string),
            paths: vec![PathArgs {
                path: args["path"].as_str().map(str::to_string),
                path_type: args["path_type"].as_str().map(str::to_string),
                service_name: args["service_name"]
                    .as_str()
                    .ok_or("service_name is required for the single-rule shorthand")?
                    .to_string(),
                service_port: args["service_port"].as_i64().and_then(|p| i32::try_from(p).ok()),
                service_port_name: None,
            }],
        }],
        (None, false) => return Ok(None),
    };
    rules
        .into_iter()
        .enumerate()
        .map(|(i, r)| {
            let paths = r
                .paths
                .into_iter()
                .enumerate()
                .map(|(j, p)| {
                    let what = format!("rules[{i}].paths[{j}]");
                    Ok(HTTPIngressPath {
                        path: Some(p.path.unwrap_or_else(|| "/".to_string())),
                        path_type: path_type(p.path_type, &format!("{what}.path_type"))?,
                        backend: backend(p.service_name, p.service_port, p.service_port_name, &what)?,
                    })
                })
                .collect::<Result<Vec<_>, String>>()?;
            if paths.is_empty() {
                return Err(format!("rules[{i}].paths must not be empty"));
            }
            Ok(IngressRule {
                host: r.host,
                http: Some(HTTPIngressRuleValue { paths }),
            })
        })
        .collect::<Result<Vec<_>, String>>()
        .map(Some)
}

fn tls_from_args(args: &Value) -> Result<Option<Vec<IngressTLS>>, String> {
    Ok(podspec::opt::<Vec<TlsArgs>>(args, "tls")?.map(|items| {
        items
            .into_iter()
            .map(|t| IngressTLS {
                hosts: t.hosts,
                secret_name: t.secret_name,
            })
            .collect()
    }))
}

fn default_backend_from_args(args: &Value) -> Result<Option<IngressBackend>, String> {
    podspec::opt::<BackendArgs>(args, "default_backend")?
        .map(|b| backend(b.service_name, b.service_port, b.service_port_name, "default_backend"))
        .transpose()
}

/// Build the Ingress `create_ingress` creates.
fn ingress_from_args(args: &Value) -> Result<Ingress, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;
    let rules = rules_from_args(args)?;
    let default_backend = default_backend_from_args(args)?;
    if rules.is_none() && default_backend.is_none() {
        return Err("give rules, default_backend, or service_name (single-rule shorthand)".to_string());
    }
    let mut labels = podspec::string_map(args, "labels")?.unwrap_or_default();
    labels.insert(MANAGED_BY_LABEL.to_string(), MANAGED_BY_VALUE.to_string());
    Ok(Ingress {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            namespace: Some(ns.to_string()),
            labels: Some(labels),
            annotations: podspec::string_map(args, "annotations")?,
            ..Default::default()
        },
        spec: Some(IngressSpec {
            ingress_class_name: args["ingress_class"].as_str().map(str::to_string),
            rules,
            tls: tls_from_args(args)?,
            default_backend,
        }),
        ..Default::default()
    })
}

/// Backend services with a numeric port, for create_missing_services.
fn numbered_backends(ingress: &Ingress) -> BTreeMap<String, i32> {
    let mut out = BTreeMap::new();
    let spec = ingress.spec.as_ref();
    let mut add = |b: &IngressBackend| {
        if let Some(svc) = &b.service {
            if let Some(n) = svc.port.as_ref().and_then(|p| p.number) {
                out.entry(svc.name.clone()).or_insert(n);
            }
        }
    };
    if let Some(b) = spec.and_then(|s| s.default_backend.as_ref()) {
        add(b);
    }
    for rule in spec.and_then(|s| s.rules.as_ref()).into_iter().flatten() {
        for path in rule.http.iter().flat_map(|h| h.paths.iter()) {
            add(&path.backend);
        }
    }
    out
}

pub fn tool_definitions() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "name": "create_ingress",
            "description": "Create an Ingress. Preferred over apply_manifest: arguments are validated and the server's guardrails are enforced. Supports several rules (host + paths with path_type, backend by port number or name), TLS (hosts + secret_name), a default backend and the ingress class. A single-rule shorthand (host, service_name, service_port, path, path_type) is also accepted. By default a ClusterIP Service selecting app=<service_name> is created for each backend that doesn't exist yet (create_missing_services). Snippet annotations and nginx configuration syntax in ingress-nginx annotation values are rejected unless the server operator allows them (IngressNightmare).",
            "inputSchema": create_ingress_schema()
        }),
        serde_json::json!({
            "name": "update_ingress",
            "description": "Update (patch) an Ingress: rules (replaced), tls (replaced), default_backend, ingress_class, annotations and labels (merged). Preferred over apply_manifest. The single-rule shorthand (host, service_name, service_port, path, path_type) replaces all rules with one. The result is checked against the server's guardrails with a server-side dry run before it is written.",
            "inputSchema": update_ingress_schema()
        }),
        serde_json::json!({
            "name": "get_ingress",
            "description": "Get detailed info for a single ingress by namespace and name. Returns hosts, ingress class, rules, TLS, annotations, and addresses.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "Ingress name" }
                },
                "required": ["namespace", "name"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "delete_ingress",
            "description": "Delete an ingress by name from a namespace.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "Ingress name" }
                },
                "required": ["namespace", "name"],
                "additionalProperties": false
            }
        }),
    ]
}

pub async fn handle_tool(
    client: &K8sClient,
    name: &str,
    args: &serde_json::Value,
) -> Option<Result<String, String>> {
    let result = match name {
        "get_ingress" => get_ingress(client, args).await,
        "create_ingress" => create_ingress(client, args).await,
        "update_ingress" => update_ingress(client, args).await,
        "delete_ingress" => delete_ingress(client, args).await,
        _ => return None,
    };
    Some(result)
}

async fn get_ingress(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;

    let ing_api = api(client, ns)?;
    let ing = ing_api.get(name).await.map_err(|e| e.to_string())?;

    let detail: IngressDetail = ingress_detail(&ing);
    serde_json::to_string_pretty(&detail).map_err(|e| e.to_string())
}

async fn create_ingress(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let args = podspec::normalize_args(args, &create_ingress_schema())?;
    let ingress = ingress_from_args(&args)?;
    let ns = ingress.metadata.namespace.clone().unwrap_or_default();
    let ing_api = api(client, &ns)?;
    // Check before creating backing Services, so a rejected Ingress leaves
    // nothing behind.
    crate::policy::check_typed(client, &ingress)?;

    if args["create_missing_services"].as_bool().unwrap_or(true) {
        for (svc, port) in numbered_backends(&ingress) {
            crate::resources::service::ensure_service(client, &ns, &svc, port).await?;
        }
    }

    let created = ing_api
        .create(&PostParams::default(), &ingress)
        .await
        .map_err(|e| e.to_string())?;

    let detail = ingress_detail(&created);
    serde_json::to_string_pretty(&serde_json::to_value(detail).unwrap_or_default())
        .map_err(|e| e.to_string())
}

async fn update_ingress(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let args = podspec::normalize_args(args, &update_ingress_schema())?;
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;

    let mut spec = Map::new();
    if let Some(rules) = rules_from_args(&args)? {
        spec.insert("rules".into(), serde_json::to_value(rules).map_err(|e| e.to_string())?);
    }
    if let Some(tls) = tls_from_args(&args)? {
        spec.insert("tls".into(), serde_json::to_value(tls).map_err(|e| e.to_string())?);
    }
    if let Some(b) = default_backend_from_args(&args)? {
        spec.insert("defaultBackend".into(), serde_json::to_value(b).map_err(|e| e.to_string())?);
    }
    if let Some(class) = args["ingress_class"].as_str() {
        spec.insert("ingressClassName".into(), json!(class));
    }
    let mut patch = Map::new();
    if !spec.is_empty() {
        patch.insert("spec".into(), Value::Object(spec));
    }
    let mut metadata = Map::new();
    for key in ["labels", "annotations"] {
        if let Some(map) = podspec::string_map(&args, key)? {
            metadata.insert(key.into(), json!(map));
        }
    }
    if !metadata.is_empty() {
        patch.insert("metadata".into(), Value::Object(metadata));
    }
    if patch.is_empty() {
        return Err("Nothing to update: give rules (or the single-rule shorthand), tls, default_backend, ingress_class, annotations or labels".to_string());
    }

    let ing_api = api(client, ns)?;
    let patched = crate::policy::patch_checked(
        client,
        &ing_api,
        name,
        Value::Object(patch),
        PatchKind::Strategic,
        Source::Typed,
    )
    .await?;

    let detail = ingress_detail(&patched);
    serde_json::to_string_pretty(&serde_json::to_value(detail).unwrap_or_default())
        .map_err(|e| e.to_string())
}

async fn delete_ingress(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;

    let ing_api = api(client, ns)?;
    ing_api
        .delete(name, &DeleteParams::default())
        .await
        .map_err(|e| e.to_string())?;

    let result = serde_json::json!({
        "deleted": true,
        "name": name,
        "namespace": ns,
    });
    serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::api::networking::v1::{
        HTTPIngressPath, HTTPIngressRuleValue, IngressBackend, IngressRule, IngressServiceBackend,
        IngressSpec, IngressTLS, ServiceBackendPort,
    };
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
    use std::collections::BTreeMap;

    #[test]
    fn tool_definitions_returns_two_unique_tools() {
        let defs = tool_definitions();
        assert_eq!(defs.len(), 2);

        let names: Vec<&str> = defs.iter().map(|d| d["name"].as_str().unwrap()).collect();

        let mut unique = names.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(names.len(), unique.len(), "tool names must be unique");

        assert!(names.contains(&"get_ingress"));
        assert!(names.contains(&"delete_ingress"));
    }

    #[test]
    fn tool_definitions_have_input_schema() {
        let defs = tool_definitions();
        for def in &defs {
            assert!(def.get("name").is_some(), "tool must have a name");
            assert!(
                def.get("description").is_some(),
                "tool must have a description"
            );
            let schema = def.get("inputSchema").expect("tool must have inputSchema");
            assert_eq!(schema["type"], "object");
            assert_eq!(schema["additionalProperties"], false);
        }
    }

    #[test]
    fn ingress_detail_extraction_from_constructed_ingress() {
        let mut labels = BTreeMap::new();
        labels.insert("app".to_string(), "web".to_string());

        let mut annotations = BTreeMap::new();
        annotations.insert(
            "nginx.ingress.kubernetes.io/rewrite-target".to_string(),
            "/".to_string(),
        );

        let ing = Ingress {
            metadata: ObjectMeta {
                name: Some("test-ingress".to_string()),
                namespace: Some("prod".to_string()),
                labels: Some(labels),
                annotations: Some(annotations.clone()),
                ..Default::default()
            },
            spec: Some(IngressSpec {
                ingress_class_name: Some("nginx".to_string()),
                tls: Some(vec![IngressTLS {
                    hosts: Some(vec!["example.com".to_string()]),
                    secret_name: Some("tls-secret".to_string()),
                }]),
                rules: Some(vec![IngressRule {
                    host: Some("example.com".to_string()),
                    http: Some(HTTPIngressRuleValue {
                        paths: vec![HTTPIngressPath {
                            path: Some("/api".to_string()),
                            path_type: "Prefix".to_string(),
                            backend: IngressBackend {
                                service: Some(IngressServiceBackend {
                                    name: "api-svc".to_string(),
                                    port: Some(ServiceBackendPort {
                                        number: Some(8080),
                                        ..Default::default()
                                    }),
                                }),
                                ..Default::default()
                            },
                        }],
                    }),
                }]),
                ..Default::default()
            }),
            ..Default::default()
        };

        let detail: IngressDetail = ingress_detail(&ing);

        assert_eq!(detail.name, "test-ingress");
        assert_eq!(detail.namespace, "prod");
        assert_eq!(detail.ingress_class.as_deref(), Some("nginx"));
        assert_eq!(detail.hosts, vec!["example.com".to_string()]);
        assert_eq!(detail.labels.get("app").unwrap(), "web");
        assert_eq!(
            detail
                .annotations
                .get("nginx.ingress.kubernetes.io/rewrite-target")
                .unwrap(),
            "/"
        );

        // Rules
        assert_eq!(detail.rules.len(), 1);
        assert_eq!(detail.rules[0].host.as_deref(), Some("example.com"));
        assert_eq!(detail.rules[0].paths.len(), 1);
        assert_eq!(detail.rules[0].paths[0].path, "/api");
        assert_eq!(detail.rules[0].paths[0].path_type, "Prefix");
        assert_eq!(detail.rules[0].paths[0].service_name, "api-svc");
        assert_eq!(detail.rules[0].paths[0].service_port, 8080);

        // TLS
        assert_eq!(detail.tls.len(), 1);
        assert_eq!(detail.tls[0].hosts, vec!["example.com".to_string()]);
        assert_eq!(detail.tls[0].secret_name.as_deref(), Some("tls-secret"));

        // Addresses should be empty (no status set)
        assert!(detail.addresses.is_empty());
        assert!(detail.created_at.is_none());
    }

    #[test]
    fn ingress_detail_extraction_from_minimal_ingress() {
        let ing = Ingress {
            metadata: ObjectMeta {
                name: Some("minimal".to_string()),
                namespace: Some("default".to_string()),
                ..Default::default()
            },
            ..Default::default()
        };

        let detail: IngressDetail = ingress_detail(&ing);

        assert_eq!(detail.name, "minimal");
        assert_eq!(detail.namespace, "default");
        assert!(detail.ingress_class.is_none());
        assert!(detail.hosts.is_empty());
        assert!(detail.rules.is_empty());
        assert!(detail.tls.is_empty());
        assert!(detail.annotations.is_empty());
        assert!(detail.labels.is_empty());
        assert!(detail.addresses.is_empty());
        assert!(detail.created_at.is_none());
    }
}
