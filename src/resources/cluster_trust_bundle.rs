use k8s_openapi::api::certificates::v1alpha1::ClusterTrustBundle;
use kube::api::ListParams;
use serde::Serialize;

use crate::client::K8sClient;

fn api(client: &K8sClient) -> kube::Api<ClusterTrustBundle> {
    kube::Api::all(client.inner().clone())
}

#[derive(Serialize, Debug)]
pub struct ClusterTrustBundleSummary {
    pub name: String,
    pub signer_name: Option<String>,
    pub created_at: Option<String>,
}

fn extract_summary(ctb: &ClusterTrustBundle) -> ClusterTrustBundleSummary {
    let meta = &ctb.metadata;
    ClusterTrustBundleSummary {
        name: meta.name.clone().unwrap_or_default(),
        signer_name: ctb.spec.signer_name.clone(),
        created_at: meta.creation_timestamp.as_ref().map(|t| t.0.to_string()),
    }
}

pub fn tool_definitions() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "name": "list_clustertrustbundles",
            "description": "List all ClusterTrustBundles. Returns name, signer_name, and created_at.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "required": [],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "get_clustertrustbundle",
            "description": "Get a ClusterTrustBundle by name. Returns name, signer_name, trust_bundle (PEM cert data), labels, and annotations.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "ClusterTrustBundle name" }
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
        "list_clustertrustbundles" => list_clustertrustbundles(client).await,
        "get_clustertrustbundle" => get_clustertrustbundle(client, args).await,
        _ => return None,
    };
    Some(result)
}

async fn list_clustertrustbundles(client: &K8sClient) -> Result<String, String> {
    let ctb_api = api(client);
    let list = ctb_api
        .list(&ListParams::default())
        .await
        .map_err(|e| e.to_string())?;

    let summaries: Vec<ClusterTrustBundleSummary> =
        list.iter().map(extract_summary).collect();

    serde_json::to_string_pretty(&summaries).map_err(|e| e.to_string())
}

async fn get_clustertrustbundle(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let name = args["name"].as_str().ok_or("name is required")?;
    let ctb_api = api(client);
    let ctb = ctb_api.get(name).await.map_err(|e| e.to_string())?;

    let meta = &ctb.metadata;

    let result = serde_json::json!({
        "name": meta.name.clone().unwrap_or_default(),
        "signer_name": ctb.spec.signer_name.clone(),
        "trust_bundle": ctb.spec.trust_bundle,
        "labels": meta.labels.clone().unwrap_or_default(),
        "annotations": meta.annotations.clone().unwrap_or_default(),
    });

    serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_definitions_returns_two_tools() {
        let defs = tool_definitions();
        assert_eq!(defs.len(), 2);

        let names: Vec<&str> = defs.iter().map(|d| d["name"].as_str().unwrap()).collect();

        let mut unique = names.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(names.len(), unique.len(), "tool names must be unique");

        assert!(names.contains(&"list_clustertrustbundles"));
        assert!(names.contains(&"get_clustertrustbundle"));
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
                "ClusterTrustBundle tools must not have a namespace parameter, but {} does",
                def["name"]
            );
        }
    }

    #[test]
    fn cluster_trust_bundle_summary_serialization() {
        let summary = ClusterTrustBundleSummary {
            name: "example.com:foo:abc".to_string(),
            signer_name: Some("example.com/foo".to_string()),
            created_at: Some("2024-01-01T00:00:00Z".to_string()),
        };

        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["name"], "example.com:foo:abc");
        assert_eq!(json["signer_name"], "example.com/foo");
        assert_eq!(json["created_at"], "2024-01-01T00:00:00Z");
    }

    #[test]
    fn cluster_trust_bundle_summary_serialization_no_signer() {
        let summary = ClusterTrustBundleSummary {
            name: "my-trust-bundle".to_string(),
            signer_name: None,
            created_at: None,
        };

        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["name"], "my-trust-bundle");
        assert!(json["signer_name"].is_null());
        assert!(json["created_at"].is_null());
    }
}
