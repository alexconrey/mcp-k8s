use k8s_openapi::api::networking::v1::Ingress;
use kube::api::DeleteParams;

use crate::client::K8sClient;
use crate::extract::ingress_detail;
use crate::types::IngressDetail;

fn api(client: &K8sClient, ns: &str) -> Result<kube::Api<Ingress>, String> {
    if !client.is_namespace_allowed(ns) {
        return Err(format!("Namespace '{ns}' is not in the allowed list"));
    }
    Ok(kube::Api::namespaced(client.inner().clone(), ns))
}

pub fn tool_definitions() -> Vec<serde_json::Value> {
    vec![
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
