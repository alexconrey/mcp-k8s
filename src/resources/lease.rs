use k8s_openapi::api::coordination::v1::Lease;
use kube::api::ListParams;
use serde::Serialize;

use crate::client::K8sClient;

fn api(client: &K8sClient, ns: &str) -> Result<kube::Api<Lease>, String> {
    if !client.is_namespace_allowed(ns) {
        return Err(format!("Namespace '{ns}' is not in the allowed list"));
    }
    Ok(kube::Api::namespaced(client.inner().clone(), ns))
}

#[derive(Serialize, Debug)]
pub struct LeaseSummary {
    pub name: String,
    pub namespace: String,
    pub holder_identity: Option<String>,
    pub lease_duration_seconds: Option<i32>,
    pub acquire_time: Option<String>,
    pub renew_time: Option<String>,
    pub created_at: Option<String>,
}

fn extract_summary(lease: &Lease) -> LeaseSummary {
    let meta = &lease.metadata;
    let spec = lease.spec.as_ref();

    LeaseSummary {
        name: meta.name.clone().unwrap_or_default(),
        namespace: meta.namespace.clone().unwrap_or_default(),
        holder_identity: spec.and_then(|s| s.holder_identity.clone()),
        lease_duration_seconds: spec.and_then(|s| s.lease_duration_seconds),
        acquire_time: spec
            .and_then(|s| s.acquire_time.as_ref())
            .map(|t| t.0.to_string()),
        renew_time: spec
            .and_then(|s| s.renew_time.as_ref())
            .map(|t| t.0.to_string()),
        created_at: meta.creation_timestamp.as_ref().map(|t| t.0.to_string()),
    }
}

pub fn tool_definitions() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "name": "list_leases",
            "description": "List leases in a namespace. Returns name, namespace, holder_identity, lease_duration_seconds, acquire_time, renew_time, and created_at.",
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
            "name": "get_lease",
            "description": "Get a lease by name. Returns name, namespace, holder_identity, lease_duration_seconds, acquire_time, renew_time, created_at, labels, annotations, and lease_transitions.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "Lease name" }
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
        "list_leases" => list_leases(client, args).await,
        "get_lease" => get_lease(client, args).await,
        _ => return None,
    };
    Some(result)
}

async fn list_leases(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let lease_api = api(client, ns)?;
    let label_selector = args["label_selector"].as_str();
    let mut lp = ListParams::default();
    if let Some(sel) = label_selector {
        lp = lp.labels(sel);
    }
    let list = lease_api.list(&lp).await.map_err(|e| e.to_string())?;

    let summaries: Vec<serde_json::Value> = list
        .iter()
        .map(|lease| {
            let s = extract_summary(lease);
            serde_json::json!({
                "name": s.name,
                "namespace": s.namespace,
                "holder_identity": s.holder_identity,
                "lease_duration_seconds": s.lease_duration_seconds,
                "acquire_time": s.acquire_time,
                "renew_time": s.renew_time,
                "created_at": s.created_at,
            })
        })
        .collect();

    serde_json::to_string_pretty(&summaries).map_err(|e| e.to_string())
}

async fn get_lease(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;
    let lease_api = api(client, ns)?;
    let lease = lease_api.get(name).await.map_err(|e| e.to_string())?;

    let summary = extract_summary(&lease);
    let meta = &lease.metadata;
    let lease_transitions = lease.spec.as_ref().and_then(|s| s.lease_transitions);

    let result = serde_json::json!({
        "name": summary.name,
        "namespace": summary.namespace,
        "holder_identity": summary.holder_identity,
        "lease_duration_seconds": summary.lease_duration_seconds,
        "acquire_time": summary.acquire_time,
        "renew_time": summary.renew_time,
        "created_at": summary.created_at,
        "labels": meta.labels.clone().unwrap_or_default(),
        "annotations": meta.annotations.clone().unwrap_or_default(),
        "lease_transitions": lease_transitions,
    });

    serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::api::coordination::v1::LeaseSpec;
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::{MicroTime, ObjectMeta, Time};
    use std::collections::BTreeMap;

    #[test]
    fn tool_definitions_returns_two_unique_tools() {
        let defs = tool_definitions();
        assert_eq!(defs.len(), 2);

        let names: Vec<&str> = defs.iter().map(|d| d["name"].as_str().unwrap()).collect();

        let mut unique = names.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(names.len(), unique.len(), "tool names must be unique");

        assert!(names.contains(&"list_leases"));
        assert!(names.contains(&"get_lease"));
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
    fn lease_summary_serialization() {
        let summary = LeaseSummary {
            name: "kube-scheduler".to_string(),
            namespace: "kube-system".to_string(),
            holder_identity: Some("master-1".to_string()),
            lease_duration_seconds: Some(15),
            acquire_time: Some("2024-01-01T00:00:00.000000Z".to_string()),
            renew_time: Some("2024-01-01T01:00:00.000000Z".to_string()),
            created_at: Some("2024-01-01T00:00:00Z".to_string()),
        };

        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["name"], "kube-scheduler");
        assert_eq!(json["namespace"], "kube-system");
        assert_eq!(json["holder_identity"], "master-1");
        assert_eq!(json["lease_duration_seconds"], 15);
        assert_eq!(json["acquire_time"], "2024-01-01T00:00:00.000000Z");
        assert_eq!(json["renew_time"], "2024-01-01T01:00:00.000000Z");
        assert_eq!(json["created_at"], "2024-01-01T00:00:00Z");
    }

    #[test]
    fn lease_summary_serialization_empty_fields() {
        let summary = LeaseSummary {
            name: "empty-lease".to_string(),
            namespace: "default".to_string(),
            holder_identity: None,
            lease_duration_seconds: None,
            acquire_time: None,
            renew_time: None,
            created_at: None,
        };

        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["name"], "empty-lease");
        assert_eq!(json["namespace"], "default");
        assert!(json["holder_identity"].is_null());
        assert!(json["lease_duration_seconds"].is_null());
        assert!(json["acquire_time"].is_null());
        assert!(json["renew_time"].is_null());
        assert!(json["created_at"].is_null());
    }

    #[test]
    fn extract_summary_from_lease() {
        let ts = k8s_openapi::jiff::Timestamp::from_second(1704067200).unwrap();
        let renew_ts = k8s_openapi::jiff::Timestamp::from_second(1704070800).unwrap();

        let lease = Lease {
            metadata: ObjectMeta {
                name: Some("kube-controller-manager".to_string()),
                namespace: Some("kube-system".to_string()),
                creation_timestamp: Some(Time(ts)),
                labels: Some(BTreeMap::from([(
                    "component".to_string(),
                    "kube-controller-manager".to_string(),
                )])),
                annotations: Some(BTreeMap::from([(
                    "control-plane.alpha.kubernetes.io/leader".to_string(),
                    "true".to_string(),
                )])),
                ..Default::default()
            },
            spec: Some(LeaseSpec {
                holder_identity: Some("master-1".to_string()),
                lease_duration_seconds: Some(15),
                acquire_time: Some(MicroTime(ts)),
                renew_time: Some(MicroTime(renew_ts)),
                lease_transitions: Some(3),
                ..Default::default()
            }),
        };

        let summary = extract_summary(&lease);
        assert_eq!(summary.name, "kube-controller-manager");
        assert_eq!(summary.namespace, "kube-system");
        assert_eq!(summary.holder_identity.as_deref(), Some("master-1"));
        assert_eq!(summary.lease_duration_seconds, Some(15));
        assert!(summary.acquire_time.is_some());
        assert!(summary.renew_time.is_some());
        assert!(summary.created_at.is_some());
    }

    #[test]
    fn extract_summary_from_empty_lease() {
        let lease = Lease {
            metadata: ObjectMeta {
                name: Some("empty-lease".to_string()),
                namespace: Some("default".to_string()),
                ..Default::default()
            },
            spec: None,
        };

        let summary = extract_summary(&lease);
        assert_eq!(summary.name, "empty-lease");
        assert_eq!(summary.namespace, "default");
        assert!(summary.holder_identity.is_none());
        assert!(summary.lease_duration_seconds.is_none());
        assert!(summary.acquire_time.is_none());
        assert!(summary.renew_time.is_none());
        assert!(summary.created_at.is_none());
    }
}
