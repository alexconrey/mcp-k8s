use std::collections::BTreeMap;

use k8s_openapi::api::storage::v1beta1::VolumeAttributesClass;
use kube::api::ListParams;
use serde::Serialize;

use crate::client::K8sClient;

fn api(client: &K8sClient) -> kube::Api<VolumeAttributesClass> {
    kube::Api::all(client.inner().clone())
}

#[derive(Serialize, Debug)]
pub struct VolumeAttributesClassSummary {
    pub name: String,
    pub driver_name: String,
    pub parameters: Option<BTreeMap<String, String>>,
    pub created_at: Option<String>,
}

fn extract_summary(vac: &VolumeAttributesClass) -> VolumeAttributesClassSummary {
    let meta = &vac.metadata;

    VolumeAttributesClassSummary {
        name: meta.name.clone().unwrap_or_default(),
        driver_name: vac.driver_name.clone(),
        parameters: vac.parameters.clone(),
        created_at: meta.creation_timestamp.as_ref().map(|t| t.0.to_string()),
    }
}

pub fn tool_definitions() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "name": "list_volumeattributesclasses",
            "description": "List all VolumeAttributesClasses (storage.k8s.io/v1beta1). Returns name, driver_name, parameters, and created_at.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "required": [],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "get_volumeattributesclass",
            "description": "Get a VolumeAttributesClass by name. Returns name, driver_name, parameters, created_at, labels, and annotations.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "VolumeAttributesClass name" }
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
        "list_volumeattributesclasses" => list_volumeattributesclasses(client).await,
        "get_volumeattributesclass" => get_volumeattributesclass(client, args).await,
        _ => return None,
    };
    Some(result)
}

async fn list_volumeattributesclasses(client: &K8sClient) -> Result<String, String> {
    let vac_api = api(client);
    let list = vac_api
        .list(&ListParams::default())
        .await
        .map_err(|e| e.to_string())?;

    let summaries: Vec<VolumeAttributesClassSummary> =
        list.iter().map(|vac| extract_summary(vac)).collect();

    serde_json::to_string_pretty(&summaries).map_err(|e| e.to_string())
}

async fn get_volumeattributesclass(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let name = args["name"].as_str().ok_or("name is required")?;
    let vac_api = api(client);
    let vac = vac_api.get(name).await.map_err(|e| e.to_string())?;

    let summary = extract_summary(&vac);
    let meta = &vac.metadata;

    let result = serde_json::json!({
        "name": summary.name,
        "driver_name": summary.driver_name,
        "parameters": summary.parameters,
        "created_at": summary.created_at,
        "labels": meta.labels.clone().unwrap_or_default(),
        "annotations": meta.annotations.clone().unwrap_or_default(),
    });

    serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
    use std::collections::BTreeMap;

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

        assert!(names.contains(&"list_volumeattributesclasses"));
        assert!(names.contains(&"get_volumeattributesclass"));
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
    fn volume_attributes_class_summary_serialization() {
        let mut params = BTreeMap::new();
        params.insert("iops".to_string(), "3000".to_string());
        params.insert("throughput".to_string(), "125".to_string());

        let summary = VolumeAttributesClassSummary {
            name: "high-perf".to_string(),
            driver_name: "ebs.csi.aws.com".to_string(),
            parameters: Some(params),
            created_at: Some("2024-01-01T00:00:00Z".to_string()),
        };

        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["name"], "high-perf");
        assert_eq!(json["driver_name"], "ebs.csi.aws.com");
        assert_eq!(json["parameters"]["iops"], "3000");
        assert_eq!(json["parameters"]["throughput"], "125");
        assert_eq!(json["created_at"], "2024-01-01T00:00:00Z");
    }

    #[test]
    fn volume_attributes_class_summary_serialization_empty_fields() {
        let summary = VolumeAttributesClassSummary {
            name: "minimal".to_string(),
            driver_name: "csi.example.com".to_string(),
            parameters: None,
            created_at: None,
        };

        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["name"], "minimal");
        assert_eq!(json["driver_name"], "csi.example.com");
        assert!(json["parameters"].is_null());
        assert!(json["created_at"].is_null());
    }

    #[test]
    fn extract_summary_from_volume_attributes_class() {
        let mut params = BTreeMap::new();
        params.insert("iops".to_string(), "5000".to_string());

        let vac = VolumeAttributesClass {
            metadata: ObjectMeta {
                name: Some("fast-storage".to_string()),
                labels: Some(BTreeMap::from([(
                    "tier".to_string(),
                    "premium".to_string(),
                )])),
                ..Default::default()
            },
            driver_name: "ebs.csi.aws.com".to_string(),
            parameters: Some(params),
        };

        let summary = extract_summary(&vac);
        assert_eq!(summary.name, "fast-storage");
        assert_eq!(summary.driver_name, "ebs.csi.aws.com");
        assert!(summary.parameters.is_some());
        assert_eq!(
            summary.parameters.as_ref().unwrap().get("iops").unwrap(),
            "5000"
        );
        assert!(summary.created_at.is_none());
    }

    #[test]
    fn extract_summary_from_minimal_volume_attributes_class() {
        let vac = VolumeAttributesClass {
            metadata: ObjectMeta {
                name: Some("basic".to_string()),
                ..Default::default()
            },
            driver_name: "csi.example.com".to_string(),
            parameters: None,
        };

        let summary = extract_summary(&vac);
        assert_eq!(summary.name, "basic");
        assert_eq!(summary.driver_name, "csi.example.com");
        assert!(summary.parameters.is_none());
        assert!(summary.created_at.is_none());
    }
}
