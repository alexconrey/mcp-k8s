use k8s_openapi::api::admissionregistration::v1::{
    MutatingWebhookConfiguration, ValidatingAdmissionPolicy, ValidatingWebhookConfiguration,
};
use kube::api::ListParams;
use serde::Serialize;

use crate::client::K8sClient;

fn mutating_api(client: &K8sClient) -> kube::Api<MutatingWebhookConfiguration> {
    kube::Api::all(client.inner().clone())
}

fn validating_api(client: &K8sClient) -> kube::Api<ValidatingWebhookConfiguration> {
    kube::Api::all(client.inner().clone())
}

fn policy_api(client: &K8sClient) -> kube::Api<ValidatingAdmissionPolicy> {
    kube::Api::all(client.inner().clone())
}

// --- Summaries ---

#[derive(Serialize, Debug)]
pub struct WebhookConfigSummary {
    pub name: String,
    pub webhooks_count: usize,
    pub created_at: Option<String>,
}

#[derive(Serialize, Debug)]
pub struct WebhookDetail {
    pub name: String,
    pub client_config: serde_json::Value,
    pub rules: Vec<serde_json::Value>,
    pub failure_policy: Option<String>,
    pub side_effects: Option<String>,
}

#[derive(Serialize, Debug)]
pub struct PolicySummary {
    pub name: String,
    pub validation_actions: Vec<String>,
    pub created_at: Option<String>,
}

// --- Tool definitions ---

pub fn tool_definitions() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "name": "list_mutatingwebhookconfigs",
            "description": "List all MutatingWebhookConfigurations in the cluster. Returns name, webhooks count, and created_at.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "required": [],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "get_mutatingwebhookconfig",
            "description": "Get a MutatingWebhookConfiguration by name. Returns webhooks (name, client_config, rules, failure_policy, side_effects), labels, and annotations.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "MutatingWebhookConfiguration name" }
                },
                "required": ["name"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "list_validatingwebhookconfigs",
            "description": "List all ValidatingWebhookConfigurations in the cluster. Returns name, webhooks count, and created_at.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "required": [],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "get_validatingwebhookconfig",
            "description": "Get a ValidatingWebhookConfiguration by name. Returns webhooks (name, client_config, rules, failure_policy, side_effects), labels, and annotations.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "ValidatingWebhookConfiguration name" }
                },
                "required": ["name"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "list_validatingadmissionpolicies",
            "description": "List all ValidatingAdmissionPolicies in the cluster. Returns name, validation_actions, and created_at.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "required": [],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "get_validatingadmissionpolicy",
            "description": "Get a ValidatingAdmissionPolicy by name. Returns spec (validations, match_constraints), labels, and annotations.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "ValidatingAdmissionPolicy name" }
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
        "list_mutatingwebhookconfigs" => list_mutatingwebhookconfigs(client).await,
        "get_mutatingwebhookconfig" => get_mutatingwebhookconfig(client, args).await,
        "list_validatingwebhookconfigs" => list_validatingwebhookconfigs(client).await,
        "get_validatingwebhookconfig" => get_validatingwebhookconfig(client, args).await,
        "list_validatingadmissionpolicies" => list_validatingadmissionpolicies(client).await,
        "get_validatingadmissionpolicy" => get_validatingadmissionpolicy(client, args).await,
        _ => return None,
    };
    Some(result)
}

// --- Helpers ---

fn extract_mutating_webhook_detail(
    wh: &k8s_openapi::api::admissionregistration::v1::MutatingWebhook,
) -> WebhookDetail {
    let client_config = {
        let cc = &wh.client_config;
        let service = cc.service.as_ref().map(|s| {
            serde_json::json!({
                "namespace": s.namespace,
                "name": s.name,
                "path": s.path,
                "port": s.port,
            })
        });
        serde_json::json!({
            "service": service,
            "url": cc.url,
        })
    };

    let rules: Vec<serde_json::Value> = wh
        .rules
        .as_ref()
        .map(|rs| {
            rs.iter()
                .map(|r| {
                    serde_json::json!({
                        "api_groups": r.api_groups,
                        "api_versions": r.api_versions,
                        "operations": r.operations,
                        "resources": r.resources,
                        "scope": r.scope,
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    WebhookDetail {
        name: wh.name.clone(),
        client_config,
        rules,
        failure_policy: wh.failure_policy.clone(),
        side_effects: Some(wh.side_effects.clone()),
    }
}

fn extract_validating_webhook_detail(
    wh: &k8s_openapi::api::admissionregistration::v1::ValidatingWebhook,
) -> WebhookDetail {
    let client_config = {
        let cc = &wh.client_config;
        let service = cc.service.as_ref().map(|s| {
            serde_json::json!({
                "namespace": s.namespace,
                "name": s.name,
                "path": s.path,
                "port": s.port,
            })
        });
        serde_json::json!({
            "service": service,
            "url": cc.url,
        })
    };

    let rules: Vec<serde_json::Value> = wh
        .rules
        .as_ref()
        .map(|rs| {
            rs.iter()
                .map(|r| {
                    serde_json::json!({
                        "api_groups": r.api_groups,
                        "api_versions": r.api_versions,
                        "operations": r.operations,
                        "resources": r.resources,
                        "scope": r.scope,
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    WebhookDetail {
        name: wh.name.clone(),
        client_config,
        rules,
        failure_policy: wh.failure_policy.clone(),
        side_effects: Some(wh.side_effects.clone()),
    }
}

// --- MutatingWebhookConfiguration ---

async fn list_mutatingwebhookconfigs(client: &K8sClient) -> Result<String, String> {
    let api = mutating_api(client);
    let list = api
        .list(&ListParams::default())
        .await
        .map_err(|e| e.to_string())?;

    let summaries: Vec<WebhookConfigSummary> = list
        .iter()
        .map(|mwc| {
            let meta = &mwc.metadata;
            WebhookConfigSummary {
                name: meta.name.clone().unwrap_or_default(),
                webhooks_count: mwc.webhooks.as_ref().map(|w| w.len()).unwrap_or(0),
                created_at: meta.creation_timestamp.as_ref().map(|t| t.0.to_string()),
            }
        })
        .collect();

    serde_json::to_string_pretty(&summaries).map_err(|e| e.to_string())
}

async fn get_mutatingwebhookconfig(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let name = args["name"].as_str().ok_or("name is required")?;
    let api = mutating_api(client);
    let mwc = api.get(name).await.map_err(|e| e.to_string())?;

    let meta = &mwc.metadata;
    let webhooks: Vec<serde_json::Value> = mwc
        .webhooks
        .as_ref()
        .map(|ws| {
            ws.iter()
                .map(|wh| {
                    let detail = extract_mutating_webhook_detail(wh);
                    serde_json::to_value(detail).unwrap_or_default()
                })
                .collect()
        })
        .unwrap_or_default();

    let result = serde_json::json!({
        "name": meta.name.clone().unwrap_or_default(),
        "webhooks": webhooks,
        "labels": meta.labels.clone().unwrap_or_default(),
        "annotations": meta.annotations.clone().unwrap_or_default(),
    });

    serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
}

// --- ValidatingWebhookConfiguration ---

async fn list_validatingwebhookconfigs(client: &K8sClient) -> Result<String, String> {
    let api = validating_api(client);
    let list = api
        .list(&ListParams::default())
        .await
        .map_err(|e| e.to_string())?;

    let summaries: Vec<WebhookConfigSummary> = list
        .iter()
        .map(|vwc| {
            let meta = &vwc.metadata;
            WebhookConfigSummary {
                name: meta.name.clone().unwrap_or_default(),
                webhooks_count: vwc.webhooks.as_ref().map(|w| w.len()).unwrap_or(0),
                created_at: meta.creation_timestamp.as_ref().map(|t| t.0.to_string()),
            }
        })
        .collect();

    serde_json::to_string_pretty(&summaries).map_err(|e| e.to_string())
}

async fn get_validatingwebhookconfig(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let name = args["name"].as_str().ok_or("name is required")?;
    let api = validating_api(client);
    let vwc = api.get(name).await.map_err(|e| e.to_string())?;

    let meta = &vwc.metadata;
    let webhooks: Vec<serde_json::Value> = vwc
        .webhooks
        .as_ref()
        .map(|ws| {
            ws.iter()
                .map(|wh| {
                    let detail = extract_validating_webhook_detail(wh);
                    serde_json::to_value(detail).unwrap_or_default()
                })
                .collect()
        })
        .unwrap_or_default();

    let result = serde_json::json!({
        "name": meta.name.clone().unwrap_or_default(),
        "webhooks": webhooks,
        "labels": meta.labels.clone().unwrap_or_default(),
        "annotations": meta.annotations.clone().unwrap_or_default(),
    });

    serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
}

// --- ValidatingAdmissionPolicy ---

async fn list_validatingadmissionpolicies(client: &K8sClient) -> Result<String, String> {
    let api = policy_api(client);
    let list = api
        .list(&ListParams::default())
        .await
        .map_err(|e| e.to_string())?;

    let summaries: Vec<PolicySummary> = list
        .iter()
        .map(|vap| {
            let meta = &vap.metadata;
            let validation_actions = vap
                .spec
                .as_ref()
                .and_then(|s| s.failure_policy.as_ref())
                .map(|fp| vec![fp.clone()])
                .unwrap_or_default();

            PolicySummary {
                name: meta.name.clone().unwrap_or_default(),
                validation_actions,
                created_at: meta.creation_timestamp.as_ref().map(|t| t.0.to_string()),
            }
        })
        .collect();

    serde_json::to_string_pretty(&summaries).map_err(|e| e.to_string())
}

async fn get_validatingadmissionpolicy(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let name = args["name"].as_str().ok_or("name is required")?;
    let api = policy_api(client);
    let vap = api.get(name).await.map_err(|e| e.to_string())?;

    let meta = &vap.metadata;

    let spec = vap.spec.as_ref();

    let validations: Vec<serde_json::Value> = spec
        .and_then(|s| s.validations.as_ref())
        .map(|vs| {
            vs.iter()
                .map(|v| {
                    serde_json::json!({
                        "expression": v.expression,
                        "message": v.message,
                        "message_expression": v.message_expression,
                        "reason": v.reason,
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    let match_constraints = spec.and_then(|s| s.match_constraints.as_ref()).map(|mc| {
        let resource_rules: Vec<serde_json::Value> = mc
            .resource_rules
            .as_ref()
            .map(|rrs| {
                rrs.iter()
                    .map(|rr| {
                        serde_json::json!({
                            "api_groups": rr.api_groups,
                            "api_versions": rr.api_versions,
                            "operations": rr.operations,
                            "resources": rr.resources,
                            "scope": rr.scope,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        serde_json::json!({
            "match_policy": mc.match_policy,
            "resource_rules": resource_rules,
        })
    });

    let result = serde_json::json!({
        "name": meta.name.clone().unwrap_or_default(),
        "spec": {
            "validations": validations,
            "match_constraints": match_constraints,
        },
        "labels": meta.labels.clone().unwrap_or_default(),
        "annotations": meta.annotations.clone().unwrap_or_default(),
    });

    serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_definitions_returns_six_tools() {
        let defs = tool_definitions();
        assert_eq!(defs.len(), 6);

        let names: Vec<&str> = defs.iter().map(|d| d["name"].as_str().unwrap()).collect();

        let mut unique = names.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(names.len(), unique.len(), "tool names must be unique");

        assert!(names.contains(&"list_mutatingwebhookconfigs"));
        assert!(names.contains(&"get_mutatingwebhookconfig"));
        assert!(names.contains(&"list_validatingwebhookconfigs"));
        assert!(names.contains(&"get_validatingwebhookconfig"));
        assert!(names.contains(&"list_validatingadmissionpolicies"));
        assert!(names.contains(&"get_validatingadmissionpolicy"));
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
    fn webhook_config_summary_serialization() {
        let summary = WebhookConfigSummary {
            name: "my-webhook".to_string(),
            webhooks_count: 3,
            created_at: Some("2024-06-01T00:00:00Z".to_string()),
        };

        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["name"], "my-webhook");
        assert_eq!(json["webhooks_count"], 3);
        assert_eq!(json["created_at"], "2024-06-01T00:00:00Z");
    }

    #[test]
    fn webhook_detail_serialization() {
        let detail = WebhookDetail {
            name: "hook.example.com".to_string(),
            client_config: serde_json::json!({
                "service": {
                    "namespace": "webhooks",
                    "name": "my-svc",
                    "path": "/validate",
                    "port": 443,
                },
                "url": null,
            }),
            rules: vec![serde_json::json!({
                "api_groups": [""],
                "api_versions": ["v1"],
                "operations": ["CREATE"],
                "resources": ["pods"],
                "scope": "*",
            })],
            failure_policy: Some("Fail".to_string()),
            side_effects: Some("None".to_string()),
        };

        let json = serde_json::to_value(&detail).unwrap();
        assert_eq!(json["name"], "hook.example.com");
        assert_eq!(json["failure_policy"], "Fail");
        assert_eq!(json["side_effects"], "None");
        assert!(json["client_config"]["service"]["namespace"].is_string());
        assert_eq!(json["rules"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn policy_summary_serialization() {
        let summary = PolicySummary {
            name: "deny-privileged".to_string(),
            validation_actions: vec!["Deny".to_string(), "Audit".to_string()],
            created_at: Some("2024-06-01T00:00:00Z".to_string()),
        };

        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["name"], "deny-privileged");
        assert_eq!(json["validation_actions"].as_array().unwrap().len(), 2);
        assert_eq!(json["validation_actions"][0], "Deny");
        assert_eq!(json["created_at"], "2024-06-01T00:00:00Z");
    }

    #[test]
    fn policy_summary_empty_fields() {
        let summary = PolicySummary {
            name: "empty-policy".to_string(),
            validation_actions: vec![],
            created_at: None,
        };

        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["name"], "empty-policy");
        assert!(json["validation_actions"].as_array().unwrap().is_empty());
        assert!(json["created_at"].is_null());
    }
}
