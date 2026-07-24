use std::collections::BTreeMap;

use k8s_openapi::api::autoscaling::v2::{
    CrossVersionObjectReference, HorizontalPodAutoscaler, HorizontalPodAutoscalerSpec, MetricSpec,
    MetricTarget, ResourceMetricSource,
};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::api::{DeleteParams, ListParams, Patch, PatchParams, PostParams};
use serde::Serialize;

use crate::client::K8sClient;

type Hpa = HorizontalPodAutoscaler;

fn api(client: &K8sClient, ns: &str) -> Result<kube::Api<Hpa>, String> {
    if !client.is_namespace_allowed(ns) {
        return Err(format!("Namespace '{ns}' is not in the allowed list"));
    }
    Ok(kube::Api::namespaced(client.inner().clone(), ns))
}

// ---------------------------------------------------------------------------
// Summary types
// ---------------------------------------------------------------------------

#[derive(Serialize, Debug)]
pub struct HpaSummary {
    pub name: String,
    pub namespace: String,
    pub scale_target_kind: String,
    pub scale_target_name: String,
    pub min_replicas: Option<i32>,
    pub max_replicas: i32,
    pub current_replicas: Option<i32>,
    pub created_at: Option<String>,
}

#[derive(Serialize, Debug)]
struct HpaConditionSummary {
    condition_type: String,
    status: String,
    reason: Option<String>,
    message: Option<String>,
    last_transition: Option<String>,
}

#[derive(Serialize, Debug)]
struct HpaDetail {
    name: String,
    namespace: String,
    scale_target_kind: String,
    scale_target_name: String,
    min_replicas: Option<i32>,
    max_replicas: i32,
    current_replicas: Option<i32>,
    desired_replicas: Option<i32>,
    created_at: Option<String>,
    labels: BTreeMap<String, String>,
    annotations: BTreeMap<String, String>,
    metrics: serde_json::Value,
    conditions: Vec<HpaConditionSummary>,
    current_metrics: serde_json::Value,
}

// ---------------------------------------------------------------------------
// Extraction helpers
// ---------------------------------------------------------------------------

fn extract_summary(hpa: &Hpa) -> HpaSummary {
    let meta = &hpa.metadata;
    let spec = hpa.spec.as_ref();
    let status = hpa.status.as_ref();

    let (scale_target_kind, scale_target_name) = spec
        .map(|s| {
            (
                s.scale_target_ref.kind.clone(),
                s.scale_target_ref.name.clone(),
            )
        })
        .unwrap_or_default();

    HpaSummary {
        name: meta.name.clone().unwrap_or_default(),
        namespace: meta.namespace.clone().unwrap_or_default(),
        scale_target_kind,
        scale_target_name,
        min_replicas: spec.and_then(|s| s.min_replicas),
        max_replicas: spec.map(|s| s.max_replicas).unwrap_or(0),
        current_replicas: status.and_then(|s| s.current_replicas),
        created_at: meta.creation_timestamp.as_ref().map(|t| t.0.to_string()),
    }
}

fn extract_detail(hpa: &Hpa) -> HpaDetail {
    let meta = &hpa.metadata;
    let spec = hpa.spec.as_ref();
    let status = hpa.status.as_ref();

    let (scale_target_kind, scale_target_name) = spec
        .map(|s| {
            (
                s.scale_target_ref.kind.clone(),
                s.scale_target_ref.name.clone(),
            )
        })
        .unwrap_or_default();

    let metrics = spec
        .and_then(|s| s.metrics.as_ref())
        .map(|m| serde_json::to_value(m).unwrap_or_default())
        .unwrap_or(serde_json::Value::Array(vec![]));

    let conditions: Vec<HpaConditionSummary> = status
        .and_then(|s| s.conditions.as_ref())
        .map(|conds| {
            conds
                .iter()
                .map(|c| HpaConditionSummary {
                    condition_type: c.type_.clone(),
                    status: c.status.clone(),
                    reason: c.reason.clone(),
                    message: c.message.clone(),
                    last_transition: c.last_transition_time.as_ref().map(|t| t.0.to_string()),
                })
                .collect()
        })
        .unwrap_or_default();

    let current_metrics = status
        .and_then(|s| s.current_metrics.as_ref())
        .map(|m| serde_json::to_value(m).unwrap_or_default())
        .unwrap_or(serde_json::Value::Array(vec![]));

    HpaDetail {
        name: meta.name.clone().unwrap_or_default(),
        namespace: meta.namespace.clone().unwrap_or_default(),
        scale_target_kind,
        scale_target_name,
        min_replicas: spec.and_then(|s| s.min_replicas),
        max_replicas: spec.map(|s| s.max_replicas).unwrap_or(0),
        current_replicas: status.and_then(|s| s.current_replicas),
        desired_replicas: status.map(|s| s.desired_replicas),
        created_at: meta.creation_timestamp.as_ref().map(|t| t.0.to_string()),
        labels: meta.labels.clone().unwrap_or_default(),
        annotations: meta.annotations.clone().unwrap_or_default(),
        metrics,
        conditions,
        current_metrics,
    }
}

// ---------------------------------------------------------------------------
// Tool definitions
// ---------------------------------------------------------------------------

pub fn tool_definitions() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "name": "list_hpas",
            "description": "List HorizontalPodAutoscalers in a namespace with scale target, replica bounds, and current replicas.",
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
            "name": "get_hpa",
            "description": "Get detailed info for a single HorizontalPodAutoscaler including metrics specs, conditions, and current metrics.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "HPA name" }
                },
                "required": ["namespace", "name"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "create_hpa",
            "description": "Create a HorizontalPodAutoscaler targeting a scalable resource.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "HPA name" },
                    "scale_target_kind": { "type": "string", "description": "Kind of the scale target (e.g. Deployment)" },
                    "scale_target_name": { "type": "string", "description": "Name of the scale target" },
                    "scale_target_api_version": { "type": "string", "description": "API version of the scale target (e.g. apps/v1)" },
                    "min_replicas": { "type": "integer", "description": "Minimum number of replicas" },
                    "max_replicas": { "type": "integer", "description": "Maximum number of replicas" },
                    "target_cpu_utilization": { "type": "integer", "description": "Target average CPU utilization percentage (optional)" }
                },
                "required": ["namespace", "name", "scale_target_kind", "scale_target_name", "scale_target_api_version", "min_replicas", "max_replicas"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "update_hpa",
            "description": "Patch a HorizontalPodAutoscaler to update min and/or max replicas.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "HPA name" },
                    "min_replicas": { "type": "integer", "description": "New minimum number of replicas (optional)" },
                    "max_replicas": { "type": "integer", "description": "New maximum number of replicas (optional)" }
                },
                "required": ["namespace", "name"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "delete_hpa",
            "description": "Delete a HorizontalPodAutoscaler by name.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "HPA name" }
                },
                "required": ["namespace", "name"],
                "additionalProperties": false
            }
        }),
    ]
}

// ---------------------------------------------------------------------------
// Tool handler
// ---------------------------------------------------------------------------

pub async fn handle_tool(
    client: &K8sClient,
    name: &str,
    args: &serde_json::Value,
) -> Option<Result<String, String>> {
    let result = match name {
        "list_hpas" => list_hpas(client, args).await,
        "get_hpa" => get_hpa(client, args).await,
        "create_hpa" => create_hpa(client, args).await,
        "update_hpa" => update_hpa(client, args).await,
        "delete_hpa" => delete_hpa(client, args).await,
        _ => return None,
    };
    Some(result)
}

// ---------------------------------------------------------------------------
// Tool implementations
// ---------------------------------------------------------------------------

async fn list_hpas(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let hpa_api = api(client, ns)?;
    let label_selector = args["label_selector"].as_str();
    let mut lp = ListParams::default();
    if let Some(sel) = label_selector {
        lp = lp.labels(sel);
    }
    let list = hpa_api.list(&lp).await.map_err(|e| e.to_string())?;

    let summaries: Vec<serde_json::Value> = list
        .iter()
        .map(|hpa| {
            let s = extract_summary(hpa);
            serde_json::to_value(s).unwrap_or_default()
        })
        .collect();

    serde_json::to_string_pretty(&summaries).map_err(|e| e.to_string())
}

async fn get_hpa(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;

    let hpa_api = api(client, ns)?;
    let hpa = hpa_api.get(name).await.map_err(|e| e.to_string())?;
    let detail = extract_detail(&hpa);

    serde_json::to_string_pretty(&serde_json::to_value(detail).unwrap_or_default())
        .map_err(|e| e.to_string())
}

async fn create_hpa(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;
    let scale_target_kind = args["scale_target_kind"]
        .as_str()
        .ok_or("scale_target_kind is required")?;
    let scale_target_name = args["scale_target_name"]
        .as_str()
        .ok_or("scale_target_name is required")?;
    let scale_target_api_version = args["scale_target_api_version"]
        .as_str()
        .ok_or("scale_target_api_version is required")?;
    let min_replicas = args["min_replicas"]
        .as_i64()
        .ok_or("min_replicas is required")? as i32;
    let max_replicas = args["max_replicas"]
        .as_i64()
        .ok_or("max_replicas is required")? as i32;
    let target_cpu_utilization = args["target_cpu_utilization"].as_i64().map(|v| v as i32);

    let mut labels = BTreeMap::new();
    labels.insert(
        "app.kubernetes.io/managed-by".to_string(),
        "mcp-k8s".to_string(),
    );

    let metrics = target_cpu_utilization.map(|pct| {
        vec![MetricSpec {
            type_: "Resource".to_string(),
            resource: Some(ResourceMetricSource {
                name: "cpu".to_string(),
                target: MetricTarget {
                    type_: "Utilization".to_string(),
                    average_utilization: Some(pct),
                    ..Default::default()
                },
            }),
            ..Default::default()
        }]
    });

    let hpa = Hpa {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            namespace: Some(ns.to_string()),
            labels: Some(labels),
            ..Default::default()
        },
        spec: Some(HorizontalPodAutoscalerSpec {
            scale_target_ref: CrossVersionObjectReference {
                kind: scale_target_kind.to_string(),
                name: scale_target_name.to_string(),
                api_version: Some(scale_target_api_version.to_string()),
            },
            min_replicas: Some(min_replicas),
            max_replicas,
            metrics,
            ..Default::default()
        }),
        ..Default::default()
    };

    let hpa_api = api(client, ns)?;
    let created = hpa_api
        .create(&PostParams::default(), &hpa)
        .await
        .map_err(|e| e.to_string())?;

    let detail = extract_detail(&created);
    serde_json::to_string_pretty(&serde_json::to_value(detail).unwrap_or_default())
        .map_err(|e| e.to_string())
}

async fn update_hpa(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;
    let min_replicas = args["min_replicas"].as_i64().map(|v| v as i32);
    let max_replicas = args["max_replicas"].as_i64().map(|v| v as i32);

    let mut spec_patch = serde_json::Map::new();
    if let Some(min) = min_replicas {
        spec_patch.insert("minReplicas".to_string(), serde_json::json!(min));
    }
    if let Some(max) = max_replicas {
        spec_patch.insert("maxReplicas".to_string(), serde_json::json!(max));
    }

    let patch = serde_json::json!({ "spec": spec_patch });

    let hpa_api = api(client, ns)?;
    let patched = hpa_api
        .patch(name, &PatchParams::apply("mcp-k8s"), &Patch::Merge(&patch))
        .await
        .map_err(|e| e.to_string())?;

    let detail = extract_detail(&patched);
    serde_json::to_string_pretty(&serde_json::to_value(detail).unwrap_or_default())
        .map_err(|e| e.to_string())
}

async fn delete_hpa(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;

    let hpa_api = api(client, ns)?;
    hpa_api
        .delete(name, &DeleteParams::default())
        .await
        .map_err(|e| e.to_string())?;

    Ok(format!("HPA '{name}' deleted from namespace '{ns}'"))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_definitions_returns_five_tools() {
        let defs = tool_definitions();
        assert_eq!(defs.len(), 5);

        let names: Vec<&str> = defs.iter().filter_map(|d| d["name"].as_str()).collect();
        assert!(names.contains(&"list_hpas"));
        assert!(names.contains(&"get_hpa"));
        assert!(names.contains(&"create_hpa"));
        assert!(names.contains(&"update_hpa"));
        assert!(names.contains(&"delete_hpa"));
    }

    #[test]
    fn hpa_summary_serialization() {
        let summary = HpaSummary {
            name: "my-hpa".to_string(),
            namespace: "default".to_string(),
            scale_target_kind: "Deployment".to_string(),
            scale_target_name: "my-app".to_string(),
            min_replicas: Some(2),
            max_replicas: 10,
            current_replicas: Some(5),
            created_at: Some("2024-01-01T00:00:00Z".to_string()),
        };

        let json = serde_json::to_string_pretty(&summary).expect("should serialize");
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("should parse");

        assert_eq!(parsed["name"], "my-hpa");
        assert_eq!(parsed["namespace"], "default");
        assert_eq!(parsed["scale_target_kind"], "Deployment");
        assert_eq!(parsed["scale_target_name"], "my-app");
        assert_eq!(parsed["min_replicas"], 2);
        assert_eq!(parsed["max_replicas"], 10);
        assert_eq!(parsed["current_replicas"], 5);
        assert_eq!(parsed["created_at"], "2024-01-01T00:00:00Z");
    }

    #[test]
    fn hpa_summary_serialization_with_none() {
        let summary = HpaSummary {
            name: "my-hpa".to_string(),
            namespace: "default".to_string(),
            scale_target_kind: "Deployment".to_string(),
            scale_target_name: "my-app".to_string(),
            min_replicas: None,
            max_replicas: 10,
            current_replicas: None,
            created_at: None,
        };

        let json = serde_json::to_string_pretty(&summary).expect("should serialize");
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("should parse");

        assert!(parsed["min_replicas"].is_null());
        assert!(parsed["current_replicas"].is_null());
        assert!(parsed["created_at"].is_null());
    }

    #[test]
    fn extract_summary_from_hpa_object() {
        let hpa = Hpa {
            metadata: ObjectMeta {
                name: Some("web-hpa".to_string()),
                namespace: Some("production".to_string()),
                creation_timestamp: Some(k8s_openapi::apimachinery::pkg::apis::meta::v1::Time(
                    "2024-06-15T12:00:00Z"
                        .parse::<k8s_openapi::jiff::Timestamp>()
                        .unwrap(),
                )),
                ..Default::default()
            },
            spec: Some(HorizontalPodAutoscalerSpec {
                scale_target_ref: CrossVersionObjectReference {
                    kind: "Deployment".to_string(),
                    name: "web-server".to_string(),
                    api_version: Some("apps/v1".to_string()),
                },
                min_replicas: Some(3),
                max_replicas: 20,
                metrics: None,
                ..Default::default()
            }),
            status: Some(
                k8s_openapi::api::autoscaling::v2::HorizontalPodAutoscalerStatus {
                    current_replicas: Some(7),
                    desired_replicas: 10,
                    ..Default::default()
                },
            ),
        };

        let summary = extract_summary(&hpa);

        assert_eq!(summary.name, "web-hpa");
        assert_eq!(summary.namespace, "production");
        assert_eq!(summary.scale_target_kind, "Deployment");
        assert_eq!(summary.scale_target_name, "web-server");
        assert_eq!(summary.min_replicas, Some(3));
        assert_eq!(summary.max_replicas, 20);
        assert_eq!(summary.current_replicas, Some(7));
        assert!(summary.created_at.is_some());
    }

    #[test]
    fn extract_summary_from_minimal_hpa() {
        let hpa = Hpa {
            metadata: ObjectMeta::default(),
            spec: None,
            status: None,
        };

        let summary = extract_summary(&hpa);

        assert_eq!(summary.name, "");
        assert_eq!(summary.namespace, "");
        assert_eq!(summary.scale_target_kind, "");
        assert_eq!(summary.scale_target_name, "");
        assert_eq!(summary.min_replicas, None);
        assert_eq!(summary.max_replicas, 0);
        assert_eq!(summary.current_replicas, None);
        assert_eq!(summary.created_at, None);
    }

    #[test]
    fn extract_detail_includes_conditions_and_metrics() {
        let hpa = Hpa {
            metadata: ObjectMeta {
                name: Some("detail-hpa".to_string()),
                namespace: Some("staging".to_string()),
                labels: Some({
                    let mut m = BTreeMap::new();
                    m.insert("app".to_string(), "test".to_string());
                    m
                }),
                annotations: Some({
                    let mut m = BTreeMap::new();
                    m.insert("note".to_string(), "testing".to_string());
                    m
                }),
                ..Default::default()
            },
            spec: Some(HorizontalPodAutoscalerSpec {
                scale_target_ref: CrossVersionObjectReference {
                    kind: "Deployment".to_string(),
                    name: "api".to_string(),
                    api_version: Some("apps/v1".to_string()),
                },
                min_replicas: Some(1),
                max_replicas: 5,
                metrics: Some(vec![MetricSpec {
                    type_: "Resource".to_string(),
                    resource: Some(ResourceMetricSource {
                        name: "cpu".to_string(),
                        target: MetricTarget {
                            type_: "Utilization".to_string(),
                            average_utilization: Some(80),
                            ..Default::default()
                        },
                    }),
                    ..Default::default()
                }]),
                ..Default::default()
            }),
            status: Some(
                k8s_openapi::api::autoscaling::v2::HorizontalPodAutoscalerStatus {
                    current_replicas: Some(2),
                    desired_replicas: 3,
                    conditions: Some(vec![
                        k8s_openapi::api::autoscaling::v2::HorizontalPodAutoscalerCondition {
                            type_: "AbleToScale".to_string(),
                            status: "True".to_string(),
                            reason: Some("ReadyForNewScale".to_string()),
                            message: Some("recommended size matches current size".to_string()),
                            last_transition_time: None,
                        },
                    ]),
                    ..Default::default()
                },
            ),
        };

        let detail = extract_detail(&hpa);

        assert_eq!(detail.name, "detail-hpa");
        assert_eq!(detail.namespace, "staging");
        assert_eq!(detail.labels.get("app").map(|s| s.as_str()), Some("test"));
        assert_eq!(
            detail.annotations.get("note").map(|s| s.as_str()),
            Some("testing")
        );
        assert_eq!(detail.conditions.len(), 1);
        assert_eq!(detail.conditions[0].condition_type, "AbleToScale");
        assert_eq!(detail.conditions[0].status, "True");
        assert!(detail.metrics.is_array());
        assert_eq!(detail.desired_replicas, Some(3));
    }
}
