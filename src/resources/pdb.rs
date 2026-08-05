use std::collections::BTreeMap;

use k8s_openapi::api::policy::v1::{PodDisruptionBudget, PodDisruptionBudgetSpec};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{LabelSelector, ObjectMeta};
use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;
use kube::api::{DeleteParams, ListParams, Patch, PatchParams, PostParams};
use serde::Serialize;

use crate::client::K8sClient;

type Pdb = PodDisruptionBudget;

fn api(client: &K8sClient, ns: &str) -> Result<kube::Api<Pdb>, String> {
    if !client.is_namespace_allowed(ns) {
        return Err(format!("Namespace '{ns}' is not in the allowed list"));
    }
    Ok(kube::Api::namespaced(client.inner().clone(), ns))
}

// ---------------------------------------------------------------------------
// Summary types
// ---------------------------------------------------------------------------

#[derive(Serialize, Debug)]
pub struct PdbSummary {
    pub name: String,
    pub namespace: String,
    pub min_available: Option<String>,
    pub max_unavailable: Option<String>,
    pub current_healthy: Option<i32>,
    pub disruptions_allowed: Option<i32>,
    pub expected_pods: Option<i32>,
    pub created_at: Option<String>,
}

#[derive(Serialize, Debug)]
struct PdbConditionSummary {
    condition_type: String,
    status: String,
    reason: Option<String>,
    message: Option<String>,
    last_transition: Option<String>,
}

#[derive(Serialize, Debug)]
struct PdbDetail {
    name: String,
    namespace: String,
    min_available: Option<String>,
    max_unavailable: Option<String>,
    current_healthy: Option<i32>,
    disruptions_allowed: Option<i32>,
    expected_pods: Option<i32>,
    created_at: Option<String>,
    selector: Option<BTreeMap<String, String>>,
    conditions: Vec<PdbConditionSummary>,
    labels: BTreeMap<String, String>,
    annotations: BTreeMap<String, String>,
}

// ---------------------------------------------------------------------------
// Extraction helpers
// ---------------------------------------------------------------------------

fn int_or_string_to_string(ios: &IntOrString) -> String {
    match ios {
        IntOrString::Int(i) => i.to_string(),
        IntOrString::String(s) => s.clone(),
    }
}

fn extract_summary(pdb: &Pdb) -> PdbSummary {
    let meta = &pdb.metadata;
    let spec = pdb.spec.as_ref();
    let status = pdb.status.as_ref();

    PdbSummary {
        name: meta.name.clone().unwrap_or_default(),
        namespace: meta.namespace.clone().unwrap_or_default(),
        min_available: spec
            .and_then(|s| s.min_available.as_ref())
            .map(int_or_string_to_string),
        max_unavailable: spec
            .and_then(|s| s.max_unavailable.as_ref())
            .map(int_or_string_to_string),
        current_healthy: status.map(|s| s.current_healthy),
        disruptions_allowed: status.map(|s| s.disruptions_allowed),
        expected_pods: status.map(|s| s.expected_pods),
        created_at: meta.creation_timestamp.as_ref().map(|t| t.0.to_string()),
    }
}

fn extract_detail(pdb: &Pdb) -> PdbDetail {
    let meta = &pdb.metadata;
    let spec = pdb.spec.as_ref();
    let status = pdb.status.as_ref();

    let selector = spec
        .and_then(|s| s.selector.as_ref())
        .and_then(|sel| sel.match_labels.clone());

    let conditions: Vec<PdbConditionSummary> = status
        .and_then(|s| s.conditions.as_ref())
        .map(|conds| {
            conds
                .iter()
                .map(|c| PdbConditionSummary {
                    condition_type: c.type_.clone(),
                    status: c.status.clone(),
                    reason: Some(c.reason.clone()),
                    message: Some(c.message.clone()),
                    last_transition: Some(c.last_transition_time.0.to_string()),
                })
                .collect()
        })
        .unwrap_or_default();

    PdbDetail {
        name: meta.name.clone().unwrap_or_default(),
        namespace: meta.namespace.clone().unwrap_or_default(),
        min_available: spec
            .and_then(|s| s.min_available.as_ref())
            .map(int_or_string_to_string),
        max_unavailable: spec
            .and_then(|s| s.max_unavailable.as_ref())
            .map(int_or_string_to_string),
        current_healthy: status.map(|s| s.current_healthy),
        disruptions_allowed: status.map(|s| s.disruptions_allowed),
        expected_pods: status.map(|s| s.expected_pods),
        created_at: meta.creation_timestamp.as_ref().map(|t| t.0.to_string()),
        selector,
        conditions,
        labels: meta.labels.clone().unwrap_or_default(),
        annotations: meta.annotations.clone().unwrap_or_default(),
    }
}

// ---------------------------------------------------------------------------
// Tool definitions
// ---------------------------------------------------------------------------

pub fn tool_definitions() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "name": "list_pdbs",
            "description": "List PodDisruptionBudgets in a namespace with name, min_available, max_unavailable, healthy/disruption counts, and expected pods.",
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
            "name": "get_pdb",
            "description": "Get detailed info for a single PodDisruptionBudget including selector, conditions, labels, and annotations.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "PDB name" }
                },
                "required": ["namespace", "name"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "create_pdb",
            "description": "Create a PodDisruptionBudget. Provide either min_available or max_unavailable (integer or percentage string like \"50%\"), plus a label selector.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "PDB name" },
                    "min_available": {
                        "oneOf": [
                            { "type": "integer", "description": "Minimum available pods (absolute count)" },
                            { "type": "string", "description": "Minimum available pods (percentage, e.g. \"50%\")" }
                        ],
                        "description": "Minimum number of pods that must remain available (mutually exclusive with max_unavailable)"
                    },
                    "max_unavailable": {
                        "oneOf": [
                            { "type": "integer", "description": "Maximum unavailable pods (absolute count)" },
                            { "type": "string", "description": "Maximum unavailable pods (percentage, e.g. \"25%\")" }
                        ],
                        "description": "Maximum number of pods that can be unavailable (mutually exclusive with min_available)"
                    },
                    "selector": {
                        "type": "object",
                        "description": "Label selector match_labels as key-value pairs",
                        "additionalProperties": { "type": "string" }
                    }
                },
                "required": ["namespace", "name", "selector"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "update_pdb",
            "description": "Patch a PodDisruptionBudget to update min_available and/or max_unavailable.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "PDB name" },
                    "min_available": {
                        "oneOf": [
                            { "type": "integer", "description": "Minimum available pods (absolute count)" },
                            { "type": "string", "description": "Minimum available pods (percentage, e.g. \"50%\")" }
                        ],
                        "description": "New minimum available value (optional)"
                    },
                    "max_unavailable": {
                        "oneOf": [
                            { "type": "integer", "description": "Maximum unavailable pods (absolute count)" },
                            { "type": "string", "description": "Maximum unavailable pods (percentage, e.g. \"25%\")" }
                        ],
                        "description": "New maximum unavailable value (optional)"
                    }
                },
                "required": ["namespace", "name"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "delete_pdb",
            "description": "Delete a PodDisruptionBudget by name.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "PDB name" }
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
        "list_pdbs" => list_pdbs(client, args).await,
        "get_pdb" => get_pdb(client, args).await,
        "create_pdb" => create_pdb(client, args).await,
        "update_pdb" => update_pdb(client, args).await,
        "delete_pdb" => delete_pdb(client, args).await,
        _ => return None,
    };
    Some(result)
}

// ---------------------------------------------------------------------------
// Argument helpers
// ---------------------------------------------------------------------------

/// Parse an IntOrString from a JSON value that may be an integer or a string.
fn parse_int_or_string(val: &serde_json::Value) -> Option<IntOrString> {
    if let Some(i) = val.as_i64() {
        Some(IntOrString::Int(i as i32))
    } else {
        val.as_str().map(|s| IntOrString::String(s.to_string()))
    }
}

// ---------------------------------------------------------------------------
// Tool implementations
// ---------------------------------------------------------------------------

async fn list_pdbs(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let pdb_api = api(client, ns)?;
    let label_selector = args["label_selector"].as_str();
    let mut lp = ListParams::default();
    if let Some(sel) = label_selector {
        lp = lp.labels(sel);
    }
    let list = pdb_api.list(&lp).await.map_err(|e| e.to_string())?;

    let summaries: Vec<serde_json::Value> = list
        .iter()
        .map(|pdb| {
            let s = extract_summary(pdb);
            serde_json::to_value(s).unwrap_or_default()
        })
        .collect();

    serde_json::to_string_pretty(&summaries).map_err(|e| e.to_string())
}

async fn get_pdb(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;

    let pdb_api = api(client, ns)?;
    let pdb = pdb_api.get(name).await.map_err(|e| e.to_string())?;
    let detail = extract_detail(&pdb);

    serde_json::to_string_pretty(&serde_json::to_value(detail).unwrap_or_default())
        .map_err(|e| e.to_string())
}

async fn create_pdb(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;

    let min_available = parse_int_or_string(&args["min_available"]);
    let max_unavailable = parse_int_or_string(&args["max_unavailable"]);

    if min_available.is_none() && max_unavailable.is_none() {
        return Err("Either min_available or max_unavailable is required".to_string());
    }

    let match_labels: BTreeMap<String, String> = args["selector"]
        .as_object()
        .ok_or("selector is required and must be an object")?
        .iter()
        .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
        .collect();

    let mut labels = BTreeMap::new();
    labels.insert(
        "app.kubernetes.io/managed-by".to_string(),
        "mcp-k8s".to_string(),
    );

    let pdb = Pdb {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            namespace: Some(ns.to_string()),
            labels: Some(labels),
            ..Default::default()
        },
        spec: Some(PodDisruptionBudgetSpec {
            min_available,
            max_unavailable,
            selector: Some(LabelSelector {
                match_labels: Some(match_labels),
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    };

    let pdb_api = api(client, ns)?;
    let created = pdb_api
        .create(&PostParams::default(), &pdb)
        .await
        .map_err(|e| e.to_string())?;

    let detail = extract_detail(&created);
    serde_json::to_string_pretty(&serde_json::to_value(detail).unwrap_or_default())
        .map_err(|e| e.to_string())
}

async fn update_pdb(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;

    let min_available = parse_int_or_string(&args["min_available"]);
    let max_unavailable = parse_int_or_string(&args["max_unavailable"]);

    let mut spec_patch = serde_json::Map::new();
    if let Some(val) = min_available {
        match val {
            IntOrString::Int(i) => {
                spec_patch.insert("minAvailable".to_string(), serde_json::json!(i));
            }
            IntOrString::String(s) => {
                spec_patch.insert("minAvailable".to_string(), serde_json::json!(s));
            }
        }
    }
    if let Some(val) = max_unavailable {
        match val {
            IntOrString::Int(i) => {
                spec_patch.insert("maxUnavailable".to_string(), serde_json::json!(i));
            }
            IntOrString::String(s) => {
                spec_patch.insert("maxUnavailable".to_string(), serde_json::json!(s));
            }
        }
    }

    let patch = serde_json::json!({ "spec": spec_patch });

    let pdb_api = api(client, ns)?;
    let patched = pdb_api
        .patch(name, &PatchParams::default(), &Patch::Strategic(patch))
        .await
        .map_err(|e| e.to_string())?;

    let detail = extract_detail(&patched);
    serde_json::to_string_pretty(&serde_json::to_value(detail).unwrap_or_default())
        .map_err(|e| e.to_string())
}

async fn delete_pdb(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;

    let pdb_api = api(client, ns)?;
    pdb_api
        .delete(name, &DeleteParams::default())
        .await
        .map_err(|e| e.to_string())?;

    Ok(format!(
        "PodDisruptionBudget '{}' deleted from namespace '{}'",
        name, ns
    ))
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
        assert!(names.contains(&"list_pdbs"));
        assert!(names.contains(&"get_pdb"));
        assert!(names.contains(&"create_pdb"));
        assert!(names.contains(&"update_pdb"));
        assert!(names.contains(&"delete_pdb"));
    }

    #[test]
    fn tool_definitions_have_input_schemas() {
        let defs = tool_definitions();
        for def in &defs {
            assert!(
                def["inputSchema"].is_object(),
                "tool '{}' missing inputSchema",
                def["name"]
            );
            assert_eq!(def["inputSchema"]["type"], "object");
        }
    }

    #[test]
    fn pdb_summary_serialization() {
        let summary = PdbSummary {
            name: "my-pdb".to_string(),
            namespace: "default".to_string(),
            min_available: Some("2".to_string()),
            max_unavailable: None,
            current_healthy: Some(5),
            disruptions_allowed: Some(3),
            expected_pods: Some(5),
            created_at: Some("2024-01-01T00:00:00Z".to_string()),
        };

        let json = serde_json::to_string_pretty(&summary).expect("should serialize");
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("should parse");

        assert_eq!(parsed["name"], "my-pdb");
        assert_eq!(parsed["namespace"], "default");
        assert_eq!(parsed["min_available"], "2");
        assert!(parsed["max_unavailable"].is_null());
        assert_eq!(parsed["current_healthy"], 5);
        assert_eq!(parsed["disruptions_allowed"], 3);
        assert_eq!(parsed["expected_pods"], 5);
        assert_eq!(parsed["created_at"], "2024-01-01T00:00:00Z");
    }

    #[test]
    fn pdb_summary_serialization_with_none() {
        let summary = PdbSummary {
            name: "my-pdb".to_string(),
            namespace: "default".to_string(),
            min_available: None,
            max_unavailable: Some("25%".to_string()),
            current_healthy: None,
            disruptions_allowed: None,
            expected_pods: None,
            created_at: None,
        };

        let json = serde_json::to_string_pretty(&summary).expect("should serialize");
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("should parse");

        assert!(parsed["min_available"].is_null());
        assert_eq!(parsed["max_unavailable"], "25%");
        assert!(parsed["current_healthy"].is_null());
        assert!(parsed["disruptions_allowed"].is_null());
        assert!(parsed["expected_pods"].is_null());
        assert!(parsed["created_at"].is_null());
    }

    #[test]
    fn extract_summary_from_pdb_object() {
        let pdb = Pdb {
            metadata: ObjectMeta {
                name: Some("web-pdb".to_string()),
                namespace: Some("production".to_string()),
                creation_timestamp: Some(k8s_openapi::apimachinery::pkg::apis::meta::v1::Time(
                    "2024-06-15T12:00:00Z"
                        .parse::<k8s_openapi::jiff::Timestamp>()
                        .unwrap(),
                )),
                ..Default::default()
            },
            spec: Some(PodDisruptionBudgetSpec {
                min_available: Some(IntOrString::Int(2)),
                max_unavailable: None,
                selector: Some(LabelSelector {
                    match_labels: Some({
                        let mut m = BTreeMap::new();
                        m.insert("app".to_string(), "web".to_string());
                        m
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            status: Some(k8s_openapi::api::policy::v1::PodDisruptionBudgetStatus {
                current_healthy: 5,
                disruptions_allowed: 3,
                expected_pods: 5,
                desired_healthy: 2,
                ..Default::default()
            }),
        };

        let summary = extract_summary(&pdb);

        assert_eq!(summary.name, "web-pdb");
        assert_eq!(summary.namespace, "production");
        assert_eq!(summary.min_available.as_deref(), Some("2"));
        assert!(summary.max_unavailable.is_none());
        assert_eq!(summary.current_healthy, Some(5));
        assert_eq!(summary.disruptions_allowed, Some(3));
        assert_eq!(summary.expected_pods, Some(5));
        assert!(summary.created_at.is_some());
    }

    #[test]
    fn extract_summary_from_minimal_pdb() {
        let pdb = Pdb {
            metadata: ObjectMeta::default(),
            spec: None,
            status: None,
        };

        let summary = extract_summary(&pdb);

        assert_eq!(summary.name, "");
        assert_eq!(summary.namespace, "");
        assert!(summary.min_available.is_none());
        assert!(summary.max_unavailable.is_none());
        assert!(summary.current_healthy.is_none());
        assert!(summary.disruptions_allowed.is_none());
        assert!(summary.expected_pods.is_none());
        assert!(summary.created_at.is_none());
    }

    #[test]
    fn extract_detail_includes_selector_conditions_labels() {
        let pdb = Pdb {
            metadata: ObjectMeta {
                name: Some("detail-pdb".to_string()),
                namespace: Some("staging".to_string()),
                labels: Some({
                    let mut m = BTreeMap::new();
                    m.insert("team".to_string(), "platform".to_string());
                    m
                }),
                annotations: Some({
                    let mut m = BTreeMap::new();
                    m.insert("note".to_string(), "testing".to_string());
                    m
                }),
                ..Default::default()
            },
            spec: Some(PodDisruptionBudgetSpec {
                max_unavailable: Some(IntOrString::String("25%".to_string())),
                selector: Some(LabelSelector {
                    match_labels: Some({
                        let mut m = BTreeMap::new();
                        m.insert("app".to_string(), "api".to_string());
                        m
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            status: Some(k8s_openapi::api::policy::v1::PodDisruptionBudgetStatus {
                current_healthy: 4,
                disruptions_allowed: 1,
                expected_pods: 4,
                desired_healthy: 3,
                conditions: Some(vec![
                    k8s_openapi::apimachinery::pkg::apis::meta::v1::Condition {
                        type_: "DisruptionAllowed".to_string(),
                        status: "True".to_string(),
                        reason: "SufficientPods".to_string(),
                        message: "The disruption budget allows disruption".to_string(),
                        last_transition_time: k8s_openapi::apimachinery::pkg::apis::meta::v1::Time(
                            "2024-06-15T12:00:00Z"
                                .parse::<k8s_openapi::jiff::Timestamp>()
                                .unwrap(),
                        ),
                        observed_generation: None,
                    },
                ]),
                ..Default::default()
            }),
        };

        let detail = extract_detail(&pdb);

        assert_eq!(detail.name, "detail-pdb");
        assert_eq!(detail.namespace, "staging");
        assert_eq!(detail.max_unavailable.as_deref(), Some("25%"));
        assert!(detail.min_available.is_none());
        assert_eq!(
            detail.labels.get("team").map(|s| s.as_str()),
            Some("platform")
        );
        assert_eq!(
            detail.annotations.get("note").map(|s| s.as_str()),
            Some("testing")
        );
        assert_eq!(
            detail
                .selector
                .as_ref()
                .and_then(|s| s.get("app"))
                .map(|s| s.as_str()),
            Some("api")
        );
        assert_eq!(detail.conditions.len(), 1);
        assert_eq!(detail.conditions[0].condition_type, "DisruptionAllowed");
        assert_eq!(detail.conditions[0].status, "True");
        assert_eq!(detail.current_healthy, Some(4));
        assert_eq!(detail.disruptions_allowed, Some(1));
        assert_eq!(detail.expected_pods, Some(4));
    }

    #[test]
    fn parse_int_or_string_handles_integer() {
        let val = serde_json::json!(3);
        let result = parse_int_or_string(&val);
        assert!(matches!(result, Some(IntOrString::Int(3))));
    }

    #[test]
    fn parse_int_or_string_handles_string() {
        let val = serde_json::json!("50%");
        let result = parse_int_or_string(&val);
        assert!(matches!(result, Some(IntOrString::String(ref s)) if s == "50%"));
    }

    #[test]
    fn parse_int_or_string_handles_null() {
        let val = serde_json::Value::Null;
        let result = parse_int_or_string(&val);
        assert!(result.is_none());
    }
}
