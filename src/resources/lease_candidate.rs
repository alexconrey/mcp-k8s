use k8s_openapi::api::coordination::v1alpha2::LeaseCandidate;
use kube::api::ListParams;

use crate::client::K8sClient;

fn api(client: &K8sClient, ns: &str) -> Result<kube::Api<LeaseCandidate>, String> {
    if !client.is_namespace_allowed(ns) {
        return Err(format!("Namespace '{ns}' is not in the allowed list"));
    }
    Ok(kube::Api::namespaced(client.inner().clone(), ns))
}

pub fn tool_definitions() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "name": "list_leasecandidates",
            "description": "List lease candidates in a namespace. Returns name, namespace, lease_name, strategy, renew_time, and created_at for each candidate.",
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
            "name": "get_leasecandidate",
            "description": "Get a lease candidate by name. Returns name, namespace, lease_name, strategy, binary_version, emulation_version, ping_time, renew_time, created_at, labels, and annotations.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "LeaseCandidate name" }
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
        "list_leasecandidates" => list_leasecandidates(client, args).await,
        "get_leasecandidate" => get_leasecandidate(client, args).await,
        _ => return None,
    };
    Some(result)
}

async fn list_leasecandidates(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let lc_api = api(client, ns)?;
    let label_selector = args["label_selector"].as_str();
    let mut lp = ListParams::default();
    if let Some(sel) = label_selector {
        lp = lp.labels(sel);
    }
    let list = lc_api.list(&lp).await.map_err(|e| e.to_string())?;

    let summaries: Vec<serde_json::Value> = list
        .iter()
        .map(|lc| {
            let meta = &lc.metadata;
            let spec = lc.spec.as_ref();
            serde_json::json!({
                "name": meta.name.clone().unwrap_or_default(),
                "namespace": meta.namespace.clone().unwrap_or_default(),
                "lease_name": spec.map(|s| s.lease_name.as_str()).unwrap_or_default(),
                "strategy": spec.map(|s| s.strategy.as_str()).unwrap_or_default(),
                "renew_time": spec
                    .and_then(|s| s.renew_time.as_ref())
                    .map(|t| t.0.to_string()),
                "created_at": meta.creation_timestamp.as_ref().map(|t| t.0.to_string()),
            })
        })
        .collect();

    serde_json::to_string_pretty(&summaries).map_err(|e| e.to_string())
}

async fn get_leasecandidate(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;
    let lc_api = api(client, ns)?;
    let lc = lc_api.get(name).await.map_err(|e| e.to_string())?;

    let meta = &lc.metadata;
    let spec = lc.spec.as_ref();

    let result = serde_json::json!({
        "name": meta.name.clone().unwrap_or_default(),
        "namespace": meta.namespace.clone().unwrap_or_default(),
        "lease_name": spec.map(|s| s.lease_name.as_str()).unwrap_or_default(),
        "strategy": spec.map(|s| s.strategy.as_str()).unwrap_or_default(),
        "binary_version": spec.map(|s| s.binary_version.as_str()).unwrap_or_default(),
        "emulation_version": spec.and_then(|s| s.emulation_version.as_deref()),
        "ping_time": spec
            .and_then(|s| s.ping_time.as_ref())
            .map(|t| t.0.to_string()),
        "renew_time": spec
            .and_then(|s| s.renew_time.as_ref())
            .map(|t| t.0.to_string()),
        "created_at": meta.creation_timestamp.as_ref().map(|t| t.0.to_string()),
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

        assert!(names.contains(&"list_leasecandidates"));
        assert!(names.contains(&"get_leasecandidate"));
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
}
