use std::collections::BTreeMap;

use k8s_openapi::api::core::v1::Secret;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use k8s_openapi::ByteString;
use kube::api::{DeleteParams, ListParams, Patch, PatchParams, PostParams};
use serde::Serialize;

use crate::client::K8sClient;

fn api(client: &K8sClient, ns: &str) -> Result<kube::Api<Secret>, String> {
    if !client.is_namespace_allowed(ns) {
        return Err(format!("Namespace '{ns}' is not in the allowed list"));
    }
    Ok(kube::Api::namespaced(client.inner().clone(), ns))
}

#[derive(Serialize, Debug, PartialEq)]
pub struct SecretSummary {
    pub name: String,
    pub namespace: String,
    pub secret_type: String,
    pub data_keys: Vec<String>,
    pub created_at: Option<String>,
}

fn secret_type(secret: &Secret) -> String {
    secret.type_.clone().unwrap_or_else(|| "Opaque".to_string())
}

fn data_keys(secret: &Secret) -> Vec<String> {
    let mut keys: Vec<String> = secret
        .data
        .as_ref()
        .map(|d| d.keys().cloned().collect())
        .unwrap_or_default();
    // Also include keys from string_data if present (they may not be in data yet)
    if let Some(sd) = &secret.string_data {
        for k in sd.keys() {
            if !keys.contains(k) {
                keys.push(k.clone());
            }
        }
    }
    keys.sort();
    keys
}

fn secret_summary(secret: &Secret) -> SecretSummary {
    let meta = &secret.metadata;
    SecretSummary {
        name: meta.name.clone().unwrap_or_default(),
        namespace: meta.namespace.clone().unwrap_or_default(),
        secret_type: secret_type(secret),
        data_keys: data_keys(secret),
        created_at: meta.creation_timestamp.as_ref().map(|t| t.0.to_string()),
    }
}

pub fn tool_definitions() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "name": "list_secrets",
            "description": "List secrets in a namespace. Returns name, namespace, type, data key count, and created_at. Secret values are never returned.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "label_selector": { "type": "string", "description": "Label selector to filter results (e.g. app=nginx)" },
                    "field_selector": { "type": "string", "description": "Field selector to filter results (e.g. metadata.name=foo, type=kubernetes.io/tls)" }
                },
                "required": ["namespace"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "get_secret",
            "description": "Get a secret by name. Returns metadata and data keys only by default. Pass decode: true to include base64-decoded values.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "Secret name" },
                    "decode": { "type": "boolean", "description": "If true, include decoded secret values in the response. Defaults to false." }
                },
                "required": ["namespace", "name"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "create_secret",
            "description": "Create a Kubernetes Secret. Accepts string_data as key-value pairs.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "Secret name" },
                    "type": { "type": "string", "description": "Secret type (default: Opaque)" },
                    "string_data": { "type": "object", "description": "Key-value pairs of secret data", "additionalProperties": { "type": "string" } }
                },
                "required": ["namespace", "name", "string_data"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "update_secret",
            "description": "Update (merge-patch) a secret's data. Provided string_data keys are merged into the existing secret.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "Secret name" },
                    "string_data": { "type": "object", "description": "Key-value pairs to merge into the secret", "additionalProperties": { "type": "string" } }
                },
                "required": ["namespace", "name", "string_data"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "delete_secret",
            "description": "Delete a Kubernetes Secret by name.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "Secret name" }
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
        "list_secrets" => list_secrets(client, args).await,
        "get_secret" => get_secret(client, args).await,
        "create_secret" => create_secret(client, args).await,
        "update_secret" => update_secret(client, args).await,
        "delete_secret" => delete_secret(client, args).await,
        _ => return None,
    };
    Some(result)
}

async fn list_secrets(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let secrets_api = api(client, ns)?;
    let label_selector = args["label_selector"].as_str();
    let field_selector = args["field_selector"].as_str();
    let mut lp = ListParams::default();
    if let Some(sel) = label_selector {
        lp = lp.labels(sel);
    }
    if let Some(sel) = field_selector {
        lp = lp.fields(sel);
    }
    let list = secrets_api.list(&lp).await.map_err(|e| e.to_string())?;

    let summaries: Vec<serde_json::Value> = list
        .iter()
        .map(|s| {
            let summary = secret_summary(s);
            serde_json::json!({
                "name": summary.name,
                "namespace": summary.namespace,
                "type": summary.secret_type,
                "data_key_count": summary.data_keys.len(),
                "created_at": summary.created_at,
            })
        })
        .collect();

    serde_json::to_string_pretty(&summaries).map_err(|e| e.to_string())
}

async fn get_secret(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;
    let decode_requested = args["decode"].as_bool().unwrap_or(false);

    // Honour the --disable-secret-decode flag: if decode is disabled server-wide,
    // ignore the caller's decode=true request.
    let decode = decode_requested && client.permissions().secret_decode_enabled;

    let secrets_api = api(client, ns)?;
    let secret = secrets_api.get(name).await.map_err(|e| e.to_string())?;

    let meta = &secret.metadata;
    let keys = data_keys(&secret);

    let mut result = serde_json::json!({
        "name": meta.name.clone().unwrap_or_default(),
        "namespace": meta.namespace.clone().unwrap_or_default(),
        "type": secret_type(&secret),
        "labels": meta.labels.clone().unwrap_or_default(),
        "annotations": meta.annotations.clone().unwrap_or_default(),
        "data_keys": keys,
        "created_at": meta.creation_timestamp.as_ref().map(|t| t.0.to_string()),
    });

    if decode {
        tracing::info!(
            namespace = ns,
            secret = name,
            "secret values decoded — decode=true was requested"
        );

        let decoded: BTreeMap<String, String> = secret
            .data
            .as_ref()
            .map(|data| {
                data.iter()
                    .map(|(k, ByteString(v))| (k.clone(), String::from_utf8_lossy(v).to_string()))
                    .collect()
            })
            .unwrap_or_default();
        result["data"] = serde_json::to_value(decoded).unwrap_or_default();
    }

    serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
}

async fn create_secret(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;
    let secret_type_str = args["type"].as_str().unwrap_or("Opaque");
    let string_data: BTreeMap<String, String> = serde_json::from_value(
        args.get("string_data")
            .ok_or("string_data is required")?
            .clone(),
    )
    .map_err(|e| e.to_string())?;

    let mut labels = BTreeMap::new();
    labels.insert(
        "app.kubernetes.io/managed-by".to_string(),
        "mcp-k8s".to_string(),
    );

    let secret = Secret {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            namespace: Some(ns.to_string()),
            labels: Some(labels),
            ..Default::default()
        },
        type_: Some(secret_type_str.to_string()),
        string_data: Some(string_data),
        ..Default::default()
    };

    let secrets_api = api(client, ns)?;
    let created = secrets_api
        .create(&PostParams::default(), &secret)
        .await
        .map_err(|e| e.to_string())?;

    let summary = secret_summary(&created);
    serde_json::to_string_pretty(&summary).map_err(|e| e.to_string())
}

async fn update_secret(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;
    let string_data: BTreeMap<String, String> = serde_json::from_value(
        args.get("string_data")
            .ok_or("string_data is required")?
            .clone(),
    )
    .map_err(|e| e.to_string())?;

    let patch = serde_json::json!({
        "stringData": string_data,
    });

    let secrets_api = api(client, ns)?;
    let patched = secrets_api
        .patch(name, &PatchParams::default(), &Patch::Strategic(patch))
        .await
        .map_err(|e| e.to_string())?;

    let summary = secret_summary(&patched);
    serde_json::to_string_pretty(&summary).map_err(|e| e.to_string())
}

async fn delete_secret(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;

    let secrets_api = api(client, ns)?;
    secrets_api
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
        let tools = tool_definitions();
        assert_eq!(tools.len(), 5, "Expected 5 tool definitions");

        let names: Vec<&str> = tools
            .iter()
            .map(|t| t["name"].as_str().expect("tool must have a name"))
            .collect();

        // Verify uniqueness
        let mut unique = names.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(
            names.len(),
            unique.len(),
            "Tool names must be unique, got: {:?}",
            names
        );

        // Verify expected names
        assert!(names.contains(&"list_secrets"));
        assert!(names.contains(&"get_secret"));
        assert!(names.contains(&"create_secret"));
        assert!(names.contains(&"update_secret"));
        assert!(names.contains(&"delete_secret"));
    }

    #[test]
    fn secret_summary_serialization() {
        let summary = SecretSummary {
            name: "my-secret".to_string(),
            namespace: "default".to_string(),
            secret_type: "Opaque".to_string(),
            data_keys: vec!["password".to_string(), "username".to_string()],
            created_at: Some("2024-01-01T00:00:00Z".to_string()),
        };

        let json = serde_json::to_value(&summary).expect("serialization should succeed");
        assert_eq!(json["name"], "my-secret");
        assert_eq!(json["namespace"], "default");
        assert_eq!(json["secret_type"], "Opaque");
        assert_eq!(
            json["data_keys"],
            serde_json::json!(["password", "username"])
        );
        assert_eq!(json["created_at"], "2024-01-01T00:00:00Z");

        // Verify round-trip: the serialized form should not contain any
        // secret values -- only metadata and key names.
        let raw = serde_json::to_string(&summary).unwrap();
        assert!(!raw.contains("\"data\""));
    }

    #[test]
    fn get_secret_default_output_does_not_contain_decoded_values() {
        // Simulate the extraction logic that get_secret uses when decode=false.
        // Build a Secret with actual data, then verify the JSON output excludes values.
        let mut data = BTreeMap::new();
        data.insert(
            "password".to_string(),
            ByteString(b"super-secret-value".to_vec()),
        );
        data.insert("api_key".to_string(), ByteString(b"ak_12345".to_vec()));

        let secret = Secret {
            metadata: ObjectMeta {
                name: Some("test-secret".to_string()),
                namespace: Some("default".to_string()),
                ..Default::default()
            },
            type_: Some("Opaque".to_string()),
            data: Some(data),
            ..Default::default()
        };

        let decode = false;
        let meta = &secret.metadata;
        let keys = data_keys(&secret);

        let mut result = serde_json::json!({
            "name": meta.name.clone().unwrap_or_default(),
            "namespace": meta.namespace.clone().unwrap_or_default(),
            "type": secret_type(&secret),
            "labels": meta.labels.clone().unwrap_or_default(),
            "annotations": meta.annotations.clone().unwrap_or_default(),
            "data_keys": keys,
        });

        if decode {
            let decoded: BTreeMap<String, String> = secret
                .data
                .as_ref()
                .map(|d| {
                    d.iter()
                        .map(|(k, ByteString(v))| {
                            (k.clone(), String::from_utf8_lossy(v).to_string())
                        })
                        .collect()
                })
                .unwrap_or_default();
            result["data"] = serde_json::to_value(decoded).unwrap_or_default();
        }

        let output = serde_json::to_string_pretty(&result).unwrap();

        // The output must contain the key names
        assert!(output.contains("password"));
        assert!(output.contains("api_key"));

        // The output must NOT contain the actual secret values
        assert!(
            !output.contains("super-secret-value"),
            "Default output must not contain decoded secret values"
        );
        assert!(
            !output.contains("ak_12345"),
            "Default output must not contain decoded secret values"
        );

        // The output must NOT have a "data" field with values
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert!(
            parsed.get("data").is_none(),
            "Default output must not include 'data' field"
        );
    }

    #[test]
    fn get_secret_decode_true_includes_values() {
        // Verify that when decode=true, the values ARE included.
        let mut data = BTreeMap::new();
        data.insert(
            "password".to_string(),
            ByteString(b"super-secret-value".to_vec()),
        );

        let secret = Secret {
            metadata: ObjectMeta {
                name: Some("test-secret".to_string()),
                namespace: Some("default".to_string()),
                ..Default::default()
            },
            type_: Some("Opaque".to_string()),
            data: Some(data),
            ..Default::default()
        };

        let decode = true;
        let meta = &secret.metadata;
        let keys = data_keys(&secret);

        let mut result = serde_json::json!({
            "name": meta.name.clone().unwrap_or_default(),
            "namespace": meta.namespace.clone().unwrap_or_default(),
            "type": secret_type(&secret),
            "data_keys": keys,
        });

        if decode {
            let decoded: BTreeMap<String, String> = secret
                .data
                .as_ref()
                .map(|d| {
                    d.iter()
                        .map(|(k, ByteString(v))| {
                            (k.clone(), String::from_utf8_lossy(v).to_string())
                        })
                        .collect()
                })
                .unwrap_or_default();
            result["data"] = serde_json::to_value(decoded).unwrap_or_default();
        }

        let output = serde_json::to_string_pretty(&result).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();

        assert!(
            parsed.get("data").is_some(),
            "decode=true must include 'data' field"
        );
        assert_eq!(parsed["data"]["password"], "super-secret-value");
    }
}
