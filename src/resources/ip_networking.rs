use k8s_openapi::api::networking::v1beta1::{IPAddress, ServiceCIDR};
use kube::api::ListParams;
use serde::Serialize;

use crate::client::K8sClient;

fn ipaddress_api(client: &K8sClient) -> kube::Api<IPAddress> {
    kube::Api::all(client.inner().clone())
}

fn servicecidr_api(client: &K8sClient) -> kube::Api<ServiceCIDR> {
    kube::Api::all(client.inner().clone())
}

// --- Summaries ---

#[derive(Serialize, Debug)]
pub struct IPAddressSummary {
    pub name: String,
    pub parent_ref: Option<ParentRefSummary>,
    pub created_at: Option<String>,
}

#[derive(Serialize, Debug)]
pub struct ParentRefSummary {
    pub group: Option<String>,
    pub resource: String,
    pub name: String,
    pub namespace: Option<String>,
}

#[derive(Serialize, Debug)]
pub struct ServiceCIDRSummary {
    pub name: String,
    pub cidrs: Vec<String>,
    pub created_at: Option<String>,
}

fn extract_ipaddress_summary(ip: &IPAddress) -> IPAddressSummary {
    let meta = &ip.metadata;

    let parent_ref = ip.spec.as_ref().map(|s| {
        let pr = &s.parent_ref;
        ParentRefSummary {
            group: pr.group.clone(),
            resource: pr.resource.clone(),
            name: pr.name.clone(),
            namespace: pr.namespace.clone(),
        }
    });

    IPAddressSummary {
        name: meta.name.clone().unwrap_or_default(),
        parent_ref,
        created_at: meta.creation_timestamp.as_ref().map(|t| t.0.to_string()),
    }
}

fn extract_servicecidr_summary(sc: &ServiceCIDR) -> ServiceCIDRSummary {
    let meta = &sc.metadata;

    let cidrs = sc
        .spec
        .as_ref()
        .and_then(|s| s.cidrs.clone())
        .unwrap_or_default();

    ServiceCIDRSummary {
        name: meta.name.clone().unwrap_or_default(),
        cidrs,
        created_at: meta.creation_timestamp.as_ref().map(|t| t.0.to_string()),
    }
}

// --- Tool definitions ---

pub fn tool_definitions() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "name": "list_ipaddresses",
            "description": "List all IPAddress resources (networking.k8s.io/v1beta1). Returns name, parent_ref (group, resource, name, namespace), and created_at.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "required": [],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "get_ipaddress",
            "description": "Get an IPAddress by name (networking.k8s.io/v1beta1). Returns name, parent_ref (group, resource, name, namespace), created_at, labels, and annotations.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "IPAddress name (the IP in canonical format)" }
                },
                "required": ["name"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "list_servicecidrs",
            "description": "List all ServiceCIDR resources (networking.k8s.io/v1beta1). Returns name, cidrs (from spec), and created_at.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "required": [],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "get_servicecidr",
            "description": "Get a ServiceCIDR by name (networking.k8s.io/v1beta1). Returns name, cidrs, created_at, status conditions, labels, and annotations.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "ServiceCIDR name" }
                },
                "required": ["name"],
                "additionalProperties": false
            }
        }),
    ]
}

// --- Handler ---

pub async fn handle_tool(
    client: &K8sClient,
    name: &str,
    args: &serde_json::Value,
) -> Option<Result<String, String>> {
    let result = match name {
        "list_ipaddresses" => list_ipaddresses(client).await,
        "get_ipaddress" => get_ipaddress(client, args).await,
        "list_servicecidrs" => list_servicecidrs(client).await,
        "get_servicecidr" => get_servicecidr(client, args).await,
        _ => return None,
    };
    Some(result)
}

// --- IPAddress ---

async fn list_ipaddresses(client: &K8sClient) -> Result<String, String> {
    let api = ipaddress_api(client);
    let list = api
        .list(&ListParams::default())
        .await
        .map_err(|e| e.to_string())?;

    let summaries: Vec<IPAddressSummary> = list.iter().map(extract_ipaddress_summary).collect();

    serde_json::to_string_pretty(&summaries).map_err(|e| e.to_string())
}

async fn get_ipaddress(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let name = args["name"].as_str().ok_or("name is required")?;
    let api = ipaddress_api(client);
    let ip = api.get(name).await.map_err(|e| e.to_string())?;

    let summary = extract_ipaddress_summary(&ip);
    let meta = &ip.metadata;

    let result = serde_json::json!({
        "name": summary.name,
        "parent_ref": summary.parent_ref.map(|pr| serde_json::json!({
            "group": pr.group,
            "resource": pr.resource,
            "name": pr.name,
            "namespace": pr.namespace,
        })),
        "created_at": summary.created_at,
        "labels": meta.labels.clone().unwrap_or_default(),
        "annotations": meta.annotations.clone().unwrap_or_default(),
    });

    serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
}

// --- ServiceCIDR ---

async fn list_servicecidrs(client: &K8sClient) -> Result<String, String> {
    let api = servicecidr_api(client);
    let list = api
        .list(&ListParams::default())
        .await
        .map_err(|e| e.to_string())?;

    let summaries: Vec<ServiceCIDRSummary> = list.iter().map(extract_servicecidr_summary).collect();

    serde_json::to_string_pretty(&summaries).map_err(|e| e.to_string())
}

async fn get_servicecidr(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let name = args["name"].as_str().ok_or("name is required")?;
    let api = servicecidr_api(client);
    let sc = api.get(name).await.map_err(|e| e.to_string())?;

    let summary = extract_servicecidr_summary(&sc);
    let meta = &sc.metadata;

    let conditions: Vec<serde_json::Value> = sc
        .status
        .as_ref()
        .and_then(|s| s.conditions.as_ref())
        .map(|conds| {
            conds
                .iter()
                .map(|c| {
                    serde_json::json!({
                        "type": c.type_,
                        "status": c.status,
                        "reason": c.reason,
                        "message": c.message,
                        "last_transition_time": c.last_transition_time.0.to_string(),
                        "observed_generation": c.observed_generation,
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    let result = serde_json::json!({
        "name": summary.name,
        "cidrs": summary.cidrs,
        "created_at": summary.created_at,
        "conditions": conditions,
        "labels": meta.labels.clone().unwrap_or_default(),
        "annotations": meta.annotations.clone().unwrap_or_default(),
    });

    serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::api::networking::v1beta1::{
        IPAddressSpec, ParentReference, ServiceCIDRSpec, ServiceCIDRStatus,
    };
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::{Condition, ObjectMeta, Time};

    #[test]
    fn tool_definitions_returns_four_tools() {
        let defs = tool_definitions();
        assert_eq!(defs.len(), 4);

        let names: Vec<&str> = defs.iter().map(|d| d["name"].as_str().unwrap()).collect();

        let mut unique = names.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(names.len(), unique.len(), "tool names must be unique");

        assert!(names.contains(&"list_ipaddresses"));
        assert!(names.contains(&"get_ipaddress"));
        assert!(names.contains(&"list_servicecidrs"));
        assert!(names.contains(&"get_servicecidr"));
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
    fn list_tools_have_no_required_params() {
        let defs = tool_definitions();
        for def in &defs {
            let name = def["name"].as_str().unwrap();
            if name.starts_with("list_") {
                let schema = &def["inputSchema"];
                let props = schema["properties"].as_object().unwrap();
                assert!(props.is_empty(), "{name} should have no properties");
            }
        }
    }

    #[test]
    fn get_tools_require_name_param() {
        let defs = tool_definitions();
        for def in &defs {
            let tool_name = def["name"].as_str().unwrap();
            if tool_name.starts_with("get_") {
                let schema = &def["inputSchema"];
                let required = schema["required"].as_array().unwrap();
                assert!(
                    required.iter().any(|v| v.as_str() == Some("name")),
                    "{tool_name} must require 'name' parameter"
                );
                assert!(
                    schema["properties"].get("name").is_some(),
                    "{tool_name} must define 'name' property"
                );
            }
        }
    }

    #[test]
    fn ipaddress_summary_serialization() {
        let summary = IPAddressSummary {
            name: "10.96.0.1".to_string(),
            parent_ref: Some(ParentRefSummary {
                group: Some("".to_string()),
                resource: "services".to_string(),
                name: "kubernetes".to_string(),
                namespace: Some("default".to_string()),
            }),
            created_at: Some("2024-01-01T00:00:00Z".to_string()),
        };

        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["name"], "10.96.0.1");
        assert_eq!(json["parent_ref"]["group"], "");
        assert_eq!(json["parent_ref"]["resource"], "services");
        assert_eq!(json["parent_ref"]["name"], "kubernetes");
        assert_eq!(json["parent_ref"]["namespace"], "default");
        assert_eq!(json["created_at"], "2024-01-01T00:00:00Z");
    }

    #[test]
    fn ipaddress_summary_no_spec() {
        let summary = IPAddressSummary {
            name: "10.96.0.2".to_string(),
            parent_ref: None,
            created_at: None,
        };

        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["name"], "10.96.0.2");
        assert!(json["parent_ref"].is_null());
        assert!(json["created_at"].is_null());
    }

    #[test]
    fn servicecidr_summary_serialization() {
        let summary = ServiceCIDRSummary {
            name: "kubernetes".to_string(),
            cidrs: vec!["10.96.0.0/12".to_string(), "fd00::/108".to_string()],
            created_at: Some("2024-01-01T00:00:00Z".to_string()),
        };

        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["name"], "kubernetes");
        let cidrs = json["cidrs"].as_array().unwrap();
        assert_eq!(cidrs.len(), 2);
        assert_eq!(cidrs[0], "10.96.0.0/12");
        assert_eq!(cidrs[1], "fd00::/108");
        assert_eq!(json["created_at"], "2024-01-01T00:00:00Z");
    }

    #[test]
    fn servicecidr_summary_no_cidrs() {
        let summary = ServiceCIDRSummary {
            name: "empty".to_string(),
            cidrs: vec![],
            created_at: None,
        };

        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["name"], "empty");
        assert!(json["cidrs"].as_array().unwrap().is_empty());
        assert!(json["created_at"].is_null());
    }

    #[test]
    fn extract_ipaddress_summary_with_spec() {
        let ip = IPAddress {
            metadata: ObjectMeta {
                name: Some("10.96.0.10".to_string()),
                ..Default::default()
            },
            spec: Some(IPAddressSpec {
                parent_ref: ParentReference {
                    group: Some("".to_string()),
                    resource: "services".to_string(),
                    name: "my-svc".to_string(),
                    namespace: Some("kube-system".to_string()),
                },
            }),
        };

        let summary = extract_ipaddress_summary(&ip);
        assert_eq!(summary.name, "10.96.0.10");
        let pr = summary.parent_ref.unwrap();
        assert_eq!(pr.group.as_deref(), Some(""));
        assert_eq!(pr.resource, "services");
        assert_eq!(pr.name, "my-svc");
        assert_eq!(pr.namespace.as_deref(), Some("kube-system"));
    }

    #[test]
    fn extract_ipaddress_summary_without_spec() {
        let ip = IPAddress {
            metadata: ObjectMeta {
                name: Some("10.96.0.11".to_string()),
                ..Default::default()
            },
            spec: None,
        };

        let summary = extract_ipaddress_summary(&ip);
        assert_eq!(summary.name, "10.96.0.11");
        assert!(summary.parent_ref.is_none());
        assert!(summary.created_at.is_none());
    }

    #[test]
    fn extract_servicecidr_summary_with_spec() {
        let sc = ServiceCIDR {
            metadata: ObjectMeta {
                name: Some("kubernetes".to_string()),
                ..Default::default()
            },
            spec: Some(ServiceCIDRSpec {
                cidrs: Some(vec!["10.96.0.0/12".to_string(), "fd00::/108".to_string()]),
            }),
            status: None,
        };

        let summary = extract_servicecidr_summary(&sc);
        assert_eq!(summary.name, "kubernetes");
        assert_eq!(summary.cidrs.len(), 2);
        assert_eq!(summary.cidrs[0], "10.96.0.0/12");
        assert_eq!(summary.cidrs[1], "fd00::/108");
    }

    #[test]
    fn extract_servicecidr_summary_without_spec() {
        let sc = ServiceCIDR {
            metadata: ObjectMeta {
                name: Some("empty".to_string()),
                ..Default::default()
            },
            spec: None,
            status: None,
        };

        let summary = extract_servicecidr_summary(&sc);
        assert_eq!(summary.name, "empty");
        assert!(summary.cidrs.is_empty());
        assert!(summary.created_at.is_none());
    }

    #[test]
    fn extract_servicecidr_summary_with_status() {
        let sc = ServiceCIDR {
            metadata: ObjectMeta {
                name: Some("kubernetes".to_string()),
                ..Default::default()
            },
            spec: Some(ServiceCIDRSpec {
                cidrs: Some(vec!["10.96.0.0/12".to_string()]),
            }),
            status: Some(ServiceCIDRStatus {
                conditions: Some(vec![Condition {
                    type_: "Ready".to_string(),
                    status: "True".to_string(),
                    reason: "Initialized".to_string(),
                    message: "ServiceCIDR is ready".to_string(),
                    last_transition_time: Time(
                        "2024-06-01T00:00:00Z"
                            .parse::<k8s_openapi::jiff::Timestamp>()
                            .unwrap(),
                    ),
                    observed_generation: Some(1),
                }]),
            }),
        };

        // extract_servicecidr_summary doesn't include status,
        // but verify it doesn't fail
        let summary = extract_servicecidr_summary(&sc);
        assert_eq!(summary.name, "kubernetes");
        assert_eq!(summary.cidrs.len(), 1);
    }

    #[test]
    fn parent_ref_summary_serialization() {
        let pr = ParentRefSummary {
            group: Some("networking.k8s.io".to_string()),
            resource: "services".to_string(),
            name: "my-service".to_string(),
            namespace: Some("default".to_string()),
        };

        let json = serde_json::to_value(&pr).unwrap();
        assert_eq!(json["group"], "networking.k8s.io");
        assert_eq!(json["resource"], "services");
        assert_eq!(json["name"], "my-service");
        assert_eq!(json["namespace"], "default");
    }

    #[test]
    fn parent_ref_summary_optional_fields() {
        let pr = ParentRefSummary {
            group: None,
            resource: "services".to_string(),
            name: "my-service".to_string(),
            namespace: None,
        };

        let json = serde_json::to_value(&pr).unwrap();
        assert!(json["group"].is_null());
        assert_eq!(json["resource"], "services");
        assert_eq!(json["name"], "my-service");
        assert!(json["namespace"].is_null());
    }
}
