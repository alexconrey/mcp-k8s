
use k8s_openapi::api::core::v1::Endpoints;
use kube::api::ListParams;
use serde::Serialize;

use crate::client::K8sClient;

fn api(client: &K8sClient, ns: &str) -> Result<kube::Api<Endpoints>, String> {
    if !client.is_namespace_allowed(ns) {
        return Err(format!("Namespace '{ns}' is not in the allowed list"));
    }
    Ok(kube::Api::namespaced(client.inner().clone(), ns))
}

#[derive(Serialize, Debug)]
pub struct EndpointsSummary {
    pub name: String,
    pub namespace: String,
    pub subsets_count: usize,
    pub total_addresses: usize,
    pub created_at: Option<String>,
}

fn extract_summary(ep: &Endpoints) -> EndpointsSummary {
    let meta = &ep.metadata;
    let subsets = ep.subsets.as_deref().unwrap_or_default();
    let subsets_count = subsets.len();
    let total_addresses: usize = subsets
        .iter()
        .map(|s| s.addresses.as_deref().unwrap_or_default().len())
        .sum();

    EndpointsSummary {
        name: meta.name.clone().unwrap_or_default(),
        namespace: meta.namespace.clone().unwrap_or_default(),
        subsets_count,
        total_addresses,
        created_at: meta.creation_timestamp.as_ref().map(|t| t.0.to_string()),
    }
}

pub fn tool_definitions() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "name": "list_endpoints",
            "description": "List endpoints in a namespace. Returns name, namespace, subsets count, total addresses count, and created_at.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "label_selector": { "type": "string", "description": "Label selector to filter results (e.g. app=nginx)" },
                    "field_selector": { "type": "string", "description": "Field selector to filter results (e.g. metadata.name=foo)" }
                },
                "required": ["namespace"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "get_endpoints",
            "description": "Get endpoints by name. Returns subsets with addresses and ports, labels, and annotations.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "Endpoints name" }
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
        "list_endpoints" => list_endpoints(client, args).await,
        "get_endpoints" => get_endpoints(client, args).await,
        _ => return None,
    };
    Some(result)
}

async fn list_endpoints(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let ep_api = api(client, ns)?;
    let label_selector = args["label_selector"].as_str();
    let field_selector = args["field_selector"].as_str();
    let mut lp = ListParams::default();
    if let Some(sel) = label_selector {
        lp = lp.labels(sel);
    }
    if let Some(sel) = field_selector {
        lp = lp.fields(sel);
    }
    let list = ep_api
        .list(&lp)
        .await
        .map_err(|e| e.to_string())?;

    let summaries: Vec<serde_json::Value> = list
        .iter()
        .map(|ep| {
            let s = extract_summary(ep);
            serde_json::to_value(s).unwrap_or_default()
        })
        .collect();

    serde_json::to_string_pretty(&summaries).map_err(|e| e.to_string())
}

async fn get_endpoints(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;

    let ep_api = api(client, ns)?;
    let ep = ep_api.get(name).await.map_err(|e| e.to_string())?;

    let meta = &ep.metadata;
    let subsets: Vec<serde_json::Value> = ep
        .subsets
        .as_deref()
        .unwrap_or_default()
        .iter()
        .map(|subset| {
            let addresses: Vec<serde_json::Value> = subset
                .addresses
                .as_deref()
                .unwrap_or_default()
                .iter()
                .map(|addr| {
                    serde_json::json!({
                        "ip": addr.ip,
                        "target_ref_name": addr.target_ref.as_ref().and_then(|r| r.name.clone()),
                    })
                })
                .collect();

            let ports: Vec<serde_json::Value> = subset
                .ports
                .as_deref()
                .unwrap_or_default()
                .iter()
                .map(|port| {
                    serde_json::json!({
                        "port": port.port,
                        "protocol": port.protocol.clone().unwrap_or_else(|| "TCP".to_string()),
                        "name": port.name,
                    })
                })
                .collect();

            serde_json::json!({
                "addresses": addresses,
                "ports": ports,
            })
        })
        .collect();

    let result = serde_json::json!({
        "name": meta.name.clone().unwrap_or_default(),
        "namespace": meta.namespace.clone().unwrap_or_default(),
        "subsets": subsets,
        "labels": meta.labels.clone().unwrap_or_default(),
        "annotations": meta.annotations.clone().unwrap_or_default(),
    });

    serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use k8s_openapi::api::core::v1::{EndpointAddress, EndpointPort, EndpointSubset, ObjectReference};
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

    #[test]
    fn tool_definitions_returns_two_unique_tools() {
        let defs = tool_definitions();
        assert_eq!(defs.len(), 2);

        let names: Vec<&str> = defs
            .iter()
            .map(|d| d["name"].as_str().unwrap())
            .collect();

        let mut unique = names.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(names.len(), unique.len(), "tool names must be unique");

        assert!(names.contains(&"list_endpoints"));
        assert!(names.contains(&"get_endpoints"));
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
    fn endpoints_summary_serialization() {
        let summary = EndpointsSummary {
            name: "my-ep".to_string(),
            namespace: "default".to_string(),
            subsets_count: 2,
            total_addresses: 5,
            created_at: Some("2024-01-01T00:00:00Z".to_string()),
        };

        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["name"], "my-ep");
        assert_eq!(json["namespace"], "default");
        assert_eq!(json["subsets_count"], 2);
        assert_eq!(json["total_addresses"], 5);
        assert_eq!(json["created_at"], "2024-01-01T00:00:00Z");
    }

    #[test]
    fn endpoints_summary_serialization_empty_fields() {
        let summary = EndpointsSummary {
            name: "empty".to_string(),
            namespace: "ns".to_string(),
            subsets_count: 0,
            total_addresses: 0,
            created_at: None,
        };

        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["name"], "empty");
        assert_eq!(json["subsets_count"], 0);
        assert_eq!(json["total_addresses"], 0);
        assert!(json["created_at"].is_null());
    }

    fn make_test_endpoints() -> Endpoints {
        let mut labels = BTreeMap::new();
        labels.insert("app".to_string(), "web".to_string());

        let mut annotations = BTreeMap::new();
        annotations.insert(
            "endpoints.kubernetes.io/last-change-trigger-time".to_string(),
            "2024-06-15T10:30:00Z".to_string(),
        );

        Endpoints {
            metadata: ObjectMeta {
                name: Some("test-ep".to_string()),
                namespace: Some("prod".to_string()),
                labels: Some(labels),
                annotations: Some(annotations),
                ..Default::default()
            },
            subsets: Some(vec![
                EndpointSubset {
                    addresses: Some(vec![
                        EndpointAddress {
                            ip: "10.0.0.1".to_string(),
                            target_ref: Some(ObjectReference {
                                name: Some("pod-abc".to_string()),
                                ..Default::default()
                            }),
                            ..Default::default()
                        },
                        EndpointAddress {
                            ip: "10.0.0.2".to_string(),
                            target_ref: Some(ObjectReference {
                                name: Some("pod-def".to_string()),
                                ..Default::default()
                            }),
                            ..Default::default()
                        },
                    ]),
                    ports: Some(vec![
                        EndpointPort {
                            port: 8080,
                            protocol: Some("TCP".to_string()),
                            name: Some("http".to_string()),
                            ..Default::default()
                        },
                    ]),
                    not_ready_addresses: None,
                },
                EndpointSubset {
                    addresses: Some(vec![
                        EndpointAddress {
                            ip: "10.0.1.1".to_string(),
                            target_ref: None,
                            ..Default::default()
                        },
                    ]),
                    ports: Some(vec![
                        EndpointPort {
                            port: 9090,
                            protocol: Some("TCP".to_string()),
                            name: Some("grpc".to_string()),
                            ..Default::default()
                        },
                        EndpointPort {
                            port: 9091,
                            protocol: Some("UDP".to_string()),
                            name: None,
                            ..Default::default()
                        },
                    ]),
                    not_ready_addresses: None,
                },
            ]),
        }
    }

    #[test]
    fn extract_summary_from_endpoints() {
        let ep = make_test_endpoints();
        let summary = extract_summary(&ep);

        assert_eq!(summary.name, "test-ep");
        assert_eq!(summary.namespace, "prod");
        assert_eq!(summary.subsets_count, 2);
        assert_eq!(summary.total_addresses, 3);
        assert!(summary.created_at.is_none());
    }

    #[test]
    fn extract_summary_from_empty_endpoints() {
        let ep = Endpoints {
            metadata: ObjectMeta {
                name: Some("empty-ep".to_string()),
                namespace: Some("default".to_string()),
                ..Default::default()
            },
            subsets: None,
        };

        let summary = extract_summary(&ep);
        assert_eq!(summary.name, "empty-ep");
        assert_eq!(summary.namespace, "default");
        assert_eq!(summary.subsets_count, 0);
        assert_eq!(summary.total_addresses, 0);
        assert!(summary.created_at.is_none());
    }

    #[test]
    fn extract_summary_from_endpoints_with_empty_subsets() {
        let ep = Endpoints {
            metadata: ObjectMeta {
                name: Some("no-addr-ep".to_string()),
                namespace: Some("default".to_string()),
                ..Default::default()
            },
            subsets: Some(vec![
                EndpointSubset {
                    addresses: None,
                    ports: Some(vec![]),
                    not_ready_addresses: None,
                },
            ]),
        };

        let summary = extract_summary(&ep);
        assert_eq!(summary.subsets_count, 1);
        assert_eq!(summary.total_addresses, 0);
    }
}
