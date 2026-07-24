use std::collections::BTreeMap;

use k8s_openapi::api::core::v1::ServiceAccount;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::api::{DeleteParams, ListParams, PostParams};
use serde::Serialize;

use crate::client::K8sClient;

fn api(client: &K8sClient, ns: &str) -> Result<kube::Api<ServiceAccount>, String> {
    if !client.is_namespace_allowed(ns) {
        return Err(format!("Namespace '{ns}' is not in the allowed list"));
    }
    Ok(kube::Api::namespaced(client.inner().clone(), ns))
}

#[derive(Serialize, Debug)]
pub struct ServiceAccountSummary {
    pub name: String,
    pub namespace: String,
    pub secrets_count: usize,
    pub image_pull_secrets: Vec<String>,
    pub created_at: Option<String>,
}

fn extract_summary(sa: &ServiceAccount) -> ServiceAccountSummary {
    let meta = &sa.metadata;
    let secrets_count = sa.secrets.as_ref().map(|s| s.len()).unwrap_or(0);
    let image_pull_secrets = sa
        .image_pull_secrets
        .as_ref()
        .map(|ips| ips.iter().map(|r| r.name.clone()).collect())
        .unwrap_or_default();

    ServiceAccountSummary {
        name: meta.name.clone().unwrap_or_default(),
        namespace: meta.namespace.clone().unwrap_or_default(),
        secrets_count,
        image_pull_secrets,
        created_at: meta.creation_timestamp.as_ref().map(|t| t.0.to_string()),
    }
}

pub fn tool_definitions() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "name": "list_serviceaccounts",
            "description": "List service accounts in a namespace. Returns name, namespace, secrets count, image pull secrets, and created_at.",
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
            "name": "get_serviceaccount",
            "description": "Get a service account by name. Returns name, namespace, secrets count, image pull secrets, created_at, labels, annotations (including IRSA/workload identity annotations like eks.amazonaws.com/role-arn), and automount_service_account_token.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "ServiceAccount name" }
                },
                "required": ["namespace", "name"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "create_serviceaccount",
            "description": "Create a service account in a namespace. Optionally set annotations (e.g. for IRSA role binding) and automount_service_account_token.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "ServiceAccount name" },
                    "annotations": {
                        "type": "object",
                        "description": "Annotations to set on the service account (e.g. {\"eks.amazonaws.com/role-arn\": \"arn:aws:iam::123456789012:role/my-role\"})",
                        "additionalProperties": { "type": "string" }
                    },
                    "automount_service_account_token": {
                        "type": "boolean",
                        "description": "Whether to automount the service account token into pods"
                    }
                },
                "required": ["namespace", "name"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "delete_serviceaccount",
            "description": "Delete a service account by name from a namespace.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "ServiceAccount name" }
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
        "list_serviceaccounts" => list_serviceaccounts(client, args).await,
        "get_serviceaccount" => get_serviceaccount(client, args).await,
        "create_serviceaccount" => create_serviceaccount(client, args).await,
        "delete_serviceaccount" => delete_serviceaccount(client, args).await,
        _ => return None,
    };
    Some(result)
}

async fn list_serviceaccounts(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let sa_api = api(client, ns)?;
    let label_selector = args["label_selector"].as_str();
    let mut lp = ListParams::default();
    if let Some(sel) = label_selector {
        lp = lp.labels(sel);
    }
    let list = sa_api.list(&lp).await.map_err(|e| e.to_string())?;

    let summaries: Vec<serde_json::Value> = list
        .iter()
        .map(|sa| {
            let s = extract_summary(sa);
            serde_json::json!({
                "name": s.name,
                "namespace": s.namespace,
                "secrets_count": s.secrets_count,
                "image_pull_secrets": s.image_pull_secrets,
                "created_at": s.created_at,
            })
        })
        .collect();

    serde_json::to_string_pretty(&summaries).map_err(|e| e.to_string())
}

async fn get_serviceaccount(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;
    let sa_api = api(client, ns)?;
    let sa = sa_api.get(name).await.map_err(|e| e.to_string())?;

    let summary = extract_summary(&sa);
    let meta = &sa.metadata;
    let result = serde_json::json!({
        "name": summary.name,
        "namespace": summary.namespace,
        "secrets_count": summary.secrets_count,
        "image_pull_secrets": summary.image_pull_secrets,
        "created_at": summary.created_at,
        "labels": meta.labels.clone().unwrap_or_default(),
        "annotations": meta.annotations.clone().unwrap_or_default(),
        "automount_service_account_token": sa.automount_service_account_token,
    });

    serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
}

async fn create_serviceaccount(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;

    let annotations: Option<BTreeMap<String, String>> = args.get("annotations").and_then(|v| {
        if v.is_null() {
            None
        } else {
            serde_json::from_value(v.clone()).ok()
        }
    });

    let automount = args
        .get("automount_service_account_token")
        .and_then(|v| v.as_bool());

    let mut labels = BTreeMap::new();
    labels.insert(
        "app.kubernetes.io/managed-by".to_string(),
        "mcp-k8s".to_string(),
    );

    let sa = ServiceAccount {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            namespace: Some(ns.to_string()),
            labels: Some(labels),
            annotations: annotations.map(|a| a.into_iter().collect()),
            ..Default::default()
        },
        automount_service_account_token: automount,
        ..Default::default()
    };

    let sa_api = api(client, ns)?;
    let created = sa_api
        .create(&PostParams::default(), &sa)
        .await
        .map_err(|e| e.to_string())?;

    let summary = extract_summary(&created);
    serde_json::to_string_pretty(&summary).map_err(|e| e.to_string())
}

async fn delete_serviceaccount(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;

    let sa_api = api(client, ns)?;
    sa_api
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
    fn tool_definitions_returns_four_unique_tools() {
        let defs = tool_definitions();
        assert_eq!(defs.len(), 4);

        let names: Vec<&str> = defs.iter().map(|d| d["name"].as_str().unwrap()).collect();

        let mut unique = names.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(names.len(), unique.len(), "tool names must be unique");

        assert!(names.contains(&"list_serviceaccounts"));
        assert!(names.contains(&"get_serviceaccount"));
        assert!(names.contains(&"create_serviceaccount"));
        assert!(names.contains(&"delete_serviceaccount"));
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
    fn serviceaccount_summary_serialization() {
        let summary = ServiceAccountSummary {
            name: "my-sa".to_string(),
            namespace: "default".to_string(),
            secrets_count: 2,
            image_pull_secrets: vec!["registry-secret".to_string()],
            created_at: Some("2024-01-01T00:00:00Z".to_string()),
        };

        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["name"], "my-sa");
        assert_eq!(json["namespace"], "default");
        assert_eq!(json["secrets_count"], 2);
        assert_eq!(json["image_pull_secrets"].as_array().unwrap().len(), 1);
        assert_eq!(json["image_pull_secrets"][0], "registry-secret");
        assert_eq!(json["created_at"], "2024-01-01T00:00:00Z");
    }

    #[test]
    fn serviceaccount_summary_serialization_empty_fields() {
        let summary = ServiceAccountSummary {
            name: "empty-sa".to_string(),
            namespace: "ns".to_string(),
            secrets_count: 0,
            image_pull_secrets: vec![],
            created_at: None,
        };

        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["name"], "empty-sa");
        assert_eq!(json["secrets_count"], 0);
        assert!(json["image_pull_secrets"].as_array().unwrap().is_empty());
        assert!(json["created_at"].is_null());
    }

    #[test]
    fn extract_summary_from_serviceaccount() {
        let sa = ServiceAccount {
            metadata: ObjectMeta {
                name: Some("test-sa".to_string()),
                namespace: Some("prod".to_string()),
                labels: Some(BTreeMap::from([("app".to_string(), "myapp".to_string())])),
                annotations: Some(BTreeMap::from([(
                    "eks.amazonaws.com/role-arn".to_string(),
                    "arn:aws:iam::123456789012:role/my-role".to_string(),
                )])),
                ..Default::default()
            },
            secrets: Some(vec![k8s_openapi::api::core::v1::ObjectReference {
                name: Some("test-sa-token-abc".to_string()),
                ..Default::default()
            }]),
            image_pull_secrets: Some(vec![
                k8s_openapi::api::core::v1::LocalObjectReference {
                    name: "ecr-creds".to_string(),
                },
                k8s_openapi::api::core::v1::LocalObjectReference {
                    name: "dockerhub-creds".to_string(),
                },
            ]),
            automount_service_account_token: Some(true),
        };

        let summary = extract_summary(&sa);
        assert_eq!(summary.name, "test-sa");
        assert_eq!(summary.namespace, "prod");
        assert_eq!(summary.secrets_count, 1);
        assert_eq!(summary.image_pull_secrets.len(), 2);
        assert!(summary
            .image_pull_secrets
            .contains(&"ecr-creds".to_string()));
        assert!(summary
            .image_pull_secrets
            .contains(&"dockerhub-creds".to_string()));
        assert!(summary.created_at.is_none());
    }

    #[test]
    fn extract_summary_from_empty_serviceaccount() {
        let sa = ServiceAccount {
            metadata: ObjectMeta {
                name: Some("empty-sa".to_string()),
                namespace: Some("default".to_string()),
                ..Default::default()
            },
            ..Default::default()
        };

        let summary = extract_summary(&sa);
        assert_eq!(summary.name, "empty-sa");
        assert_eq!(summary.namespace, "default");
        assert_eq!(summary.secrets_count, 0);
        assert!(summary.image_pull_secrets.is_empty());
        assert!(summary.created_at.is_none());
    }
}
