
use k8s_openapi::api::scheduling::v1::PriorityClass;
use kube::api::ListParams;
use serde::Serialize;

use crate::client::K8sClient;

fn api(client: &K8sClient) -> kube::Api<PriorityClass> {
    kube::Api::all(client.inner().clone())
}

#[derive(Serialize, Debug)]
pub struct PriorityClassSummary {
    pub name: String,
    pub value: i32,
    pub global_default: bool,
    pub preemption_policy: Option<String>,
    pub description: Option<String>,
    pub created_at: Option<String>,
}

fn extract_summary(pc: &PriorityClass) -> PriorityClassSummary {
    let meta = &pc.metadata;

    PriorityClassSummary {
        name: meta.name.clone().unwrap_or_default(),
        value: pc.value,
        global_default: pc.global_default.unwrap_or(false),
        preemption_policy: pc.preemption_policy.clone(),
        description: pc.description.clone(),
        created_at: meta.creation_timestamp.as_ref().map(|t| t.0.to_string()),
    }
}

pub fn tool_definitions() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "name": "list_priorityclasses",
            "description": "List all PriorityClasses. Returns name, value, global_default, preemption_policy, description, and created_at.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "required": [],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "get_priorityclass",
            "description": "Get a PriorityClass by name. Returns name, value, global_default, preemption_policy, description, created_at, labels, and annotations.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "PriorityClass name" }
                },
                "required": ["name"],
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
        "list_priorityclasses" => list_priorityclasses(client).await,
        "get_priorityclass" => get_priorityclass(client, args).await,
        _ => return None,
    };
    Some(result)
}

async fn list_priorityclasses(client: &K8sClient) -> Result<String, String> {
    let pc_api = api(client);
    let list = pc_api
        .list(&ListParams::default())
        .await
        .map_err(|e| e.to_string())?;

    let summaries: Vec<PriorityClassSummary> =
        list.iter().map(|pc| extract_summary(pc)).collect();

    serde_json::to_string_pretty(&summaries).map_err(|e| e.to_string())
}

async fn get_priorityclass(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let name = args["name"].as_str().ok_or("name is required")?;
    let pc_api = api(client);
    let pc = pc_api.get(name).await.map_err(|e| e.to_string())?;

    let summary = extract_summary(&pc);
    let meta = &pc.metadata;

    let result = serde_json::json!({
        "name": summary.name,
        "value": summary.value,
        "global_default": summary.global_default,
        "preemption_policy": summary.preemption_policy,
        "description": summary.description,
        "created_at": summary.created_at,
        "labels": meta.labels.clone().unwrap_or_default(),
        "annotations": meta.annotations.clone().unwrap_or_default(),
    });

    serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

    #[test]
    fn tool_definitions_returns_two_tools() {
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

        assert!(names.contains(&"list_priorityclasses"));
        assert!(names.contains(&"get_priorityclass"));
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
    fn priorityclass_summary_serialization() {
        let summary = PriorityClassSummary {
            name: "high-priority".to_string(),
            value: 1000000,
            global_default: false,
            preemption_policy: Some("PreemptLowerPriority".to_string()),
            description: Some("Used for critical workloads".to_string()),
            created_at: Some("2024-01-01T00:00:00Z".to_string()),
        };

        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["name"], "high-priority");
        assert_eq!(json["value"], 1000000);
        assert_eq!(json["global_default"], false);
        assert_eq!(json["preemption_policy"], "PreemptLowerPriority");
        assert_eq!(json["description"], "Used for critical workloads");
        assert_eq!(json["created_at"], "2024-01-01T00:00:00Z");
    }

    #[test]
    fn priorityclass_summary_serialization_empty_fields() {
        let summary = PriorityClassSummary {
            name: "default-priority".to_string(),
            value: 0,
            global_default: true,
            preemption_policy: None,
            description: None,
            created_at: None,
        };

        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["name"], "default-priority");
        assert_eq!(json["value"], 0);
        assert_eq!(json["global_default"], true);
        assert!(json["preemption_policy"].is_null());
        assert!(json["description"].is_null());
        assert!(json["created_at"].is_null());
    }

    #[test]
    fn extract_summary_from_priorityclass() {
        let pc = PriorityClass {
            metadata: ObjectMeta {
                name: Some("system-critical".to_string()),
                labels: Some(BTreeMap::from([(
                    "tier".to_string(),
                    "system".to_string(),
                )])),
                annotations: Some(BTreeMap::from([(
                    "note".to_string(),
                    "do not delete".to_string(),
                )])),
                ..Default::default()
            },
            value: 2000000000,
            global_default: Some(false),
            preemption_policy: Some("PreemptLowerPriority".to_string()),
            description: Some("System-critical pods".to_string()),
        };

        let summary = extract_summary(&pc);
        assert_eq!(summary.name, "system-critical");
        assert_eq!(summary.value, 2000000000);
        assert!(!summary.global_default);
        assert_eq!(
            summary.preemption_policy.as_deref(),
            Some("PreemptLowerPriority")
        );
        assert_eq!(summary.description.as_deref(), Some("System-critical pods"));
        assert!(summary.created_at.is_none());
    }

    #[test]
    fn extract_summary_from_minimal_priorityclass() {
        let pc = PriorityClass {
            metadata: ObjectMeta {
                name: Some("low-priority".to_string()),
                ..Default::default()
            },
            value: 100,
            global_default: None,
            preemption_policy: None,
            description: None,
        };

        let summary = extract_summary(&pc);
        assert_eq!(summary.name, "low-priority");
        assert_eq!(summary.value, 100);
        assert!(!summary.global_default);
        assert!(summary.preemption_policy.is_none());
        assert!(summary.description.is_none());
        assert!(summary.created_at.is_none());
    }

    #[test]
    fn extract_summary_global_default_true() {
        let pc = PriorityClass {
            metadata: ObjectMeta {
                name: Some("cluster-default".to_string()),
                ..Default::default()
            },
            value: 0,
            global_default: Some(true),
            preemption_policy: Some("Never".to_string()),
            description: Some("Default priority class for the cluster".to_string()),
        };

        let summary = extract_summary(&pc);
        assert_eq!(summary.name, "cluster-default");
        assert_eq!(summary.value, 0);
        assert!(summary.global_default);
        assert_eq!(summary.preemption_policy.as_deref(), Some("Never"));
        assert_eq!(
            summary.description.as_deref(),
            Some("Default priority class for the cluster")
        );
    }
}
