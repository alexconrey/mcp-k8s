use k8s_openapi::api::admissionregistration::v1alpha1::{
    MutatingAdmissionPolicy, MutatingAdmissionPolicyBinding,
};
use kube::api::ListParams;

use crate::client::K8sClient;

fn policy_api(client: &K8sClient) -> kube::Api<MutatingAdmissionPolicy> {
    kube::Api::all(client.inner().clone())
}

fn binding_api(client: &K8sClient) -> kube::Api<MutatingAdmissionPolicyBinding> {
    kube::Api::all(client.inner().clone())
}

// --- Tool definitions ---

pub fn tool_definitions() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "name": "list_mutatingadmissionpolicies",
            "description": "List all MutatingAdmissionPolicies (v1alpha1) in the cluster. Returns name and created_at.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "required": [],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "get_mutatingadmissionpolicy",
            "description": "Get a MutatingAdmissionPolicy (v1alpha1) by name. Returns name, spec (mutations, match_constraints, failure_policy), labels, and annotations.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "MutatingAdmissionPolicy name" }
                },
                "required": ["name"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "list_mutatingadmissionpolicybindings",
            "description": "List all MutatingAdmissionPolicyBindings (v1alpha1) in the cluster. Returns name, policy_name, and created_at.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "required": [],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "get_mutatingadmissionpolicybinding",
            "description": "Get a MutatingAdmissionPolicyBinding (v1alpha1) by name. Returns spec (policy_name, match_resources, param_ref), labels, and annotations.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "MutatingAdmissionPolicyBinding name" }
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
        "list_mutatingadmissionpolicies" => list_mutatingadmissionpolicies(client).await,
        "get_mutatingadmissionpolicy" => get_mutatingadmissionpolicy(client, args).await,
        "list_mutatingadmissionpolicybindings" => {
            list_mutatingadmissionpolicybindings(client).await
        }
        "get_mutatingadmissionpolicybinding" => {
            get_mutatingadmissionpolicybinding(client, args).await
        }
        _ => return None,
    };
    Some(result)
}

// --- MutatingAdmissionPolicy ---

async fn list_mutatingadmissionpolicies(client: &K8sClient) -> Result<String, String> {
    let api = policy_api(client);
    let list = api
        .list(&ListParams::default())
        .await
        .map_err(|e| e.to_string())?;

    let summaries: Vec<serde_json::Value> = list
        .iter()
        .map(|policy| {
            let meta = &policy.metadata;
            serde_json::json!({
                "name": meta.name.clone().unwrap_or_default(),
                "created_at": meta.creation_timestamp.as_ref().map(|t| t.0.to_string()),
            })
        })
        .collect();

    serde_json::to_string_pretty(&summaries).map_err(|e| e.to_string())
}

async fn get_mutatingadmissionpolicy(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let name = args["name"].as_str().ok_or("name is required")?;
    let api = policy_api(client);
    let policy = api.get(name).await.map_err(|e| e.to_string())?;

    let meta = &policy.metadata;
    let spec = policy.spec.as_ref();

    let mutations: Vec<serde_json::Value> = spec
        .and_then(|s| s.mutations.as_ref())
        .map(|ms| {
            ms.iter()
                .map(|m| {
                    serde_json::json!({
                        "patch_type": m.patch_type,
                        "apply_configuration": m.apply_configuration.as_ref().map(|ac| {
                            serde_json::json!({ "expression": ac.expression })
                        }),
                        "json_patch": m.json_patch.as_ref().map(|jp| {
                            serde_json::json!({ "expression": jp.expression })
                        }),
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
                            "resource_names": rr.resource_names,
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

    let failure_policy = spec.and_then(|s| s.failure_policy.as_ref());

    let result = serde_json::json!({
        "name": meta.name.clone().unwrap_or_default(),
        "spec": {
            "mutations": mutations,
            "match_constraints": match_constraints,
            "failure_policy": failure_policy,
        },
        "labels": meta.labels.clone().unwrap_or_default(),
        "annotations": meta.annotations.clone().unwrap_or_default(),
    });

    serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
}

// --- MutatingAdmissionPolicyBinding ---

async fn list_mutatingadmissionpolicybindings(client: &K8sClient) -> Result<String, String> {
    let api = binding_api(client);
    let list = api
        .list(&ListParams::default())
        .await
        .map_err(|e| e.to_string())?;

    let summaries: Vec<serde_json::Value> = list
        .iter()
        .map(|binding| {
            let meta = &binding.metadata;
            let policy_name = binding
                .spec
                .as_ref()
                .and_then(|s| s.policy_name.as_ref());
            serde_json::json!({
                "name": meta.name.clone().unwrap_or_default(),
                "policy_name": policy_name,
                "created_at": meta.creation_timestamp.as_ref().map(|t| t.0.to_string()),
            })
        })
        .collect();

    serde_json::to_string_pretty(&summaries).map_err(|e| e.to_string())
}

async fn get_mutatingadmissionpolicybinding(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let name = args["name"].as_str().ok_or("name is required")?;
    let api = binding_api(client);
    let binding = api.get(name).await.map_err(|e| e.to_string())?;

    let meta = &binding.metadata;
    let spec = binding.spec.as_ref();

    let policy_name = spec.and_then(|s| s.policy_name.as_ref());

    let match_resources = spec.and_then(|s| s.match_resources.as_ref()).map(|mr| {
        let resource_rules: Vec<serde_json::Value> = mr
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
                            "resource_names": rr.resource_names,
                            "scope": rr.scope,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        serde_json::json!({
            "match_policy": mr.match_policy,
            "resource_rules": resource_rules,
        })
    });

    let param_ref = spec.and_then(|s| s.param_ref.as_ref()).map(|pr| {
        serde_json::json!({
            "name": pr.name,
            "namespace": pr.namespace,
            "parameter_not_found_action": pr.parameter_not_found_action,
            "selector": pr.selector.as_ref().map(|sel| {
                serde_json::json!({
                    "match_labels": sel.match_labels,
                    "match_expressions": sel.match_expressions,
                })
            }),
        })
    });

    let result = serde_json::json!({
        "name": meta.name.clone().unwrap_or_default(),
        "spec": {
            "policy_name": policy_name,
            "match_resources": match_resources,
            "param_ref": param_ref,
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
    fn tool_definitions_returns_four_tools() {
        let defs = tool_definitions();
        assert_eq!(defs.len(), 4);

        let names: Vec<&str> = defs
            .iter()
            .map(|d| d["name"].as_str().unwrap())
            .collect();

        let mut unique = names.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(names.len(), unique.len(), "tool names must be unique");

        assert!(names.contains(&"list_mutatingadmissionpolicies"));
        assert!(names.contains(&"get_mutatingadmissionpolicy"));
        assert!(names.contains(&"list_mutatingadmissionpolicybindings"));
        assert!(names.contains(&"get_mutatingadmissionpolicybinding"));
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
}
