use std::collections::BTreeMap;

use k8s_openapi::api::rbac::v1::ClusterRole;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::api::{DeleteParams, ListParams, Patch, PatchParams, PostParams};
use serde::Serialize;

use crate::client::K8sClient;

fn api(client: &K8sClient) -> kube::Api<ClusterRole> {
    kube::Api::all(client.inner().clone())
}

#[derive(Serialize, Debug)]
pub struct ClusterRoleSummary {
    pub name: String,
    pub rules_count: usize,
    pub aggregation_rule: Option<serde_json::Value>,
    pub created_at: Option<String>,
}

#[derive(Serialize, Debug)]
pub struct PolicyRuleSummary {
    pub api_groups: Vec<String>,
    pub resources: Vec<String>,
    pub verbs: Vec<String>,
}

fn extract_summary(cr: &ClusterRole) -> ClusterRoleSummary {
    let meta = &cr.metadata;
    let rules_count = cr.rules.as_ref().map(|r| r.len()).unwrap_or(0);

    let aggregation_rule = cr.aggregation_rule.as_ref().map(|agg| {
        let selectors = agg
            .cluster_role_selectors
            .as_ref()
            .map(|sels| {
                sels.iter()
                    .map(|sel| {
                        serde_json::json!({
                            "match_labels": sel.match_labels.clone().unwrap_or_default(),
                            "match_expressions": sel.match_expressions.clone().unwrap_or_default()
                                .iter()
                                .map(|expr| serde_json::json!({
                                    "key": expr.key,
                                    "operator": expr.operator,
                                    "values": expr.values.clone().unwrap_or_default(),
                                }))
                                .collect::<Vec<_>>(),
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        serde_json::json!({ "cluster_role_selectors": selectors })
    });

    ClusterRoleSummary {
        name: meta.name.clone().unwrap_or_default(),
        rules_count,
        aggregation_rule,
        created_at: meta.creation_timestamp.as_ref().map(|t| t.0.to_string()),
    }
}

fn extract_rules(cr: &ClusterRole) -> Vec<PolicyRuleSummary> {
    cr.rules
        .as_ref()
        .map(|rules| {
            rules
                .iter()
                .map(|rule| PolicyRuleSummary {
                    api_groups: rule.api_groups.clone().unwrap_or_default(),
                    resources: rule.resources.clone().unwrap_or_default(),
                    verbs: rule.verbs.clone(),
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn tool_definitions() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "name": "list_clusterroles",
            "description": "List all ClusterRoles. Returns name, rules count, aggregation_rule (if any), and created_at.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "required": [],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "get_clusterrole",
            "description": "Get a ClusterRole by name. Returns rules (api_groups, resources, verbs), labels, and annotations.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "ClusterRole name" }
                },
                "required": ["name"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "create_clusterrole",
            "description": "Create a ClusterRole with the given rules. Each rule must specify api_groups, resources, and verbs.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "ClusterRole name" },
                    "rules": {
                        "type": "array",
                        "description": "Policy rules for the ClusterRole",
                        "items": {
                            "type": "object",
                            "properties": {
                                "api_groups": {
                                    "type": "array",
                                    "items": { "type": "string" },
                                    "description": "API groups (e.g. [\"\", \"apps\"])"
                                },
                                "resources": {
                                    "type": "array",
                                    "items": { "type": "string" },
                                    "description": "Resources (e.g. [\"pods\", \"services\"])"
                                },
                                "verbs": {
                                    "type": "array",
                                    "items": { "type": "string" },
                                    "description": "Verbs (e.g. [\"get\", \"list\", \"watch\"])"
                                }
                            },
                            "required": ["api_groups", "resources", "verbs"],
                            "additionalProperties": false
                        }
                    }
                },
                "required": ["name", "rules"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "update_clusterrole",
            "description": "Update (merge patch) a ClusterRole's rules. Replaces the rules array with the provided rules.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "ClusterRole name" },
                    "rules": {
                        "type": "array",
                        "description": "New policy rules for the ClusterRole",
                        "items": {
                            "type": "object",
                            "properties": {
                                "api_groups": {
                                    "type": "array",
                                    "items": { "type": "string" },
                                    "description": "API groups (e.g. [\"\", \"apps\"])"
                                },
                                "resources": {
                                    "type": "array",
                                    "items": { "type": "string" },
                                    "description": "Resources (e.g. [\"pods\", \"services\"])"
                                },
                                "verbs": {
                                    "type": "array",
                                    "items": { "type": "string" },
                                    "description": "Verbs (e.g. [\"get\", \"list\", \"watch\"])"
                                }
                            },
                            "required": ["api_groups", "resources", "verbs"],
                            "additionalProperties": false
                        }
                    }
                },
                "required": ["name", "rules"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "delete_clusterrole",
            "description": "Delete a ClusterRole by name.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "ClusterRole name" }
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
        "list_clusterroles" => list_clusterroles(client).await,
        "get_clusterrole" => get_clusterrole(client, args).await,
        "create_clusterrole" => create_clusterrole(client, args).await,
        "update_clusterrole" => update_clusterrole(client, args).await,
        "delete_clusterrole" => delete_clusterrole(client, args).await,
        _ => return None,
    };
    Some(result)
}

async fn list_clusterroles(client: &K8sClient) -> Result<String, String> {
    let cr_api = api(client);
    let list = cr_api
        .list(&ListParams::default())
        .await
        .map_err(|e| e.to_string())?;

    let summaries: Vec<ClusterRoleSummary> = list.iter().map(|cr| extract_summary(cr)).collect();

    serde_json::to_string_pretty(&summaries).map_err(|e| e.to_string())
}

async fn get_clusterrole(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let name = args["name"].as_str().ok_or("name is required")?;
    let cr_api = api(client);
    let cr = cr_api.get(name).await.map_err(|e| e.to_string())?;

    let meta = &cr.metadata;
    let rules = extract_rules(&cr);

    let result = serde_json::json!({
        "name": meta.name.clone().unwrap_or_default(),
        "rules": rules,
        "labels": meta.labels.clone().unwrap_or_default(),
        "annotations": meta.annotations.clone().unwrap_or_default(),
    });

    serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
}

async fn create_clusterrole(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let name = args["name"].as_str().ok_or("name is required")?;
    let rules_value = args.get("rules").ok_or("rules is required")?;

    let rules = parse_policy_rules(rules_value)?;

    let mut labels = BTreeMap::new();
    labels.insert(
        "app.kubernetes.io/managed-by".to_string(),
        "mcp-k8s".to_string(),
    );

    let cr = ClusterRole {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            labels: Some(labels),
            ..Default::default()
        },
        rules: Some(rules),
        aggregation_rule: None,
    };

    let cr_api = api(client);
    let created = cr_api
        .create(&PostParams::default(), &cr)
        .await
        .map_err(|e| e.to_string())?;

    let summary = extract_summary(&created);
    serde_json::to_string_pretty(&summary).map_err(|e| e.to_string())
}

async fn update_clusterrole(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let name = args["name"].as_str().ok_or("name is required")?;
    let rules_value = args.get("rules").ok_or("rules is required")?;

    let rules: Vec<serde_json::Value> = rules_value
        .as_array()
        .ok_or("rules must be an array")?
        .iter()
        .map(|rule| {
            serde_json::json!({
                "apiGroups": rule.get("api_groups").cloned().unwrap_or(serde_json::json!([])),
                "resources": rule.get("resources").cloned().unwrap_or(serde_json::json!([])),
                "verbs": rule.get("verbs").cloned().unwrap_or(serde_json::json!([])),
            })
        })
        .collect();

    let patch = serde_json::json!({
        "rules": rules,
    });

    let cr_api = api(client);
    let patched = cr_api
        .patch(name, &PatchParams::apply("mcp-k8s"), &Patch::Merge(&patch))
        .await
        .map_err(|e| e.to_string())?;

    let summary = extract_summary(&patched);
    serde_json::to_string_pretty(&summary).map_err(|e| e.to_string())
}

async fn delete_clusterrole(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let name = args["name"].as_str().ok_or("name is required")?;

    let cr_api = api(client);
    cr_api
        .delete(name, &DeleteParams::default())
        .await
        .map_err(|e| e.to_string())?;

    let result = serde_json::json!({
        "deleted": name,
    });
    serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
}

fn parse_policy_rules(
    rules_value: &serde_json::Value,
) -> Result<Vec<k8s_openapi::api::rbac::v1::PolicyRule>, String> {
    let rules_arr = rules_value.as_array().ok_or("rules must be an array")?;

    rules_arr
        .iter()
        .map(|rule| {
            let api_groups: Vec<String> = rule
                .get("api_groups")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default();

            let resources: Vec<String> = rule
                .get("resources")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default();

            let verbs: Vec<String> = rule
                .get("verbs")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .ok_or("each rule must have verbs")?;

            Ok(k8s_openapi::api::rbac::v1::PolicyRule {
                api_groups: Some(api_groups),
                resources: Some(resources),
                verbs,
                non_resource_urls: None,
                resource_names: None,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::api::rbac::v1::{AggregationRule, PolicyRule};
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::{LabelSelector, ObjectMeta};
    use std::collections::BTreeMap;

    #[test]
    fn tool_definitions_returns_five_unique_tools() {
        let defs = tool_definitions();
        assert_eq!(defs.len(), 5);

        let names: Vec<&str> = defs.iter().map(|d| d["name"].as_str().unwrap()).collect();

        let mut unique = names.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(names.len(), unique.len(), "tool names must be unique");

        assert!(names.contains(&"list_clusterroles"));
        assert!(names.contains(&"get_clusterrole"));
        assert!(names.contains(&"create_clusterrole"));
        assert!(names.contains(&"update_clusterrole"));
        assert!(names.contains(&"delete_clusterrole"));
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
    fn tool_definitions_no_namespace_parameter() {
        let defs = tool_definitions();
        for def in &defs {
            let schema = def.get("inputSchema").unwrap();
            let props = schema.get("properties").unwrap().as_object().unwrap();
            assert!(
                !props.contains_key("namespace"),
                "ClusterRole tools must not have a namespace parameter, but {} does",
                def["name"]
            );
        }
    }

    #[test]
    fn clusterrole_summary_serialization() {
        let summary = ClusterRoleSummary {
            name: "cluster-admin".to_string(),
            rules_count: 1,
            aggregation_rule: None,
            created_at: Some("2024-01-01T00:00:00Z".to_string()),
        };

        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["name"], "cluster-admin");
        assert_eq!(json["rules_count"], 1);
        assert!(json["aggregation_rule"].is_null());
        assert_eq!(json["created_at"], "2024-01-01T00:00:00Z");
    }

    #[test]
    fn clusterrole_summary_serialization_with_aggregation() {
        let aggregation = serde_json::json!({
            "cluster_role_selectors": [{
                "match_labels": { "rbac.example.com/aggregate-to-admin": "true" },
                "match_expressions": []
            }]
        });

        let summary = ClusterRoleSummary {
            name: "admin".to_string(),
            rules_count: 0,
            aggregation_rule: Some(aggregation.clone()),
            created_at: None,
        };

        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["name"], "admin");
        assert_eq!(json["rules_count"], 0);
        assert_eq!(json["aggregation_rule"], aggregation);
        assert!(json["created_at"].is_null());
    }

    #[test]
    fn clusterrole_summary_serialization_empty_fields() {
        let summary = ClusterRoleSummary {
            name: "empty-role".to_string(),
            rules_count: 0,
            aggregation_rule: None,
            created_at: None,
        };

        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["name"], "empty-role");
        assert_eq!(json["rules_count"], 0);
        assert!(json["aggregation_rule"].is_null());
        assert!(json["created_at"].is_null());
    }

    #[test]
    fn policy_rule_summary_serialization() {
        let rule = PolicyRuleSummary {
            api_groups: vec!["".to_string(), "apps".to_string()],
            resources: vec!["pods".to_string(), "deployments".to_string()],
            verbs: vec!["get".to_string(), "list".to_string(), "watch".to_string()],
        };

        let json = serde_json::to_value(&rule).unwrap();
        assert_eq!(json["api_groups"].as_array().unwrap().len(), 2);
        assert_eq!(json["resources"].as_array().unwrap().len(), 2);
        assert_eq!(json["verbs"].as_array().unwrap().len(), 3);
        assert_eq!(json["api_groups"][0], "");
        assert_eq!(json["api_groups"][1], "apps");
        assert_eq!(json["resources"][0], "pods");
        assert_eq!(json["verbs"][0], "get");
    }

    fn make_test_clusterrole() -> ClusterRole {
        let mut labels = BTreeMap::new();
        labels.insert(
            "app.kubernetes.io/managed-by".to_string(),
            "mcp-k8s".to_string(),
        );

        let mut annotations = BTreeMap::new();
        annotations.insert("description".to_string(), "Test cluster role".to_string());

        ClusterRole {
            metadata: ObjectMeta {
                name: Some("test-role".to_string()),
                labels: Some(labels),
                annotations: Some(annotations),
                ..Default::default()
            },
            rules: Some(vec![
                PolicyRule {
                    api_groups: Some(vec!["".to_string()]),
                    resources: Some(vec!["pods".to_string(), "services".to_string()]),
                    verbs: vec!["get".to_string(), "list".to_string(), "watch".to_string()],
                    non_resource_urls: None,
                    resource_names: None,
                },
                PolicyRule {
                    api_groups: Some(vec!["apps".to_string()]),
                    resources: Some(vec!["deployments".to_string()]),
                    verbs: vec![
                        "get".to_string(),
                        "list".to_string(),
                        "create".to_string(),
                        "update".to_string(),
                    ],
                    non_resource_urls: None,
                    resource_names: None,
                },
            ]),
            aggregation_rule: None,
        }
    }

    fn make_aggregated_clusterrole() -> ClusterRole {
        let mut match_labels = BTreeMap::new();
        match_labels.insert(
            "rbac.example.com/aggregate-to-monitoring".to_string(),
            "true".to_string(),
        );

        ClusterRole {
            metadata: ObjectMeta {
                name: Some("monitoring-aggregate".to_string()),
                ..Default::default()
            },
            rules: Some(vec![]),
            aggregation_rule: Some(AggregationRule {
                cluster_role_selectors: Some(vec![LabelSelector {
                    match_labels: Some(match_labels),
                    match_expressions: None,
                }]),
            }),
        }
    }

    #[test]
    fn extract_summary_from_clusterrole() {
        let cr = make_test_clusterrole();
        let summary = extract_summary(&cr);

        assert_eq!(summary.name, "test-role");
        assert_eq!(summary.rules_count, 2);
        assert!(summary.aggregation_rule.is_none());
        assert!(summary.created_at.is_none());
    }

    #[test]
    fn extract_summary_from_aggregated_clusterrole() {
        let cr = make_aggregated_clusterrole();
        let summary = extract_summary(&cr);

        assert_eq!(summary.name, "monitoring-aggregate");
        assert_eq!(summary.rules_count, 0);
        assert!(summary.aggregation_rule.is_some());

        let agg = summary.aggregation_rule.unwrap();
        let selectors = agg["cluster_role_selectors"].as_array().unwrap();
        assert_eq!(selectors.len(), 1);
        assert_eq!(
            selectors[0]["match_labels"]["rbac.example.com/aggregate-to-monitoring"],
            "true"
        );
    }

    #[test]
    fn extract_summary_from_empty_clusterrole() {
        let cr = ClusterRole {
            metadata: ObjectMeta {
                name: Some("empty-role".to_string()),
                ..Default::default()
            },
            rules: None,
            aggregation_rule: None,
        };

        let summary = extract_summary(&cr);
        assert_eq!(summary.name, "empty-role");
        assert_eq!(summary.rules_count, 0);
        assert!(summary.aggregation_rule.is_none());
        assert!(summary.created_at.is_none());
    }

    #[test]
    fn extract_rules_from_clusterrole() {
        let cr = make_test_clusterrole();
        let rules = extract_rules(&cr);

        assert_eq!(rules.len(), 2);

        assert_eq!(rules[0].api_groups, vec![""]);
        assert_eq!(rules[0].resources, vec!["pods", "services"]);
        assert_eq!(rules[0].verbs, vec!["get", "list", "watch"]);

        assert_eq!(rules[1].api_groups, vec!["apps"]);
        assert_eq!(rules[1].resources, vec!["deployments"]);
        assert_eq!(rules[1].verbs, vec!["get", "list", "create", "update"]);
    }

    #[test]
    fn extract_rules_from_empty_clusterrole() {
        let cr = ClusterRole {
            metadata: ObjectMeta {
                name: Some("no-rules".to_string()),
                ..Default::default()
            },
            rules: None,
            aggregation_rule: None,
        };

        let rules = extract_rules(&cr);
        assert!(rules.is_empty());
    }

    #[test]
    fn parse_policy_rules_valid() {
        let input = serde_json::json!([
            {
                "api_groups": [""],
                "resources": ["pods"],
                "verbs": ["get", "list"]
            },
            {
                "api_groups": ["apps"],
                "resources": ["deployments"],
                "verbs": ["create", "update", "delete"]
            }
        ]);

        let rules = parse_policy_rules(&input).unwrap();
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0].api_groups.as_ref().unwrap(), &vec!["".to_string()]);
        assert_eq!(
            rules[0].resources.as_ref().unwrap(),
            &vec!["pods".to_string()]
        );
        assert_eq!(rules[0].verbs, vec!["get", "list"]);
        assert_eq!(
            rules[1].api_groups.as_ref().unwrap(),
            &vec!["apps".to_string()]
        );
        assert_eq!(
            rules[1].resources.as_ref().unwrap(),
            &vec!["deployments".to_string()]
        );
        assert_eq!(rules[1].verbs, vec!["create", "update", "delete"]);
    }

    #[test]
    fn parse_policy_rules_missing_verbs() {
        let input = serde_json::json!([
            {
                "api_groups": [""],
                "resources": ["pods"]
            }
        ]);

        let result = parse_policy_rules(&input);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "each rule must have verbs");
    }

    #[test]
    fn parse_policy_rules_not_array() {
        let input = serde_json::json!("not an array");
        let result = parse_policy_rules(&input);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "rules must be an array");
    }
}
