use k8s_openapi::api::discovery::v1::EndpointSlice;
use kube::api::ListParams;
use serde::Serialize;

use crate::client::K8sClient;

fn api(client: &K8sClient, ns: &str) -> Result<kube::Api<EndpointSlice>, String> {
    if !client.is_namespace_allowed(ns) {
        return Err(format!("Namespace '{ns}' is not in the allowed list"));
    }
    Ok(kube::Api::namespaced(client.inner().clone(), ns))
}

// ---------------------------------------------------------------------------
// Summary types
// ---------------------------------------------------------------------------

#[derive(Serialize, Debug)]
pub struct PortSummary {
    pub name: Option<String>,
    pub protocol: Option<String>,
    pub port: Option<i32>,
}

#[derive(Serialize, Debug)]
pub struct EndpointConditionsSummary {
    pub ready: Option<bool>,
    pub serving: Option<bool>,
    pub terminating: Option<bool>,
}

#[derive(Serialize, Debug)]
pub struct TargetRefSummary {
    pub kind: Option<String>,
    pub name: Option<String>,
    pub namespace: Option<String>,
}

#[derive(Serialize, Debug)]
pub struct EndpointDetail {
    pub addresses: Vec<String>,
    pub conditions: Option<EndpointConditionsSummary>,
    pub target_ref: Option<TargetRefSummary>,
    pub hostname: Option<String>,
    pub node_name: Option<String>,
    pub zone: Option<String>,
}

#[derive(Serialize, Debug)]
pub struct EndpointSliceSummary {
    pub name: String,
    pub namespace: String,
    pub address_type: String,
    pub endpoints_count: usize,
    pub ports: Vec<PortSummary>,
    pub created_at: Option<String>,
}

// ---------------------------------------------------------------------------
// Extraction helpers
// ---------------------------------------------------------------------------

fn extract_port_summary(
    port: &k8s_openapi::api::discovery::v1::EndpointPort,
) -> PortSummary {
    PortSummary {
        name: port.name.clone(),
        protocol: port.protocol.clone(),
        port: port.port,
    }
}

fn extract_summary(es: &EndpointSlice) -> EndpointSliceSummary {
    let meta = &es.metadata;

    let endpoints_count = es.endpoints.len();

    let ports: Vec<PortSummary> = es
        .ports
        .as_ref()
        .map(|ports| ports.iter().map(extract_port_summary).collect())
        .unwrap_or_default();

    EndpointSliceSummary {
        name: meta.name.clone().unwrap_or_default(),
        namespace: meta.namespace.clone().unwrap_or_default(),
        address_type: es.address_type.clone(),
        endpoints_count,
        ports,
        created_at: meta.creation_timestamp.as_ref().map(|t| t.0.to_string()),
    }
}

fn extract_endpoint_detail(
    ep: &k8s_openapi::api::discovery::v1::Endpoint,
) -> EndpointDetail {
    let conditions = ep.conditions.as_ref().map(|c| EndpointConditionsSummary {
        ready: c.ready,
        serving: c.serving,
        terminating: c.terminating,
    });

    let target_ref = ep.target_ref.as_ref().map(|tr| TargetRefSummary {
        kind: tr.kind.clone(),
        name: tr.name.clone(),
        namespace: tr.namespace.clone(),
    });

    EndpointDetail {
        addresses: ep.addresses.clone(),
        conditions,
        target_ref,
        hostname: ep.hostname.clone(),
        node_name: ep.node_name.clone(),
        zone: ep.zone.clone(),
    }
}

// ---------------------------------------------------------------------------
// Tool definitions
// ---------------------------------------------------------------------------

pub fn tool_definitions() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "name": "list_endpointslices",
            "description": "List EndpointSlices in a namespace. Returns name, namespace, address_type, endpoints count, ports, and created_at.",
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
            "name": "get_endpointslice",
            "description": "Get detailed info for a single EndpointSlice including full endpoints (addresses, conditions, target_ref), labels, and annotations.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "EndpointSlice name" }
                },
                "required": ["namespace", "name"],
                "additionalProperties": false
            }
        }),
    ]
}

// ---------------------------------------------------------------------------
// Tool handler
// ---------------------------------------------------------------------------

pub async fn handle_tool(
    client: &K8sClient,
    name: &str,
    args: &serde_json::Value,
) -> Option<Result<String, String>> {
    let result = match name {
        "list_endpointslices" => list_endpointslices(client, args).await,
        "get_endpointslice" => get_endpointslice(client, args).await,
        _ => return None,
    };
    Some(result)
}

// ---------------------------------------------------------------------------
// Tool implementations
// ---------------------------------------------------------------------------

async fn list_endpointslices(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let es_api = api(client, ns)?;
    let label_selector = args["label_selector"].as_str();
    let field_selector = args["field_selector"].as_str();
    let mut lp = ListParams::default();
    if let Some(sel) = label_selector {
        lp = lp.labels(sel);
    }
    if let Some(sel) = field_selector {
        lp = lp.fields(sel);
    }
    let list = es_api
        .list(&lp)
        .await
        .map_err(|e| e.to_string())?;

    let summaries: Vec<serde_json::Value> = list
        .iter()
        .map(|es| {
            let s = extract_summary(es);
            serde_json::to_value(s).unwrap_or_default()
        })
        .collect();

    serde_json::to_string_pretty(&summaries).map_err(|e| e.to_string())
}

async fn get_endpointslice(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;

    let es_api = api(client, ns)?;
    let es = es_api.get(name).await.map_err(|e| e.to_string())?;

    let summary = extract_summary(&es);
    let meta = &es.metadata;

    let endpoints: Vec<EndpointDetail> = es
        .endpoints
        .iter()
        .map(extract_endpoint_detail)
        .collect();

    let result = serde_json::json!({
        "name": summary.name,
        "namespace": summary.namespace,
        "address_type": summary.address_type,
        "endpoints_count": summary.endpoints_count,
        "ports": serde_json::to_value(&summary.ports).unwrap_or_default(),
        "created_at": summary.created_at,
        "endpoints": serde_json::to_value(&endpoints).unwrap_or_default(),
        "labels": meta.labels.clone().unwrap_or_default(),
        "annotations": meta.annotations.clone().unwrap_or_default(),
    });

    serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use k8s_openapi::api::discovery::v1::{Endpoint, EndpointConditions, EndpointPort};
    use k8s_openapi::api::core::v1::ObjectReference;
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

        assert!(names.contains(&"list_endpointslices"));
        assert!(names.contains(&"get_endpointslice"));
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
    fn endpointslice_summary_serialization() {
        let summary = EndpointSliceSummary {
            name: "my-svc-abc12".to_string(),
            namespace: "default".to_string(),
            address_type: "IPv4".to_string(),
            endpoints_count: 3,
            ports: vec![PortSummary {
                name: Some("http".to_string()),
                protocol: Some("TCP".to_string()),
                port: Some(8080),
            }],
            created_at: Some("2024-01-01T00:00:00Z".to_string()),
        };

        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["name"], "my-svc-abc12");
        assert_eq!(json["namespace"], "default");
        assert_eq!(json["address_type"], "IPv4");
        assert_eq!(json["endpoints_count"], 3);
        assert_eq!(json["ports"].as_array().unwrap().len(), 1);
        assert_eq!(json["ports"][0]["name"], "http");
        assert_eq!(json["ports"][0]["protocol"], "TCP");
        assert_eq!(json["ports"][0]["port"], 8080);
        assert_eq!(json["created_at"], "2024-01-01T00:00:00Z");
    }

    #[test]
    fn endpointslice_summary_serialization_empty_fields() {
        let summary = EndpointSliceSummary {
            name: "empty".to_string(),
            namespace: "ns".to_string(),
            address_type: "IPv4".to_string(),
            endpoints_count: 0,
            ports: vec![],
            created_at: None,
        };

        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["name"], "empty");
        assert_eq!(json["endpoints_count"], 0);
        assert!(json["ports"].as_array().unwrap().is_empty());
        assert!(json["created_at"].is_null());
    }

    fn make_test_endpointslice() -> EndpointSlice {
        let mut labels = BTreeMap::new();
        labels.insert("kubernetes.io/service-name".to_string(), "my-svc".to_string());

        let mut annotations = BTreeMap::new();
        annotations.insert(
            "endpoints.kubernetes.io/last-change-trigger-time".to_string(),
            "2024-06-15T12:00:00Z".to_string(),
        );

        EndpointSlice {
            metadata: ObjectMeta {
                name: Some("my-svc-abc12".to_string()),
                namespace: Some("prod".to_string()),
                labels: Some(labels),
                annotations: Some(annotations),
                creation_timestamp: Some(k8s_openapi::apimachinery::pkg::apis::meta::v1::Time(
                    "2024-06-15T12:00:00Z".parse::<k8s_openapi::jiff::Timestamp>().unwrap(),
                )),
                ..Default::default()
            },
            address_type: "IPv4".to_string(),
            endpoints: vec![
                Endpoint {
                    addresses: vec!["10.244.0.5".to_string(), "10.244.0.6".to_string()],
                    conditions: Some(EndpointConditions {
                        ready: Some(true),
                        serving: Some(true),
                        terminating: Some(false),
                    }),
                    target_ref: Some(ObjectReference {
                        kind: Some("Pod".to_string()),
                        name: Some("my-svc-pod-1".to_string()),
                        namespace: Some("prod".to_string()),
                        ..Default::default()
                    }),
                    hostname: Some("pod-1".to_string()),
                    node_name: Some("node-1".to_string()),
                    zone: Some("us-east-1a".to_string()),
                    ..Default::default()
                },
                Endpoint {
                    addresses: vec!["10.244.1.10".to_string()],
                    conditions: Some(EndpointConditions {
                        ready: Some(false),
                        serving: Some(false),
                        terminating: Some(true),
                    }),
                    target_ref: Some(ObjectReference {
                        kind: Some("Pod".to_string()),
                        name: Some("my-svc-pod-2".to_string()),
                        namespace: Some("prod".to_string()),
                        ..Default::default()
                    }),
                    hostname: None,
                    node_name: Some("node-2".to_string()),
                    zone: Some("us-east-1b".to_string()),
                    ..Default::default()
                },
            ],
            ports: Some(vec![
                EndpointPort {
                    name: Some("http".to_string()),
                    protocol: Some("TCP".to_string()),
                    port: Some(8080),
                    ..Default::default()
                },
                EndpointPort {
                    name: Some("https".to_string()),
                    protocol: Some("TCP".to_string()),
                    port: Some(8443),
                    ..Default::default()
                },
            ]),
        }
    }

    #[test]
    fn extract_summary_from_endpointslice() {
        let es = make_test_endpointslice();
        let summary = extract_summary(&es);

        assert_eq!(summary.name, "my-svc-abc12");
        assert_eq!(summary.namespace, "prod");
        assert_eq!(summary.address_type, "IPv4");
        assert_eq!(summary.endpoints_count, 2);
        assert_eq!(summary.ports.len(), 2);

        assert_eq!(summary.ports[0].name.as_deref(), Some("http"));
        assert_eq!(summary.ports[0].protocol.as_deref(), Some("TCP"));
        assert_eq!(summary.ports[0].port, Some(8080));

        assert_eq!(summary.ports[1].name.as_deref(), Some("https"));
        assert_eq!(summary.ports[1].protocol.as_deref(), Some("TCP"));
        assert_eq!(summary.ports[1].port, Some(8443));

        assert!(summary.created_at.is_some());
    }

    #[test]
    fn extract_summary_from_minimal_endpointslice() {
        let es = EndpointSlice {
            metadata: ObjectMeta {
                name: Some("minimal".to_string()),
                namespace: Some("default".to_string()),
                ..Default::default()
            },
            address_type: "IPv6".to_string(),
            endpoints: vec![],
            ports: None,
        };

        let summary = extract_summary(&es);
        assert_eq!(summary.name, "minimal");
        assert_eq!(summary.namespace, "default");
        assert_eq!(summary.address_type, "IPv6");
        assert_eq!(summary.endpoints_count, 0);
        assert!(summary.ports.is_empty());
        assert!(summary.created_at.is_none());
    }

    #[test]
    fn extract_endpoint_detail_full() {
        let es = make_test_endpointslice();
        let detail = extract_endpoint_detail(&es.endpoints[0]);

        assert_eq!(detail.addresses, vec!["10.244.0.5", "10.244.0.6"]);
        assert_eq!(detail.hostname.as_deref(), Some("pod-1"));
        assert_eq!(detail.node_name.as_deref(), Some("node-1"));
        assert_eq!(detail.zone.as_deref(), Some("us-east-1a"));

        let cond = detail.conditions.as_ref().unwrap();
        assert_eq!(cond.ready, Some(true));
        assert_eq!(cond.serving, Some(true));
        assert_eq!(cond.terminating, Some(false));

        let tref = detail.target_ref.as_ref().unwrap();
        assert_eq!(tref.kind.as_deref(), Some("Pod"));
        assert_eq!(tref.name.as_deref(), Some("my-svc-pod-1"));
        assert_eq!(tref.namespace.as_deref(), Some("prod"));
    }

    #[test]
    fn extract_endpoint_detail_minimal() {
        let ep = Endpoint {
            addresses: vec!["10.0.0.1".to_string()],
            conditions: None,
            target_ref: None,
            hostname: None,
            node_name: None,
            zone: None,
            ..Default::default()
        };

        let detail = extract_endpoint_detail(&ep);
        assert_eq!(detail.addresses, vec!["10.0.0.1"]);
        assert!(detail.conditions.is_none());
        assert!(detail.target_ref.is_none());
        assert!(detail.hostname.is_none());
        assert!(detail.node_name.is_none());
        assert!(detail.zone.is_none());
    }
}
