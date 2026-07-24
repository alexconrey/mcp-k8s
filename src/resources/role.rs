use std::collections::BTreeMap;

use k8s_openapi::api::rbac::v1::{PolicyRule, Role};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::api::{DeleteParams, ListParams, Patch, PatchParams, PostParams};
use serde::Serialize;

use crate::client::K8sClient;

fn api(client: &K8sClient, ns: &str) -> Result<kube::Api<Role>, String> {
    if !client.is_namespace_allowed(ns) {
        return Err(format!("Namespace '{ns}' is not in the allowed list"));
    }
    Ok(kube::Api::namespaced(client.inner().clone(), ns))
}

#[derive(Serialize, Debug)]
pub struct PolicyRuleSummary {
    pub api_groups: Vec<String>,
    pub resources: Vec<String>,
    pub verbs: Vec<String>,
}

#[derive(Serialize, Debug)]
pub struct RoleSummary {
    pub name: String,
    pub namespace: String,
    pub rules_count: usize,
    pub created_at: Option<String>,
}

fn extract_policy_rule_summary(rule: &PolicyRule) -> PolicyRuleSummary {
    PolicyRuleSummary {
        api_groups: rule.api_groups.clone().unwrap_or_default(),
        resources: rule.resources.clone().unwrap_or_default(),
        verbs: rule.verbs.clone(),
    }
}

fn extract_summary(role: &Role) -> RoleSummary {
    let meta = &role.metadata;
    let rules_count = role.rules.as_ref().map(|r| r.len()).unwrap_or(0);

    RoleSummary {
        name: meta.name.clone().unwrap_or_default(),
        namespace: meta.namespace.clone().unwrap_or_default(),
        rules_count,
        created_at: meta.creation_timestamp.as_ref().map(|t| t.0.to_string()),
    }
}

pub fn tool_definitions() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "name": "list_roles",
            "description": "List roles in a namespace. Returns name, namespace, rules count, and created_at.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "label_selector": { "type": "string", "description": "Label selector to filter results (e.g. app=nginx)" }
                },
                "required": ["namespace"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "get_role",
            "description": "Get a role by name. Returns name, namespace, rules (array of {api_groups, resources, verbs}), labels, and annotations.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "Role name" }
                },
                "required": ["namespace", "name"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "create_role",
            "description": "Create a role in a namespace with the specified policy rules.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "Role name" },
                    "rules": {
                        "type": "array",
                        "description": "Array of policy rules",
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
                "required": ["namespace", "name", "rules"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "update_role",
            "description": "Update a role's rules using a merge patch. Replaces the entire rules list.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "Role name" },
                    "rules": {
                        "type": "array",
                        "description": "New policy rules to set on the role",
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
                "required": ["namespace", "name", "rules"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "delete_role",
            "description": "Delete a role by name from a namespace.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "Role name" }
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
        "list_roles" => list_roles(client, args).await,
        "get_role" => get_role(client, args).await,
        "create_role" => create_role(client, args).await,
        "update_role" => update_role(client, args).await,
        "delete_role" => delete_role(client, args).await,
        _ => return None,
    };
    Some(result)
}

fn parse_rules(args: &serde_json::Value) -> Result<Vec<PolicyRule>, String> {
    let rules_val = args
        .get("rules")
        .ok_or("rules is required")?;
    let rules_arr = rules_val
        .as_array()
        .ok_or("rules must be an array")?;

    rules_arr
        .iter()
        .map(|r| {
            let api_groups: Vec<String> = serde_json::from_value(
                r.get("api_groups")
                    .ok_or("api_groups is required in each rule")?
                    .clone(),
            )
            .map_err(|e| format!("invalid api_groups: {e}"))?;

            let resources: Vec<String> = serde_json::from_value(
                r.get("resources")
                    .ok_or("resources is required in each rule")?
                    .clone(),
            )
            .map_err(|e| format!("invalid resources: {e}"))?;

            let verbs: Vec<String> = serde_json::from_value(
                r.get("verbs")
                    .ok_or("verbs is required in each rule")?
                    .clone(),
            )
            .map_err(|e| format!("invalid verbs: {e}"))?;

            Ok(PolicyRule {
                api_groups: Some(api_groups),
                resources: Some(resources),
                verbs,
                ..Default::default()
            })
        })
        .collect()
}

async fn list_roles(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let role_api = api(client, ns)?;
    let label_selector = args["label_selector"].as_str();
    let mut lp = ListParams::default();
    if let Some(sel) = label_selector {
        lp = lp.labels(sel);
    }
    let list = role_api
        .list(&lp)
        .await
        .map_err(|e| e.to_string())?;

    let summaries: Vec<serde_json::Value> = list
        .iter()
        .map(|role| {
            let s = extract_summary(role);
            serde_json::json!({
                "name": s.name,
                "namespace": s.namespace,
                "rules_count": s.rules_count,
                "created_at": s.created_at,
            })
        })
        .collect();

    serde_json::to_string_pretty(&summaries).map_err(|e| e.to_string())
}

async fn get_role(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;
    let role_api = api(client, ns)?;
    let role = role_api.get(name).await.map_err(|e| e.to_string())?;

    let meta = &role.metadata;
    let rules: Vec<PolicyRuleSummary> = role
        .rules
        .as_ref()
        .map(|rs| rs.iter().map(extract_policy_rule_summary).collect())
        .unwrap_or_default();

    let result = serde_json::json!({
        "name": meta.name.clone().unwrap_or_default(),
        "namespace": meta.namespace.clone().unwrap_or_default(),
        "rules": rules,
        "labels": meta.labels.clone().unwrap_or_default(),
        "annotations": meta.annotations.clone().unwrap_or_default(),
    });

    serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
}

async fn create_role(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;
    let rules = parse_rules(args)?;

    let mut labels = BTreeMap::new();
    labels.insert(
        "app.kubernetes.io/managed-by".to_string(),
        "mcp-k8s".to_string(),
    );

    let role = Role {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            namespace: Some(ns.to_string()),
            labels: Some(labels),
            ..Default::default()
        },
        rules: Some(rules),
    };

    let role_api = api(client, ns)?;
    let created = role_api
        .create(&PostParams::default(), &role)
        .await
        .map_err(|e| e.to_string())?;

    let summary = extract_summary(&created);
    serde_json::to_string_pretty(&summary).map_err(|e| e.to_string())
}

async fn update_role(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;
    let rules = parse_rules(args)?;

    let rules_json: Vec<serde_json::Value> = rules
        .iter()
        .map(|r| {
            serde_json::json!({
                "apiGroups": r.api_groups,
                "resources": r.resources,
                "verbs": r.verbs,
            })
        })
        .collect();

    let patch = serde_json::json!({
        "rules": rules_json,
    });

    let role_api = api(client, ns)?;
    let patched = role_api
        .patch(
            name,
            &PatchParams::apply("mcp-k8s"),
            &Patch::Merge(&patch),
        )
        .await
        .map_err(|e| e.to_string())?;

    let summary = extract_summary(&patched);
    serde_json::to_string_pretty(&summary).map_err(|e| e.to_string())
}

async fn delete_role(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;

    let role_api = api(client, ns)?;
    role_api
        .delete(name, &DeleteParams::default())
        .await
        .map_err(|e| e.to_string())?;

    let result = serde_json::json!({
        "deleted": name,
        "namespace": ns,
    });
    serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_definitions_returns_five_unique_tools() {
        let defs = tool_definitions();
        assert_eq!(defs.len(), 5);

        let names: Vec<&str> = defs
            .iter()
            .map(|d| d["name"].as_str().unwrap())
            .collect();

        let mut unique = names.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(names.len(), unique.len(), "tool names must be unique");

        assert!(names.contains(&"list_roles"));
        assert!(names.contains(&"get_role"));
        assert!(names.contains(&"create_role"));
        assert!(names.contains(&"update_role"));
        assert!(names.contains(&"delete_role"));
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
    fn role_summary_serialization() {
        let summary = RoleSummary {
            name: "pod-reader".to_string(),
            namespace: "default".to_string(),
            rules_count: 2,
            created_at: Some("2024-01-01T00:00:00Z".to_string()),
        };

        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["name"], "pod-reader");
        assert_eq!(json["namespace"], "default");
        assert_eq!(json["rules_count"], 2);
        assert_eq!(json["created_at"], "2024-01-01T00:00:00Z");
    }

    #[test]
    fn role_summary_serialization_empty_fields() {
        let summary = RoleSummary {
            name: "empty-role".to_string(),
            namespace: "ns".to_string(),
            rules_count: 0,
            created_at: None,
        };

        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["name"], "empty-role");
        assert_eq!(json["rules_count"], 0);
        assert!(json["created_at"].is_null());
    }

    #[test]
    fn policy_rule_summary_serialization() {
        let summary = PolicyRuleSummary {
            api_groups: vec!["".to_string(), "apps".to_string()],
            resources: vec!["pods".to_string(), "deployments".to_string()],
            verbs: vec!["get".to_string(), "list".to_string(), "watch".to_string()],
        };

        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["api_groups"].as_array().unwrap().len(), 2);
        assert_eq!(json["resources"].as_array().unwrap().len(), 2);
        assert_eq!(json["verbs"].as_array().unwrap().len(), 3);
        assert_eq!(json["api_groups"][0], "");
        assert_eq!(json["api_groups"][1], "apps");
        assert_eq!(json["resources"][0], "pods");
        assert_eq!(json["verbs"][0], "get");
    }

    #[test]
    fn policy_rule_summary_serialization_empty() {
        let summary = PolicyRuleSummary {
            api_groups: vec![],
            resources: vec![],
            verbs: vec![],
        };

        let json = serde_json::to_value(&summary).unwrap();
        assert!(json["api_groups"].as_array().unwrap().is_empty());
        assert!(json["resources"].as_array().unwrap().is_empty());
        assert!(json["verbs"].as_array().unwrap().is_empty());
    }

    #[test]
    fn extract_summary_from_role() {
        let role = Role {
            metadata: ObjectMeta {
                name: Some("test-role".to_string()),
                namespace: Some("prod".to_string()),
                labels: Some(BTreeMap::from([(
                    "app".to_string(),
                    "myapp".to_string(),
                )])),
                ..Default::default()
            },
            rules: Some(vec![
                PolicyRule {
                    api_groups: Some(vec!["".to_string()]),
                    resources: Some(vec!["pods".to_string()]),
                    verbs: vec!["get".to_string(), "list".to_string()],
                    ..Default::default()
                },
                PolicyRule {
                    api_groups: Some(vec!["apps".to_string()]),
                    resources: Some(vec!["deployments".to_string()]),
                    verbs: vec!["get".to_string()],
                    ..Default::default()
                },
            ]),
        };

        let summary = extract_summary(&role);
        assert_eq!(summary.name, "test-role");
        assert_eq!(summary.namespace, "prod");
        assert_eq!(summary.rules_count, 2);
        assert!(summary.created_at.is_none());
    }

    #[test]
    fn extract_summary_from_empty_role() {
        let role = Role {
            metadata: ObjectMeta {
                name: Some("empty-role".to_string()),
                namespace: Some("default".to_string()),
                ..Default::default()
            },
            rules: None,
        };

        let summary = extract_summary(&role);
        assert_eq!(summary.name, "empty-role");
        assert_eq!(summary.namespace, "default");
        assert_eq!(summary.rules_count, 0);
        assert!(summary.created_at.is_none());
    }

    #[test]
    fn extract_policy_rule_summary_from_rule() {
        let rule = PolicyRule {
            api_groups: Some(vec!["".to_string(), "apps".to_string()]),
            resources: Some(vec!["pods".to_string(), "services".to_string()]),
            verbs: vec![
                "get".to_string(),
                "list".to_string(),
                "watch".to_string(),
            ],
            ..Default::default()
        };

        let summary = extract_policy_rule_summary(&rule);
        assert_eq!(summary.api_groups, vec!["", "apps"]);
        assert_eq!(summary.resources, vec!["pods", "services"]);
        assert_eq!(summary.verbs, vec!["get", "list", "watch"]);
    }

    #[test]
    fn extract_policy_rule_summary_with_none_fields() {
        let rule = PolicyRule {
            api_groups: None,
            resources: None,
            verbs: vec!["get".to_string()],
            ..Default::default()
        };

        let summary = extract_policy_rule_summary(&rule);
        assert!(summary.api_groups.is_empty());
        assert!(summary.resources.is_empty());
        assert_eq!(summary.verbs, vec!["get"]);
    }
}
