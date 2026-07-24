use std::collections::BTreeMap;

use k8s_openapi::api::core::v1::{LimitRange, LimitRangeItem, LimitRangeSpec};
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::api::{DeleteParams, ListParams, PostParams};
use serde::Serialize;

use crate::client::K8sClient;

fn api(client: &K8sClient, ns: &str) -> Result<kube::Api<LimitRange>, String> {
    if !client.is_namespace_allowed(ns) {
        return Err(format!("Namespace '{ns}' is not in the allowed list"));
    }
    Ok(kube::Api::namespaced(client.inner().clone(), ns))
}

#[derive(Serialize, Debug)]
pub struct LimitRangeSummary {
    pub name: String,
    pub namespace: String,
    pub limits_count: usize,
    pub created_at: Option<String>,
}

#[derive(Serialize, Debug)]
pub struct LimitItemSummary {
    #[serde(rename = "type")]
    pub type_: String,
    pub max: BTreeMap<String, String>,
    pub min: BTreeMap<String, String>,
    pub default: BTreeMap<String, String>,
    pub default_request: BTreeMap<String, String>,
}

fn quantity_map_to_strings(m: &Option<BTreeMap<String, Quantity>>) -> BTreeMap<String, String> {
    m.as_ref()
        .map(|map| map.iter().map(|(k, v)| (k.clone(), v.0.clone())).collect())
        .unwrap_or_default()
}

fn extract_summary(lr: &LimitRange) -> LimitRangeSummary {
    let meta = &lr.metadata;
    let limits_count = lr.spec.as_ref().map(|s| s.limits.len()).unwrap_or(0);

    LimitRangeSummary {
        name: meta.name.clone().unwrap_or_default(),
        namespace: meta.namespace.clone().unwrap_or_default(),
        limits_count,
        created_at: meta.creation_timestamp.as_ref().map(|t| t.0.to_string()),
    }
}

fn extract_limit_item(item: &LimitRangeItem) -> LimitItemSummary {
    LimitItemSummary {
        type_: item.type_.clone(),
        max: quantity_map_to_strings(&item.max),
        min: quantity_map_to_strings(&item.min),
        default: quantity_map_to_strings(&item.default),
        default_request: quantity_map_to_strings(&item.default_request),
    }
}

pub fn tool_definitions() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "name": "list_limitranges",
            "description": "List LimitRanges in a namespace. Returns name, namespace, limits count, and created_at.",
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
            "name": "get_limitrange",
            "description": "Get a LimitRange by name. Returns limits (array of items with type, max, min, default, default_request), labels, and annotations.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "LimitRange name" }
                },
                "required": ["namespace", "name"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "create_limitrange",
            "description": "Create a LimitRange in a namespace with the given limit items.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "LimitRange name" },
                    "limits": {
                        "type": "array",
                        "description": "Array of limit items",
                        "items": {
                            "type": "object",
                            "properties": {
                                "type": { "type": "string", "description": "Type of resource (e.g. Container, Pod, PersistentVolumeClaim)" },
                                "max": {
                                    "type": "object",
                                    "description": "Max usage constraints by resource name",
                                    "additionalProperties": { "type": "string" }
                                },
                                "min": {
                                    "type": "object",
                                    "description": "Min usage constraints by resource name",
                                    "additionalProperties": { "type": "string" }
                                },
                                "default": {
                                    "type": "object",
                                    "description": "Default resource limit values by resource name",
                                    "additionalProperties": { "type": "string" }
                                },
                                "default_request": {
                                    "type": "object",
                                    "description": "Default resource request values by resource name",
                                    "additionalProperties": { "type": "string" }
                                }
                            },
                            "required": ["type"],
                            "additionalProperties": false
                        }
                    }
                },
                "required": ["namespace", "name", "limits"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "delete_limitrange",
            "description": "Delete a LimitRange by name from a namespace.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "LimitRange name" }
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
        "list_limitranges" => list_limitranges(client, args).await,
        "get_limitrange" => get_limitrange(client, args).await,
        "create_limitrange" => create_limitrange(client, args).await,
        "delete_limitrange" => delete_limitrange(client, args).await,
        _ => return None,
    };
    Some(result)
}

async fn list_limitranges(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let lr_api = api(client, ns)?;
    let label_selector = args["label_selector"].as_str();
    let mut lp = ListParams::default();
    if let Some(sel) = label_selector {
        lp = lp.labels(sel);
    }
    let list = lr_api.list(&lp).await.map_err(|e| e.to_string())?;

    let summaries: Vec<serde_json::Value> = list
        .iter()
        .map(|lr| {
            let s = extract_summary(lr);
            serde_json::json!({
                "name": s.name,
                "namespace": s.namespace,
                "limits_count": s.limits_count,
                "created_at": s.created_at,
            })
        })
        .collect();

    serde_json::to_string_pretty(&summaries).map_err(|e| e.to_string())
}

async fn get_limitrange(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;
    let lr_api = api(client, ns)?;
    let lr = lr_api.get(name).await.map_err(|e| e.to_string())?;

    let meta = &lr.metadata;
    let limits: Vec<LimitItemSummary> = lr
        .spec
        .as_ref()
        .map(|s| s.limits.iter().map(extract_limit_item).collect())
        .unwrap_or_default();

    let result = serde_json::json!({
        "name": meta.name.clone().unwrap_or_default(),
        "namespace": meta.namespace.clone().unwrap_or_default(),
        "limits": limits,
        "labels": meta.labels.clone().unwrap_or_default(),
        "annotations": meta.annotations.clone().unwrap_or_default(),
    });

    serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
}

fn parse_quantity_map(val: Option<&serde_json::Value>) -> Option<BTreeMap<String, Quantity>> {
    val.and_then(|v| {
        if v.is_null() {
            return None;
        }
        let map: BTreeMap<String, String> = serde_json::from_value(v.clone()).ok()?;
        if map.is_empty() {
            return None;
        }
        Some(map.into_iter().map(|(k, v)| (k, Quantity(v))).collect())
    })
}

async fn create_limitrange(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;
    let limits_val = args.get("limits").ok_or("limits is required")?;
    let limits_arr = limits_val.as_array().ok_or("limits must be an array")?;

    let mut items = Vec::new();
    for item in limits_arr {
        let type_ = item["type"]
            .as_str()
            .ok_or("each limit item must have a type")?
            .to_string();

        items.push(LimitRangeItem {
            type_,
            max: parse_quantity_map(item.get("max")),
            min: parse_quantity_map(item.get("min")),
            default: parse_quantity_map(item.get("default")),
            default_request: parse_quantity_map(item.get("default_request")),
            ..Default::default()
        });
    }

    let mut labels = BTreeMap::new();
    labels.insert(
        "app.kubernetes.io/managed-by".to_string(),
        "mcp-k8s".to_string(),
    );

    let lr = LimitRange {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            namespace: Some(ns.to_string()),
            labels: Some(labels),
            ..Default::default()
        },
        spec: Some(LimitRangeSpec { limits: items }),
    };

    let lr_api = api(client, ns)?;
    let created = lr_api
        .create(&PostParams::default(), &lr)
        .await
        .map_err(|e| e.to_string())?;

    let summary = extract_summary(&created);
    serde_json::to_string_pretty(&summary).map_err(|e| e.to_string())
}

async fn delete_limitrange(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;

    let lr_api = api(client, ns)?;
    lr_api
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

        assert!(names.contains(&"list_limitranges"));
        assert!(names.contains(&"get_limitrange"));
        assert!(names.contains(&"create_limitrange"));
        assert!(names.contains(&"delete_limitrange"));
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
    fn limitrange_summary_serialization() {
        let summary = LimitRangeSummary {
            name: "my-limits".to_string(),
            namespace: "default".to_string(),
            limits_count: 2,
            created_at: Some("2024-01-01T00:00:00Z".to_string()),
        };

        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["name"], "my-limits");
        assert_eq!(json["namespace"], "default");
        assert_eq!(json["limits_count"], 2);
        assert_eq!(json["created_at"], "2024-01-01T00:00:00Z");
    }

    #[test]
    fn limitrange_summary_serialization_empty_fields() {
        let summary = LimitRangeSummary {
            name: "empty".to_string(),
            namespace: "ns".to_string(),
            limits_count: 0,
            created_at: None,
        };

        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["name"], "empty");
        assert_eq!(json["limits_count"], 0);
        assert!(json["created_at"].is_null());
    }

    #[test]
    fn limit_item_summary_serialization() {
        let item = LimitItemSummary {
            type_: "Container".to_string(),
            max: BTreeMap::from([
                ("cpu".to_string(), "2".to_string()),
                ("memory".to_string(), "1Gi".to_string()),
            ]),
            min: BTreeMap::from([
                ("cpu".to_string(), "100m".to_string()),
                ("memory".to_string(), "64Mi".to_string()),
            ]),
            default: BTreeMap::from([
                ("cpu".to_string(), "500m".to_string()),
                ("memory".to_string(), "256Mi".to_string()),
            ]),
            default_request: BTreeMap::from([
                ("cpu".to_string(), "200m".to_string()),
                ("memory".to_string(), "128Mi".to_string()),
            ]),
        };

        let json = serde_json::to_value(&item).unwrap();
        assert_eq!(json["type"], "Container");
        assert_eq!(json["max"]["cpu"], "2");
        assert_eq!(json["max"]["memory"], "1Gi");
        assert_eq!(json["min"]["cpu"], "100m");
        assert_eq!(json["min"]["memory"], "64Mi");
        assert_eq!(json["default"]["cpu"], "500m");
        assert_eq!(json["default"]["memory"], "256Mi");
        assert_eq!(json["default_request"]["cpu"], "200m");
        assert_eq!(json["default_request"]["memory"], "128Mi");
    }

    #[test]
    fn limit_item_summary_serialization_empty_maps() {
        let item = LimitItemSummary {
            type_: "Pod".to_string(),
            max: BTreeMap::new(),
            min: BTreeMap::new(),
            default: BTreeMap::new(),
            default_request: BTreeMap::new(),
        };

        let json = serde_json::to_value(&item).unwrap();
        assert_eq!(json["type"], "Pod");
        assert!(json["max"].as_object().unwrap().is_empty());
        assert!(json["min"].as_object().unwrap().is_empty());
        assert!(json["default"].as_object().unwrap().is_empty());
        assert!(json["default_request"].as_object().unwrap().is_empty());
    }

    #[test]
    fn extract_summary_from_limitrange() {
        let lr = LimitRange {
            metadata: ObjectMeta {
                name: Some("test-lr".to_string()),
                namespace: Some("prod".to_string()),
                labels: Some(BTreeMap::from([(
                    "env".to_string(),
                    "production".to_string(),
                )])),
                ..Default::default()
            },
            spec: Some(LimitRangeSpec {
                limits: vec![
                    LimitRangeItem {
                        type_: "Container".to_string(),
                        max: Some(BTreeMap::from([(
                            "cpu".to_string(),
                            Quantity("2".to_string()),
                        )])),
                        min: Some(BTreeMap::from([(
                            "cpu".to_string(),
                            Quantity("100m".to_string()),
                        )])),
                        default: Some(BTreeMap::from([(
                            "cpu".to_string(),
                            Quantity("500m".to_string()),
                        )])),
                        default_request: Some(BTreeMap::from([(
                            "cpu".to_string(),
                            Quantity("200m".to_string()),
                        )])),
                        ..Default::default()
                    },
                    LimitRangeItem {
                        type_: "Pod".to_string(),
                        max: Some(BTreeMap::from([(
                            "memory".to_string(),
                            Quantity("4Gi".to_string()),
                        )])),
                        ..Default::default()
                    },
                ],
            }),
        };

        let summary = extract_summary(&lr);
        assert_eq!(summary.name, "test-lr");
        assert_eq!(summary.namespace, "prod");
        assert_eq!(summary.limits_count, 2);
        assert!(summary.created_at.is_none());
    }

    #[test]
    fn extract_summary_from_empty_limitrange() {
        let lr = LimitRange {
            metadata: ObjectMeta {
                name: Some("empty-lr".to_string()),
                namespace: Some("default".to_string()),
                ..Default::default()
            },
            spec: None,
        };

        let summary = extract_summary(&lr);
        assert_eq!(summary.name, "empty-lr");
        assert_eq!(summary.namespace, "default");
        assert_eq!(summary.limits_count, 0);
        assert!(summary.created_at.is_none());
    }

    #[test]
    fn extract_limit_item_full() {
        let item = LimitRangeItem {
            type_: "Container".to_string(),
            max: Some(BTreeMap::from([
                ("cpu".to_string(), Quantity("4".to_string())),
                ("memory".to_string(), Quantity("2Gi".to_string())),
            ])),
            min: Some(BTreeMap::from([(
                "cpu".to_string(),
                Quantity("50m".to_string()),
            )])),
            default: Some(BTreeMap::from([(
                "cpu".to_string(),
                Quantity("1".to_string()),
            )])),
            default_request: Some(BTreeMap::from([(
                "memory".to_string(),
                Quantity("256Mi".to_string()),
            )])),
            ..Default::default()
        };

        let summary = extract_limit_item(&item);
        assert_eq!(summary.type_, "Container");
        assert_eq!(summary.max.get("cpu").unwrap(), "4");
        assert_eq!(summary.max.get("memory").unwrap(), "2Gi");
        assert_eq!(summary.min.get("cpu").unwrap(), "50m");
        assert_eq!(summary.default.get("cpu").unwrap(), "1");
        assert_eq!(summary.default_request.get("memory").unwrap(), "256Mi");
    }

    #[test]
    fn extract_limit_item_empty() {
        let item = LimitRangeItem {
            type_: "PersistentVolumeClaim".to_string(),
            ..Default::default()
        };

        let summary = extract_limit_item(&item);
        assert_eq!(summary.type_, "PersistentVolumeClaim");
        assert!(summary.max.is_empty());
        assert!(summary.min.is_empty());
        assert!(summary.default.is_empty());
        assert!(summary.default_request.is_empty());
    }
}
