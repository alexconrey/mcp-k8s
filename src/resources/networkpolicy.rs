use std::collections::BTreeMap;

use k8s_openapi::api::networking::v1::{
    NetworkPolicy, NetworkPolicyEgressRule, NetworkPolicyIngressRule, NetworkPolicySpec,
};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{LabelSelector, ObjectMeta};
use kube::api::{DeleteParams, ListParams, Patch, PatchParams, PostParams};
use serde::Serialize;

use crate::client::K8sClient;

fn api(client: &K8sClient, ns: &str) -> Result<kube::Api<NetworkPolicy>, String> {
    if !client.is_namespace_allowed(ns) {
        return Err(format!("Namespace '{ns}' is not in the allowed list"));
    }
    Ok(kube::Api::namespaced(client.inner().clone(), ns))
}

#[derive(Serialize, Debug)]
pub struct NetworkPolicySummary {
    pub name: String,
    pub namespace: String,
    pub pod_selector: serde_json::Value,
    pub policy_types: Vec<String>,
    pub created_at: Option<String>,
}

fn extract_summary(np: &NetworkPolicy) -> NetworkPolicySummary {
    let meta = &np.metadata;
    let spec = np.spec.as_ref();

    let pod_selector = spec
        .and_then(|s| s.pod_selector.as_ref())
        .map(|ps| serde_json::to_value(ps).unwrap_or_default())
        .unwrap_or_else(|| serde_json::json!({}));

    let policy_types = spec
        .and_then(|s| s.policy_types.clone())
        .unwrap_or_default();

    NetworkPolicySummary {
        name: meta.name.clone().unwrap_or_default(),
        namespace: meta.namespace.clone().unwrap_or_default(),
        pod_selector,
        policy_types,
        created_at: meta.creation_timestamp.as_ref().map(|t| t.0.to_string()),
    }
}

pub fn tool_definitions() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "name": "list_networkpolicies",
            "description": "List network policies in a namespace. Returns name, namespace, pod_selector, policy_types (Ingress/Egress), and created_at.",
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
            "name": "get_networkpolicy",
            "description": "Get detailed info for a single network policy including ingress rules, egress rules, labels, and annotations.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "NetworkPolicy name" }
                },
                "required": ["namespace", "name"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "create_networkpolicy",
            "description": "Create a network policy in a namespace. Accepts pod_selector, policy_types, and optional ingress/egress rules as JSON.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "NetworkPolicy name" },
                    "pod_selector": {
                        "type": "object",
                        "description": "Label selector for pods this policy applies to (e.g. {\"matchLabels\": {\"app\": \"web\"}})"
                    },
                    "policy_types": {
                        "type": "array",
                        "description": "List of policy types: Ingress, Egress, or both",
                        "items": { "type": "string", "enum": ["Ingress", "Egress"] }
                    },
                    "ingress": {
                        "type": "array",
                        "description": "Ingress rules as JSON array of NetworkPolicyIngressRule objects"
                    },
                    "egress": {
                        "type": "array",
                        "description": "Egress rules as JSON array of NetworkPolicyEgressRule objects"
                    }
                },
                "required": ["namespace", "name", "pod_selector", "policy_types"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "update_networkpolicy",
            "description": "Update (merge patch) a network policy. Accepts optional pod_selector, policy_types, ingress, and egress fields.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "NetworkPolicy name" },
                    "pod_selector": {
                        "type": "object",
                        "description": "Label selector for pods this policy applies to"
                    },
                    "policy_types": {
                        "type": "array",
                        "description": "List of policy types: Ingress, Egress, or both",
                        "items": { "type": "string", "enum": ["Ingress", "Egress"] }
                    },
                    "ingress": {
                        "type": "array",
                        "description": "Ingress rules as JSON array"
                    },
                    "egress": {
                        "type": "array",
                        "description": "Egress rules as JSON array"
                    }
                },
                "required": ["namespace", "name"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "delete_networkpolicy",
            "description": "Delete a network policy by name from a namespace.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "NetworkPolicy name" }
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
        "list_networkpolicies" => list_networkpolicies(client, args).await,
        "get_networkpolicy" => get_networkpolicy(client, args).await,
        "create_networkpolicy" => create_networkpolicy(client, args).await,
        "update_networkpolicy" => update_networkpolicy(client, args).await,
        "delete_networkpolicy" => delete_networkpolicy(client, args).await,
        _ => return None,
    };
    Some(result)
}

async fn list_networkpolicies(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let np_api = api(client, ns)?;
    let label_selector = args["label_selector"].as_str();
    let mut lp = ListParams::default();
    if let Some(sel) = label_selector {
        lp = lp.labels(sel);
    }
    let list = np_api
        .list(&lp)
        .await
        .map_err(|e| e.to_string())?;

    let summaries: Vec<serde_json::Value> = list
        .iter()
        .map(|np| {
            let s = extract_summary(np);
            serde_json::to_value(s).unwrap_or_default()
        })
        .collect();

    serde_json::to_string_pretty(&summaries).map_err(|e| e.to_string())
}

async fn get_networkpolicy(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;

    let np_api = api(client, ns)?;
    let np = np_api.get(name).await.map_err(|e| e.to_string())?;

    let summary = extract_summary(&np);
    let spec = np.spec.as_ref();
    let meta = &np.metadata;

    let ingress_rules = spec
        .and_then(|s| s.ingress.as_ref())
        .map(|rules| serde_json::to_value(rules).unwrap_or_default())
        .unwrap_or_else(|| serde_json::json!([]));

    let egress_rules = spec
        .and_then(|s| s.egress.as_ref())
        .map(|rules| serde_json::to_value(rules).unwrap_or_default())
        .unwrap_or_else(|| serde_json::json!([]));

    let result = serde_json::json!({
        "name": summary.name,
        "namespace": summary.namespace,
        "pod_selector": summary.pod_selector,
        "policy_types": summary.policy_types,
        "created_at": summary.created_at,
        "ingress_rules": ingress_rules,
        "egress_rules": egress_rules,
        "labels": meta.labels.clone().unwrap_or_default(),
        "annotations": meta.annotations.clone().unwrap_or_default(),
    });

    serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
}

async fn create_networkpolicy(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;

    let pod_selector: LabelSelector = serde_json::from_value(
        args.get("pod_selector")
            .ok_or("pod_selector is required")?
            .clone(),
    )
    .map_err(|e| format!("invalid pod_selector: {e}"))?;

    let policy_types: Vec<String> = serde_json::from_value(
        args.get("policy_types")
            .ok_or("policy_types is required")?
            .clone(),
    )
    .map_err(|e| format!("invalid policy_types: {e}"))?;

    let ingress: Option<Vec<NetworkPolicyIngressRule>> = args
        .get("ingress")
        .map(|v| serde_json::from_value(v.clone()))
        .transpose()
        .map_err(|e| format!("invalid ingress rules: {e}"))?;

    let egress: Option<Vec<NetworkPolicyEgressRule>> = args
        .get("egress")
        .map(|v| serde_json::from_value(v.clone()))
        .transpose()
        .map_err(|e| format!("invalid egress rules: {e}"))?;

    let mut labels = BTreeMap::new();
    labels.insert(
        "app.kubernetes.io/managed-by".to_string(),
        "mcp-k8s".to_string(),
    );

    let np = NetworkPolicy {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            namespace: Some(ns.to_string()),
            labels: Some(labels),
            ..Default::default()
        },
        spec: Some(NetworkPolicySpec {
            pod_selector: Some(pod_selector),
            policy_types: Some(policy_types),
            ingress,
            egress,
        }),
    };

    let np_api = api(client, ns)?;
    let created = np_api
        .create(&PostParams::default(), &np)
        .await
        .map_err(|e| e.to_string())?;

    let summary = extract_summary(&created);
    serde_json::to_string_pretty(&summary).map_err(|e| e.to_string())
}

async fn update_networkpolicy(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;

    let mut spec_patch = serde_json::Map::new();

    if let Some(pod_selector) = args.get("pod_selector") {
        spec_patch.insert("podSelector".to_string(), pod_selector.clone());
    }

    if let Some(policy_types) = args.get("policy_types") {
        spec_patch.insert("policyTypes".to_string(), policy_types.clone());
    }

    if let Some(ingress) = args.get("ingress") {
        spec_patch.insert("ingress".to_string(), ingress.clone());
    }

    if let Some(egress) = args.get("egress") {
        spec_patch.insert("egress".to_string(), egress.clone());
    }

    if spec_patch.is_empty() {
        return Err(
            "At least one of pod_selector, policy_types, ingress, or egress must be provided"
                .to_string(),
        );
    }

    let patch = serde_json::json!({
        "spec": spec_patch,
    });

    let np_api = api(client, ns)?;
    let patched = np_api
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

async fn delete_networkpolicy(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;

    let np_api = api(client, ns)?;
    np_api
        .delete(name, &DeleteParams::default())
        .await
        .map_err(|e| e.to_string())?;

    let result = serde_json::json!({
        "deleted": true,
        "name": name,
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

        assert!(names.contains(&"list_networkpolicies"));
        assert!(names.contains(&"get_networkpolicy"));
        assert!(names.contains(&"create_networkpolicy"));
        assert!(names.contains(&"update_networkpolicy"));
        assert!(names.contains(&"delete_networkpolicy"));
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
    fn networkpolicy_summary_serialization() {
        let summary = NetworkPolicySummary {
            name: "deny-all".to_string(),
            namespace: "prod".to_string(),
            pod_selector: serde_json::json!({"matchLabels": {"app": "web"}}),
            policy_types: vec!["Ingress".to_string(), "Egress".to_string()],
            created_at: Some("2024-01-01T00:00:00Z".to_string()),
        };

        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["name"], "deny-all");
        assert_eq!(json["namespace"], "prod");
        assert_eq!(json["pod_selector"]["matchLabels"]["app"], "web");
        assert_eq!(json["policy_types"].as_array().unwrap().len(), 2);
        assert_eq!(json["policy_types"][0], "Ingress");
        assert_eq!(json["policy_types"][1], "Egress");
        assert_eq!(json["created_at"], "2024-01-01T00:00:00Z");
    }

    #[test]
    fn networkpolicy_summary_serialization_empty_fields() {
        let summary = NetworkPolicySummary {
            name: "empty".to_string(),
            namespace: "ns".to_string(),
            pod_selector: serde_json::json!({}),
            policy_types: vec![],
            created_at: None,
        };

        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["name"], "empty");
        assert_eq!(json["namespace"], "ns");
        assert!(json["pod_selector"].as_object().unwrap().is_empty());
        assert!(json["policy_types"].as_array().unwrap().is_empty());
        assert!(json["created_at"].is_null());
    }

    fn make_test_networkpolicy() -> NetworkPolicy {
        let mut labels = BTreeMap::new();
        labels.insert("env".to_string(), "production".to_string());
        labels.insert(
            "app.kubernetes.io/managed-by".to_string(),
            "mcp-k8s".to_string(),
        );

        let mut annotations = BTreeMap::new();
        annotations.insert(
            "description".to_string(),
            "Allow ingress from frontend".to_string(),
        );

        NetworkPolicy {
            metadata: ObjectMeta {
                name: Some("allow-frontend".to_string()),
                namespace: Some("prod".to_string()),
                labels: Some(labels),
                annotations: Some(annotations),
                ..Default::default()
            },
            spec: Some(NetworkPolicySpec {
                pod_selector: Some(LabelSelector {
                    match_labels: Some(BTreeMap::from([(
                        "app".to_string(),
                        "backend".to_string(),
                    )])),
                    ..Default::default()
                }),
                policy_types: Some(vec![
                    "Ingress".to_string(),
                    "Egress".to_string(),
                ]),
                ingress: Some(vec![NetworkPolicyIngressRule {
                    from: Some(vec![
                        k8s_openapi::api::networking::v1::NetworkPolicyPeer {
                            pod_selector: Some(LabelSelector {
                                match_labels: Some(BTreeMap::from([(
                                    "app".to_string(),
                                    "frontend".to_string(),
                                )])),
                                ..Default::default()
                            }),
                            ..Default::default()
                        },
                    ]),
                    ..Default::default()
                }]),
                egress: Some(vec![NetworkPolicyEgressRule {
                    to: Some(vec![
                        k8s_openapi::api::networking::v1::NetworkPolicyPeer {
                            pod_selector: Some(LabelSelector {
                                match_labels: Some(BTreeMap::from([(
                                    "app".to_string(),
                                    "database".to_string(),
                                )])),
                                ..Default::default()
                            }),
                            ..Default::default()
                        },
                    ]),
                    ..Default::default()
                }]),
            }),
        }
    }

    #[test]
    fn extract_summary_from_networkpolicy() {
        let np = make_test_networkpolicy();
        let summary = extract_summary(&np);

        assert_eq!(summary.name, "allow-frontend");
        assert_eq!(summary.namespace, "prod");
        assert_eq!(
            summary.pod_selector,
            serde_json::json!({"matchLabels": {"app": "backend"}})
        );
        assert_eq!(summary.policy_types, vec!["Ingress", "Egress"]);
        assert!(summary.created_at.is_none()); // no timestamp set
    }

    #[test]
    fn extract_summary_from_minimal_networkpolicy() {
        let np = NetworkPolicy {
            metadata: ObjectMeta {
                name: Some("minimal".to_string()),
                namespace: Some("default".to_string()),
                ..Default::default()
            },
            ..Default::default()
        };

        let summary = extract_summary(&np);
        assert_eq!(summary.name, "minimal");
        assert_eq!(summary.namespace, "default");
        assert_eq!(summary.pod_selector, serde_json::json!({}));
        assert!(summary.policy_types.is_empty());
        assert!(summary.created_at.is_none());
    }

    #[test]
    fn extract_summary_ingress_only_policy() {
        let np = NetworkPolicy {
            metadata: ObjectMeta {
                name: Some("ingress-only".to_string()),
                namespace: Some("web".to_string()),
                ..Default::default()
            },
            spec: Some(NetworkPolicySpec {
                pod_selector: Some(LabelSelector {
                    match_labels: Some(BTreeMap::from([(
                        "role".to_string(),
                        "api".to_string(),
                    )])),
                    ..Default::default()
                }),
                policy_types: Some(vec!["Ingress".to_string()]),
                ingress: None,
                egress: None,
            }),
        };

        let summary = extract_summary(&np);
        assert_eq!(summary.name, "ingress-only");
        assert_eq!(summary.namespace, "web");
        assert_eq!(summary.policy_types, vec!["Ingress"]);
        assert_eq!(
            summary.pod_selector,
            serde_json::json!({"matchLabels": {"role": "api"}})
        );
    }
}
