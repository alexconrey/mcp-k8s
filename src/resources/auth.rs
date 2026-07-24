use k8s_openapi::api::authentication::v1::SelfSubjectReview;
use k8s_openapi::api::authorization::v1::{
    ResourceAttributes, SelfSubjectAccessReview, SelfSubjectAccessReviewSpec,
    SelfSubjectRulesReview, SelfSubjectRulesReviewSpec,
};
use kube::api::PostParams;

use crate::client::K8sClient;

pub fn tool_definitions() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "name": "can_i",
            "description": "Check if the current user can perform a specific action on a resource. Creates a SelfSubjectAccessReview. Returns allowed (bool), reason, and evaluation_error.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "verb": { "type": "string", "description": "Kubernetes API verb (e.g. get, list, create, update, delete, watch)" },
                    "resource": { "type": "string", "description": "Resource type (e.g. pods, deployments, services)" },
                    "namespace": { "type": "string", "description": "Namespace to check (omit for cluster-scoped)" },
                    "subresource": { "type": "string", "description": "Subresource (e.g. status, log)" }
                },
                "required": ["verb", "resource"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "whoami",
            "description": "Identify the current authenticated user. Creates a SelfSubjectReview. Returns username, uid, groups, and extra attributes.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "list_my_permissions",
            "description": "List what the current user can do in a namespace. Creates a SelfSubjectRulesReview. Returns resource_rules and non_resource_rules.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Namespace to evaluate rules for" }
                },
                "required": ["namespace"],
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
        "can_i" => can_i(client, args).await,
        "whoami" => whoami(client).await,
        "list_my_permissions" => list_my_permissions(client, args).await,
        _ => return None,
    };
    Some(result)
}

async fn can_i(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let verb = args["verb"].as_str().ok_or("verb is required")?.to_string();
    let resource = args["resource"]
        .as_str()
        .ok_or("resource is required")?
        .to_string();
    let namespace = args
        .get("namespace")
        .and_then(|v| v.as_str())
        .map(String::from);
    let subresource = args
        .get("subresource")
        .and_then(|v| v.as_str())
        .map(String::from);

    let review = SelfSubjectAccessReview {
        spec: SelfSubjectAccessReviewSpec {
            resource_attributes: Some(ResourceAttributes {
                verb: Some(verb),
                resource: Some(resource),
                namespace,
                subresource,
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    };

    let api: kube::Api<SelfSubjectAccessReview> = kube::Api::all(client.inner().clone());
    let result = api
        .create(&PostParams::default(), &review)
        .await
        .map_err(|e| e.to_string())?;

    let status = result.status.unwrap_or_default();
    let output = serde_json::json!({
        "allowed": status.allowed,
        "reason": status.reason,
        "evaluation_error": status.evaluation_error,
    });

    serde_json::to_string_pretty(&output).map_err(|e| e.to_string())
}

async fn whoami(client: &K8sClient) -> Result<String, String> {
    let review = SelfSubjectReview {
        ..Default::default()
    };

    let api: kube::Api<SelfSubjectReview> = kube::Api::all(client.inner().clone());
    let result = api
        .create(&PostParams::default(), &review)
        .await
        .map_err(|e| e.to_string())?;

    let user_info = result.status.and_then(|s| s.user_info).unwrap_or_default();

    let output = serde_json::json!({
        "username": user_info.username,
        "uid": user_info.uid,
        "groups": user_info.groups.unwrap_or_default(),
        "extra": user_info.extra.unwrap_or_default(),
    });

    serde_json::to_string_pretty(&output).map_err(|e| e.to_string())
}

async fn list_my_permissions(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let namespace = args["namespace"]
        .as_str()
        .ok_or("namespace is required")?
        .to_string();

    let review = SelfSubjectRulesReview {
        spec: SelfSubjectRulesReviewSpec {
            namespace: Some(namespace),
        },
        ..Default::default()
    };

    let api: kube::Api<SelfSubjectRulesReview> = kube::Api::all(client.inner().clone());
    let result = api
        .create(&PostParams::default(), &review)
        .await
        .map_err(|e| e.to_string())?;

    let status = result.status.unwrap_or_default();

    let resource_rules: Vec<serde_json::Value> = status
        .resource_rules
        .iter()
        .map(|r| {
            serde_json::json!({
                "verbs": r.verbs,
                "api_groups": r.api_groups,
                "resources": r.resources,
                "resource_names": r.resource_names,
            })
        })
        .collect();

    let non_resource_rules: Vec<serde_json::Value> = status
        .non_resource_rules
        .iter()
        .map(|r| {
            serde_json::json!({
                "verbs": r.verbs,
                "non_resource_urls": r.non_resource_urls,
            })
        })
        .collect();

    let output = serde_json::json!({
        "resource_rules": resource_rules,
        "non_resource_rules": non_resource_rules,
        "incomplete": status.incomplete,
        "evaluation_error": status.evaluation_error,
    });

    serde_json::to_string_pretty(&output).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_definitions_returns_three_unique_tools() {
        let defs = tool_definitions();
        assert_eq!(defs.len(), 3);

        let names: Vec<&str> = defs.iter().map(|d| d["name"].as_str().unwrap()).collect();

        let mut unique = names.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(names.len(), unique.len(), "tool names must be unique");

        assert!(names.contains(&"can_i"));
        assert!(names.contains(&"whoami"));
        assert!(names.contains(&"list_my_permissions"));
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
    fn can_i_requires_verb_and_resource() {
        let defs = tool_definitions();
        let can_i_def = defs.iter().find(|d| d["name"] == "can_i").unwrap();
        let required = can_i_def["inputSchema"]["required"].as_array().unwrap();
        assert!(required.contains(&serde_json::json!("verb")));
        assert!(required.contains(&serde_json::json!("resource")));
        assert_eq!(required.len(), 2);
    }

    #[test]
    fn whoami_has_no_required_params() {
        let defs = tool_definitions();
        let whoami_def = defs.iter().find(|d| d["name"] == "whoami").unwrap();
        let schema = &whoami_def["inputSchema"];
        assert!(
            schema.get("required").is_none()
                || schema["required"].as_array().map_or(true, |a| a.is_empty()),
            "whoami should have no required parameters"
        );
    }

    #[test]
    fn list_my_permissions_requires_namespace() {
        let defs = tool_definitions();
        let def = defs
            .iter()
            .find(|d| d["name"] == "list_my_permissions")
            .unwrap();
        let required = def["inputSchema"]["required"].as_array().unwrap();
        assert!(required.contains(&serde_json::json!("namespace")));
        assert_eq!(required.len(), 1);
    }

    #[test]
    fn can_i_has_optional_namespace_and_subresource() {
        let defs = tool_definitions();
        let can_i_def = defs.iter().find(|d| d["name"] == "can_i").unwrap();
        let props = can_i_def["inputSchema"]["properties"].as_object().unwrap();
        assert!(
            props.contains_key("namespace"),
            "should have namespace property"
        );
        assert!(
            props.contains_key("subresource"),
            "should have subresource property"
        );

        let required = can_i_def["inputSchema"]["required"].as_array().unwrap();
        assert!(
            !required.contains(&serde_json::json!("namespace")),
            "namespace should be optional"
        );
        assert!(
            !required.contains(&serde_json::json!("subresource")),
            "subresource should be optional"
        );
    }
}
