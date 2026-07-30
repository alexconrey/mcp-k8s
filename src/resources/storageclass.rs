use std::collections::BTreeMap;

use k8s_openapi::api::storage::v1::StorageClass;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::api::{DeleteParams, ListParams, PostParams};
use serde::Serialize;

use crate::client::K8sClient;

const DEFAULT_SC_ANNOTATION: &str = "storageclass.kubernetes.io/is-default-class";

fn api(client: &K8sClient) -> kube::Api<StorageClass> {
    kube::Api::all(client.inner().clone())
}

#[derive(Serialize, Debug)]
pub struct StorageClassSummary {
    pub name: String,
    pub provisioner: String,
    pub reclaim_policy: Option<String>,
    pub volume_binding_mode: Option<String>,
    pub allow_volume_expansion: bool,
    pub is_default: bool,
    pub created_at: Option<String>,
}

fn extract_summary(sc: &StorageClass) -> StorageClassSummary {
    let meta = &sc.metadata;

    let is_default = meta
        .annotations
        .as_ref()
        .and_then(|a| a.get(DEFAULT_SC_ANNOTATION))
        .map(|v| v == "true")
        .unwrap_or(false);

    StorageClassSummary {
        name: meta.name.clone().unwrap_or_default(),
        provisioner: sc.provisioner.clone(),
        reclaim_policy: sc.reclaim_policy.clone(),
        volume_binding_mode: sc.volume_binding_mode.clone(),
        allow_volume_expansion: sc.allow_volume_expansion.unwrap_or(false),
        is_default,
        created_at: meta.creation_timestamp.as_ref().map(|t| t.0.to_string()),
    }
}

pub fn tool_definitions() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "name": "list_storageclasses",
            "description": "List all StorageClasses. Returns name, provisioner, reclaim_policy, volume_binding_mode, allow_volume_expansion, is_default, and created_at.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "required": [],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "get_storageclass",
            "description": "Get a StorageClass by name. Returns detailed info including parameters, mount_options, labels, and annotations.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "StorageClass name" }
                },
                "required": ["name"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "create_storageclass",
            "description": "Create a StorageClass with the given provisioner and optional settings.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "StorageClass name" },
                    "provisioner": { "type": "string", "description": "Provisioner name (e.g. kubernetes.io/aws-ebs, ebs.csi.aws.com)" },
                    "reclaim_policy": { "type": "string", "description": "Reclaim policy: Delete or Retain (default: Delete)" },
                    "volume_binding_mode": { "type": "string", "description": "Volume binding mode: Immediate or WaitForFirstConsumer" },
                    "parameters": {
                        "type": "object",
                        "description": "Provisioner-specific parameters as key-value string pairs",
                        "additionalProperties": { "type": "string" }
                    }
                },
                "required": ["name", "provisioner"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "update_storageclass",
            "description": "Update an existing StorageClass. Fetches the current resource, applies changes, and replaces it.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "StorageClass name" },
                    "provisioner": { "type": "string", "description": "Provisioner name (e.g. kubernetes.io/aws-ebs, ebs.csi.aws.com)" },
                    "reclaim_policy": { "type": "string", "description": "Reclaim policy: Delete or Retain" },
                    "volume_binding_mode": { "type": "string", "description": "Volume binding mode: Immediate or WaitForFirstConsumer" },
                    "allow_volume_expansion": { "type": "boolean", "description": "Whether volumes created with this StorageClass can be expanded" },
                    "mount_options": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Mount options for volumes created with this StorageClass"
                    },
                    "parameters": {
                        "type": "object",
                        "description": "Provisioner-specific parameters as key-value string pairs",
                        "additionalProperties": { "type": "string" }
                    },
                    "is_default": { "type": "boolean", "description": "Set as the default StorageClass in the cluster" }
                },
                "required": ["name"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "delete_storageclass",
            "description": "Delete a StorageClass by name.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "StorageClass name" }
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
        "list_storageclasses" => list_storageclasses(client).await,
        "get_storageclass" => get_storageclass(client, args).await,
        "create_storageclass" => create_storageclass(client, args).await,
        "update_storageclass" => update_storageclass(client, args).await,
        "delete_storageclass" => delete_storageclass(client, args).await,
        _ => return None,
    };
    Some(result)
}

async fn list_storageclasses(client: &K8sClient) -> Result<String, String> {
    let sc_api = api(client);
    let list = sc_api
        .list(&ListParams::default())
        .await
        .map_err(|e| e.to_string())?;

    let summaries: Vec<StorageClassSummary> = list.iter().map(extract_summary).collect();

    serde_json::to_string_pretty(&summaries).map_err(|e| e.to_string())
}

async fn get_storageclass(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let name = args["name"].as_str().ok_or("name is required")?;
    let sc_api = api(client);
    let sc = sc_api.get(name).await.map_err(|e| e.to_string())?;

    let summary = extract_summary(&sc);
    let meta = &sc.metadata;

    let result = serde_json::json!({
        "name": summary.name,
        "provisioner": summary.provisioner,
        "reclaim_policy": summary.reclaim_policy,
        "volume_binding_mode": summary.volume_binding_mode,
        "allow_volume_expansion": summary.allow_volume_expansion,
        "is_default": summary.is_default,
        "created_at": summary.created_at,
        "parameters": sc.parameters.clone().unwrap_or_default(),
        "mount_options": sc.mount_options.clone().unwrap_or_default(),
        "labels": meta.labels.clone().unwrap_or_default(),
        "annotations": meta.annotations.clone().unwrap_or_default(),
    });

    serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
}

async fn create_storageclass(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let name = args["name"].as_str().ok_or("name is required")?;
    let provisioner = args["provisioner"]
        .as_str()
        .ok_or("provisioner is required")?;

    let reclaim_policy = args
        .get("reclaim_policy")
        .and_then(|v| v.as_str())
        .map(String::from);

    let volume_binding_mode = args
        .get("volume_binding_mode")
        .and_then(|v| v.as_str())
        .map(String::from);

    let parameters: Option<BTreeMap<String, String>> = args
        .get("parameters")
        .and_then(|v| serde_json::from_value(v.clone()).ok());

    let mut labels = BTreeMap::new();
    labels.insert(
        "app.kubernetes.io/managed-by".to_string(),
        "mcp-k8s".to_string(),
    );

    let sc = StorageClass {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            labels: Some(labels),
            ..Default::default()
        },
        provisioner: provisioner.to_string(),
        reclaim_policy,
        volume_binding_mode,
        parameters,
        ..Default::default()
    };

    let sc_api = api(client);
    let created = sc_api
        .create(&PostParams::default(), &sc)
        .await
        .map_err(|e| e.to_string())?;

    let summary = extract_summary(&created);
    serde_json::to_string_pretty(&summary).map_err(|e| e.to_string())
}

async fn update_storageclass(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let name = args["name"].as_str().ok_or("name is required")?;

    let sc_api = api(client);
    let mut sc = sc_api.get(name).await.map_err(|e| e.to_string())?;

    // Update provisioner if provided
    if let Some(provisioner) = args.get("provisioner").and_then(|v| v.as_str()) {
        sc.provisioner = provisioner.to_string();
    }

    // Update reclaim_policy if provided
    if let Some(reclaim_policy) = args.get("reclaim_policy").and_then(|v| v.as_str()) {
        sc.reclaim_policy = Some(reclaim_policy.to_string());
    }

    // Update volume_binding_mode if provided
    if let Some(volume_binding_mode) = args.get("volume_binding_mode").and_then(|v| v.as_str()) {
        sc.volume_binding_mode = Some(volume_binding_mode.to_string());
    }

    // Update allow_volume_expansion if provided
    if let Some(allow_volume_expansion) =
        args.get("allow_volume_expansion").and_then(|v| v.as_bool())
    {
        sc.allow_volume_expansion = Some(allow_volume_expansion);
    }

    // Update mount_options if provided
    if let Some(mount_options) = args.get("mount_options") {
        let opts: Vec<String> = serde_json::from_value(mount_options.clone())
            .map_err(|e| format!("invalid mount_options: {}", e))?;
        sc.mount_options = Some(opts);
    }

    // Update parameters if provided
    if let Some(parameters) = args.get("parameters") {
        let params: BTreeMap<String, String> = serde_json::from_value(parameters.clone())
            .map_err(|e| format!("invalid parameters: {}", e))?;
        sc.parameters = Some(params);
    }

    // Update is_default annotation if provided
    if let Some(is_default) = args.get("is_default").and_then(|v| v.as_bool()) {
        let annotations = sc.metadata.annotations.get_or_insert_with(BTreeMap::new);
        annotations.insert(DEFAULT_SC_ANNOTATION.to_string(), is_default.to_string());
    }

    let updated = sc_api
        .replace(name, &PostParams::default(), &sc)
        .await
        .map_err(|e| e.to_string())?;

    let summary = extract_summary(&updated);
    serde_json::to_string_pretty(&summary).map_err(|e| e.to_string())
}

async fn delete_storageclass(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let name = args["name"].as_str().ok_or("name is required")?;

    let sc_api = api(client);
    sc_api
        .delete(name, &DeleteParams::default())
        .await
        .map_err(|e| e.to_string())?;

    let result = serde_json::json!({
        "deleted": name,
    });
    serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
    use std::collections::BTreeMap;

    #[test]
    fn tool_definitions_returns_five_tools() {
        let defs = tool_definitions();
        assert_eq!(defs.len(), 5);

        let names: Vec<&str> = defs.iter().map(|d| d["name"].as_str().unwrap()).collect();

        let mut unique = names.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(names.len(), unique.len(), "tool names must be unique");

        assert!(names.contains(&"list_storageclasses"));
        assert!(names.contains(&"get_storageclass"));
        assert!(names.contains(&"create_storageclass"));
        assert!(names.contains(&"update_storageclass"));
        assert!(names.contains(&"delete_storageclass"));
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
    fn storageclass_summary_serialization() {
        let summary = StorageClassSummary {
            name: "gp3".to_string(),
            provisioner: "ebs.csi.aws.com".to_string(),
            reclaim_policy: Some("Delete".to_string()),
            volume_binding_mode: Some("WaitForFirstConsumer".to_string()),
            allow_volume_expansion: true,
            is_default: true,
            created_at: Some("2024-01-01T00:00:00Z".to_string()),
        };

        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["name"], "gp3");
        assert_eq!(json["provisioner"], "ebs.csi.aws.com");
        assert_eq!(json["reclaim_policy"], "Delete");
        assert_eq!(json["volume_binding_mode"], "WaitForFirstConsumer");
        assert_eq!(json["allow_volume_expansion"], true);
        assert_eq!(json["is_default"], true);
        assert_eq!(json["created_at"], "2024-01-01T00:00:00Z");
    }

    #[test]
    fn storageclass_summary_serialization_empty_fields() {
        let summary = StorageClassSummary {
            name: "minimal".to_string(),
            provisioner: "kubernetes.io/no-provisioner".to_string(),
            reclaim_policy: None,
            volume_binding_mode: None,
            allow_volume_expansion: false,
            is_default: false,
            created_at: None,
        };

        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["name"], "minimal");
        assert_eq!(json["provisioner"], "kubernetes.io/no-provisioner");
        assert!(json["reclaim_policy"].is_null());
        assert!(json["volume_binding_mode"].is_null());
        assert_eq!(json["allow_volume_expansion"], false);
        assert_eq!(json["is_default"], false);
        assert!(json["created_at"].is_null());
    }

    #[test]
    fn extract_summary_from_storageclass() {
        let mut annotations = BTreeMap::new();
        annotations.insert(DEFAULT_SC_ANNOTATION.to_string(), "true".to_string());

        let sc = StorageClass {
            metadata: ObjectMeta {
                name: Some("gp3-encrypted".to_string()),
                annotations: Some(annotations),
                labels: Some(BTreeMap::from([(
                    "tier".to_string(),
                    "storage".to_string(),
                )])),
                ..Default::default()
            },
            provisioner: "ebs.csi.aws.com".to_string(),
            reclaim_policy: Some("Retain".to_string()),
            volume_binding_mode: Some("WaitForFirstConsumer".to_string()),
            allow_volume_expansion: Some(true),
            parameters: Some(BTreeMap::from([
                ("type".to_string(), "gp3".to_string()),
                ("encrypted".to_string(), "true".to_string()),
            ])),
            mount_options: Some(vec!["debug".to_string()]),
            ..Default::default()
        };

        let summary = extract_summary(&sc);
        assert_eq!(summary.name, "gp3-encrypted");
        assert_eq!(summary.provisioner, "ebs.csi.aws.com");
        assert_eq!(summary.reclaim_policy.as_deref(), Some("Retain"));
        assert_eq!(
            summary.volume_binding_mode.as_deref(),
            Some("WaitForFirstConsumer")
        );
        assert!(summary.allow_volume_expansion);
        assert!(summary.is_default);
        assert!(summary.created_at.is_none());
    }

    #[test]
    fn extract_summary_from_minimal_storageclass() {
        let sc = StorageClass {
            metadata: ObjectMeta {
                name: Some("local-path".to_string()),
                ..Default::default()
            },
            provisioner: "rancher.io/local-path".to_string(),
            ..Default::default()
        };

        let summary = extract_summary(&sc);
        assert_eq!(summary.name, "local-path");
        assert_eq!(summary.provisioner, "rancher.io/local-path");
        assert!(summary.reclaim_policy.is_none());
        assert!(summary.volume_binding_mode.is_none());
        assert!(!summary.allow_volume_expansion);
        assert!(!summary.is_default);
        assert!(summary.created_at.is_none());
    }

    #[test]
    fn extract_summary_default_annotation_false() {
        let mut annotations = BTreeMap::new();
        annotations.insert(DEFAULT_SC_ANNOTATION.to_string(), "false".to_string());

        let sc = StorageClass {
            metadata: ObjectMeta {
                name: Some("not-default".to_string()),
                annotations: Some(annotations),
                ..Default::default()
            },
            provisioner: "ebs.csi.aws.com".to_string(),
            ..Default::default()
        };

        let summary = extract_summary(&sc);
        assert!(!summary.is_default);
    }
}
