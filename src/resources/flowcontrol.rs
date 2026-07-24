use k8s_openapi::api::flowcontrol::v1::{FlowSchema, PriorityLevelConfiguration};
use kube::api::ListParams;
use serde::Serialize;

use crate::client::K8sClient;

fn flowschema_api(client: &K8sClient) -> kube::Api<FlowSchema> {
    kube::Api::all(client.inner().clone())
}

fn prioritylevel_api(client: &K8sClient) -> kube::Api<PriorityLevelConfiguration> {
    kube::Api::all(client.inner().clone())
}

// ---------------------------------------------------------------------------
// Summary types
// ---------------------------------------------------------------------------

#[derive(Serialize, Debug)]
pub struct FlowSchemaSummary {
    pub name: String,
    pub priority_level_name: String,
    pub matching_precedence: Option<i32>,
    pub created_at: Option<String>,
}

#[derive(Serialize, Debug)]
pub struct PriorityLevelSummary {
    pub name: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub assured_concurrency_shares: Option<i32>,
    pub created_at: Option<String>,
}

// ---------------------------------------------------------------------------
// Condition summary (shared shape for both resource types)
// ---------------------------------------------------------------------------

#[derive(Serialize, Debug)]
struct ConditionSummary {
    condition_type: Option<String>,
    status: Option<String>,
    reason: Option<String>,
    message: Option<String>,
    last_transition: Option<String>,
}

// ---------------------------------------------------------------------------
// FlowSchema extraction helpers
// ---------------------------------------------------------------------------

fn extract_flowschema_summary(fs: &FlowSchema) -> FlowSchemaSummary {
    let meta = &fs.metadata;
    let spec = fs.spec.as_ref();

    let priority_level_name = spec
        .map(|s| s.priority_level_configuration.name.clone())
        .unwrap_or_default();

    let matching_precedence = spec.and_then(|s| s.matching_precedence);

    let created_at = meta.creation_timestamp.as_ref().map(|t| t.0.to_string());

    FlowSchemaSummary {
        name: meta.name.clone().unwrap_or_default(),
        priority_level_name,
        matching_precedence,
        created_at,
    }
}

// ---------------------------------------------------------------------------
// PriorityLevelConfiguration extraction helpers
// ---------------------------------------------------------------------------

fn extract_prioritylevel_summary(plc: &PriorityLevelConfiguration) -> PriorityLevelSummary {
    let meta = &plc.metadata;
    let spec = plc.spec.as_ref();

    let type_ = spec.map(|s| s.type_.clone()).unwrap_or_default();

    let assured_concurrency_shares = spec
        .and_then(|s| s.limited.as_ref())
        .and_then(|l| l.nominal_concurrency_shares);

    let created_at = meta.creation_timestamp.as_ref().map(|t| t.0.to_string());

    PriorityLevelSummary {
        name: meta.name.clone().unwrap_or_default(),
        type_,
        assured_concurrency_shares,
        created_at,
    }
}

// ---------------------------------------------------------------------------
// Tool definitions
// ---------------------------------------------------------------------------

pub fn tool_definitions() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "name": "list_flowschemas",
            "description": "List all FlowSchemas in the cluster. Returns name, priority_level_name, matching_precedence, and created_at.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "required": [],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "get_flowschema",
            "description": "Get a FlowSchema by name. Returns name, priority_level_name, matching_precedence, distinguisher_method, rules, conditions, labels, annotations, and created_at.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "FlowSchema name" }
                },
                "required": ["name"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "list_prioritylevelconfigs",
            "description": "List all PriorityLevelConfigurations in the cluster. Returns name, type (Limited/Exempt), assured_concurrency_shares, and created_at.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "required": [],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "get_prioritylevelconfig",
            "description": "Get a PriorityLevelConfiguration by name. Returns name, type, assured_concurrency_shares, limited config (nominal_concurrency, limit_response), conditions, labels, annotations, and created_at.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "PriorityLevelConfiguration name" }
                },
                "required": ["name"],
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
        "list_flowschemas" => list_flowschemas(client).await,
        "get_flowschema" => get_flowschema(client, args).await,
        "list_prioritylevelconfigs" => list_prioritylevelconfigs(client).await,
        "get_prioritylevelconfig" => get_prioritylevelconfig(client, args).await,
        _ => return None,
    };
    Some(result)
}

// ---------------------------------------------------------------------------
// Tool implementations
// ---------------------------------------------------------------------------

async fn list_flowschemas(client: &K8sClient) -> Result<String, String> {
    let api = flowschema_api(client);
    let list = api
        .list(&ListParams::default())
        .await
        .map_err(|e| e.to_string())?;

    let summaries: Vec<FlowSchemaSummary> = list.iter().map(extract_flowschema_summary).collect();

    serde_json::to_string_pretty(&summaries).map_err(|e| e.to_string())
}

async fn get_flowschema(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let name = args["name"].as_str().ok_or("name is required")?;

    let api = flowschema_api(client);
    let fs = api.get(name).await.map_err(|e| e.to_string())?;

    let summary = extract_flowschema_summary(&fs);
    let meta = &fs.metadata;
    let spec = fs.spec.as_ref();

    let distinguisher_method = spec
        .and_then(|s| s.distinguisher_method.as_ref())
        .map(|dm| dm.type_.clone());

    let rules = spec
        .and_then(|s| s.rules.as_ref())
        .map(|r| serde_json::to_value(r).unwrap_or_default())
        .unwrap_or(serde_json::Value::Array(vec![]));

    let conditions: Vec<ConditionSummary> = fs
        .status
        .as_ref()
        .and_then(|s| s.conditions.as_ref())
        .map(|conds| {
            conds
                .iter()
                .map(|c| ConditionSummary {
                    condition_type: c.type_.clone(),
                    status: c.status.clone(),
                    reason: c.reason.clone(),
                    message: c.message.clone(),
                    last_transition: c.last_transition_time.as_ref().map(|t| t.0.to_string()),
                })
                .collect()
        })
        .unwrap_or_default();

    let result = serde_json::json!({
        "name": summary.name,
        "priority_level_name": summary.priority_level_name,
        "matching_precedence": summary.matching_precedence,
        "distinguisher_method": distinguisher_method,
        "rules": rules,
        "conditions": conditions,
        "labels": meta.labels.clone().unwrap_or_default(),
        "annotations": meta.annotations.clone().unwrap_or_default(),
        "created_at": summary.created_at,
    });

    serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
}

async fn list_prioritylevelconfigs(client: &K8sClient) -> Result<String, String> {
    let api = prioritylevel_api(client);
    let list = api
        .list(&ListParams::default())
        .await
        .map_err(|e| e.to_string())?;

    let summaries: Vec<PriorityLevelSummary> =
        list.iter().map(extract_prioritylevel_summary).collect();

    serde_json::to_string_pretty(&summaries).map_err(|e| e.to_string())
}

async fn get_prioritylevelconfig(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let name = args["name"].as_str().ok_or("name is required")?;

    let api = prioritylevel_api(client);
    let plc = api.get(name).await.map_err(|e| e.to_string())?;

    let summary = extract_prioritylevel_summary(&plc);
    let meta = &plc.metadata;
    let spec = plc.spec.as_ref();

    let limited = spec.and_then(|s| s.limited.as_ref());

    let limited_config = limited.map(|l| {
        serde_json::json!({
            "nominal_concurrency_shares": l.nominal_concurrency_shares,
            "borrowing_limit_percent": l.borrowing_limit_percent,
            "lendable_percent": l.lendable_percent,
            "limit_response": l.limit_response.as_ref().map(|lr| {
                serde_json::json!({
                    "type": lr.type_,
                    "queuing": lr.queuing.as_ref().map(|q| serde_json::to_value(q).unwrap_or_default()),
                })
            }),
        })
    });

    let conditions: Vec<ConditionSummary> = plc
        .status
        .as_ref()
        .and_then(|s| s.conditions.as_ref())
        .map(|conds| {
            conds
                .iter()
                .map(|c| ConditionSummary {
                    condition_type: c.type_.clone(),
                    status: c.status.clone(),
                    reason: c.reason.clone(),
                    message: c.message.clone(),
                    last_transition: c.last_transition_time.as_ref().map(|t| t.0.to_string()),
                })
                .collect()
        })
        .unwrap_or_default();

    let result = serde_json::json!({
        "name": summary.name,
        "type": summary.type_,
        "assured_concurrency_shares": summary.assured_concurrency_shares,
        "limited_config": limited_config,
        "conditions": conditions,
        "labels": meta.labels.clone().unwrap_or_default(),
        "annotations": meta.annotations.clone().unwrap_or_default(),
        "created_at": summary.created_at,
    });

    serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::api::flowcontrol::v1::{
        FlowDistinguisherMethod, FlowSchemaSpec, LimitResponse, LimitedPriorityLevelConfiguration,
        PriorityLevelConfigurationReference, PriorityLevelConfigurationSpec,
    };
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

    #[test]
    fn tool_definitions_returns_four_tools() {
        let defs = tool_definitions();
        assert_eq!(defs.len(), 4);

        let names: Vec<&str> = defs.iter().map(|d| d["name"].as_str().unwrap()).collect();

        let mut unique = names.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(names.len(), unique.len(), "tool names must be unique");

        assert!(names.contains(&"list_flowschemas"));
        assert!(names.contains(&"get_flowschema"));
        assert!(names.contains(&"list_prioritylevelconfigs"));
        assert!(names.contains(&"get_prioritylevelconfig"));
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
    fn flowschema_summary_serialization() {
        let summary = FlowSchemaSummary {
            name: "system-leader-election".to_string(),
            priority_level_name: "leader-election".to_string(),
            matching_precedence: Some(100),
            created_at: Some("2024-01-01T00:00:00Z".to_string()),
        };

        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["name"], "system-leader-election");
        assert_eq!(json["priority_level_name"], "leader-election");
        assert_eq!(json["matching_precedence"], 100);
        assert_eq!(json["created_at"], "2024-01-01T00:00:00Z");
    }

    #[test]
    fn flowschema_summary_serialization_minimal() {
        let summary = FlowSchemaSummary {
            name: "catch-all".to_string(),
            priority_level_name: String::new(),
            matching_precedence: None,
            created_at: None,
        };

        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["name"], "catch-all");
        assert_eq!(json["priority_level_name"], "");
        assert!(json["matching_precedence"].is_null());
        assert!(json["created_at"].is_null());
    }

    #[test]
    fn prioritylevel_summary_serialization() {
        let summary = PriorityLevelSummary {
            name: "workload-high".to_string(),
            type_: "Limited".to_string(),
            assured_concurrency_shares: Some(40),
            created_at: Some("2024-01-01T00:00:00Z".to_string()),
        };

        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["name"], "workload-high");
        assert_eq!(json["type"], "Limited");
        assert_eq!(json["assured_concurrency_shares"], 40);
        assert_eq!(json["created_at"], "2024-01-01T00:00:00Z");
    }

    #[test]
    fn prioritylevel_summary_serialization_exempt() {
        let summary = PriorityLevelSummary {
            name: "exempt".to_string(),
            type_: "Exempt".to_string(),
            assured_concurrency_shares: None,
            created_at: None,
        };

        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["name"], "exempt");
        assert_eq!(json["type"], "Exempt");
        assert!(json["assured_concurrency_shares"].is_null());
        assert!(json["created_at"].is_null());
    }

    #[test]
    fn extract_flowschema_summary_from_object() {
        let fs = FlowSchema {
            metadata: ObjectMeta {
                name: Some("test-flow".to_string()),
                ..Default::default()
            },
            spec: Some(FlowSchemaSpec {
                priority_level_configuration: PriorityLevelConfigurationReference {
                    name: "workload-low".to_string(),
                },
                matching_precedence: Some(1000),
                distinguisher_method: Some(FlowDistinguisherMethod {
                    type_: "ByUser".to_string(),
                }),
                rules: None,
            }),
            status: None,
        };

        let summary = extract_flowschema_summary(&fs);
        assert_eq!(summary.name, "test-flow");
        assert_eq!(summary.priority_level_name, "workload-low");
        assert_eq!(summary.matching_precedence, Some(1000));
    }

    #[test]
    fn extract_flowschema_summary_minimal() {
        let fs = FlowSchema {
            metadata: ObjectMeta {
                name: Some("empty-flow".to_string()),
                ..Default::default()
            },
            spec: None,
            status: None,
        };

        let summary = extract_flowschema_summary(&fs);
        assert_eq!(summary.name, "empty-flow");
        assert_eq!(summary.priority_level_name, "");
        assert_eq!(summary.matching_precedence, None);
        assert!(summary.created_at.is_none());
    }

    #[test]
    fn extract_prioritylevel_summary_limited() {
        let plc = PriorityLevelConfiguration {
            metadata: ObjectMeta {
                name: Some("workload-high".to_string()),
                ..Default::default()
            },
            spec: Some(PriorityLevelConfigurationSpec {
                type_: "Limited".to_string(),
                limited: Some(LimitedPriorityLevelConfiguration {
                    nominal_concurrency_shares: Some(40),
                    borrowing_limit_percent: None,
                    lendable_percent: Some(50),
                    limit_response: Some(LimitResponse {
                        type_: "Queue".to_string(),
                        queuing: None,
                    }),
                }),
                exempt: None,
            }),
            status: None,
        };

        let summary = extract_prioritylevel_summary(&plc);
        assert_eq!(summary.name, "workload-high");
        assert_eq!(summary.type_, "Limited");
        assert_eq!(summary.assured_concurrency_shares, Some(40));
    }

    #[test]
    fn extract_prioritylevel_summary_exempt() {
        let plc = PriorityLevelConfiguration {
            metadata: ObjectMeta {
                name: Some("exempt".to_string()),
                ..Default::default()
            },
            spec: Some(PriorityLevelConfigurationSpec {
                type_: "Exempt".to_string(),
                limited: None,
                exempt: None,
            }),
            status: None,
        };

        let summary = extract_prioritylevel_summary(&plc);
        assert_eq!(summary.name, "exempt");
        assert_eq!(summary.type_, "Exempt");
        assert_eq!(summary.assured_concurrency_shares, None);
    }

    #[test]
    fn extract_prioritylevel_summary_minimal() {
        let plc = PriorityLevelConfiguration {
            metadata: ObjectMeta::default(),
            spec: None,
            status: None,
        };

        let summary = extract_prioritylevel_summary(&plc);
        assert_eq!(summary.name, "");
        assert_eq!(summary.type_, "");
        assert_eq!(summary.assured_concurrency_shares, None);
        assert!(summary.created_at.is_none());
    }
}
