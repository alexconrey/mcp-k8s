use k8s_openapi::api::certificates::v1::CertificateSigningRequest;
use kube::api::{ListParams, Patch, PatchParams};
use serde::Serialize;

use crate::client::K8sClient;

fn api(client: &K8sClient) -> kube::Api<CertificateSigningRequest> {
    kube::Api::all(client.inner().clone())
}

#[derive(Serialize, Debug, Clone)]
pub struct CsrConditionSummary {
    pub condition_type: String,
    pub status: String,
    pub reason: Option<String>,
    pub message: Option<String>,
    pub last_update_time: Option<String>,
}

#[derive(Serialize, Debug, Clone)]
pub struct CsrSummary {
    pub name: String,
    pub signer_name: String,
    pub requestor: Option<String>,
    pub status: String,
    pub created_at: Option<String>,
}

fn csr_status_label(csr: &CertificateSigningRequest) -> String {
    let conditions = csr.status.as_ref().and_then(|s| s.conditions.as_ref());

    match conditions {
        Some(conds) => {
            for c in conds {
                if c.type_ == "Approved" && c.status == "True" {
                    return "Approved".to_string();
                }
                if c.type_ == "Denied" && c.status == "True" {
                    return "Denied".to_string();
                }
            }
            "Pending".to_string()
        }
        None => "Pending".to_string(),
    }
}

fn extract_summary(csr: &CertificateSigningRequest) -> CsrSummary {
    let meta = &csr.metadata;
    let spec = &csr.spec;

    CsrSummary {
        name: meta.name.clone().unwrap_or_default(),
        signer_name: spec.signer_name.clone(),
        requestor: spec.username.clone(),
        status: csr_status_label(csr),
        created_at: meta.creation_timestamp.as_ref().map(|t| t.0.to_string()),
    }
}

pub fn tool_definitions() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "name": "list_csrs",
            "description": "List all CertificateSigningRequests. Returns name, signer_name, requestor, status (Pending/Approved/Denied), and created_at.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "required": [],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "get_csr",
            "description": "Get a CertificateSigningRequest by name. Returns detailed info including conditions, usages, labels, and annotations.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "CertificateSigningRequest name" }
                },
                "required": ["name"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "approve_csr",
            "description": "Approve a pending CertificateSigningRequest by adding an Approved condition.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "CertificateSigningRequest name" },
                    "message": { "type": "string", "description": "Optional approval message" }
                },
                "required": ["name"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "deny_csr",
            "description": "Deny a pending CertificateSigningRequest by adding a Denied condition.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "CertificateSigningRequest name" },
                    "message": { "type": "string", "description": "Optional denial message" }
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
        "list_csrs" => list_csrs(client).await,
        "get_csr" => get_csr(client, args).await,
        "approve_csr" => approve_csr(client, args).await,
        "deny_csr" => deny_csr(client, args).await,
        _ => return None,
    };
    Some(result)
}

async fn list_csrs(client: &K8sClient) -> Result<String, String> {
    let csr_api = api(client);
    let list = csr_api
        .list(&ListParams::default())
        .await
        .map_err(|e| e.to_string())?;

    let summaries: Vec<CsrSummary> = list.iter().map(|csr| extract_summary(csr)).collect();

    serde_json::to_string_pretty(&summaries).map_err(|e| e.to_string())
}

async fn get_csr(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let name = args["name"].as_str().ok_or("name is required")?;
    let csr_api = api(client);
    let csr = csr_api.get(name).await.map_err(|e| e.to_string())?;

    let summary = extract_summary(&csr);
    let meta = &csr.metadata;
    let spec = &csr.spec;

    let conditions: Vec<CsrConditionSummary> = csr
        .status
        .as_ref()
        .and_then(|s| s.conditions.as_ref())
        .map(|conds| {
            conds
                .iter()
                .map(|c| CsrConditionSummary {
                    condition_type: c.type_.clone(),
                    status: c.status.clone(),
                    reason: c.reason.clone(),
                    message: c.message.clone(),
                    last_update_time: c.last_update_time.as_ref().map(|t| t.0.to_string()),
                })
                .collect()
        })
        .unwrap_or_default();

    let usages: Vec<String> = spec.usages.clone().unwrap_or_default();

    let result = serde_json::json!({
        "name": summary.name,
        "signer_name": summary.signer_name,
        "requestor": summary.requestor,
        "status": summary.status,
        "created_at": summary.created_at,
        "conditions": conditions,
        "usages": usages,
        "labels": meta.labels.clone().unwrap_or_default(),
        "annotations": meta.annotations.clone().unwrap_or_default(),
    });

    serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
}

async fn approve_csr(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let name = args["name"].as_str().ok_or("name is required")?;
    let message = args
        .get("message")
        .and_then(|v| v.as_str())
        .unwrap_or("Approved via mcp-k8s");

    let csr_api = api(client);

    // Verify CSR exists and is still pending
    let csr = csr_api.get(name).await.map_err(|e| e.to_string())?;
    let current_status = csr_status_label(&csr);
    if current_status != "Pending" {
        return Err(format!("CSR '{name}' is already {current_status}"));
    }

    let patch = serde_json::json!({
        "status": {
            "conditions": [{
                "type": "Approved",
                "status": "True",
                "reason": "ApprovedViaMcpK8s",
                "message": message,
            }]
        }
    });

    csr_api
        .patch_approval(name, &PatchParams::apply("mcp-k8s"), &Patch::Merge(&patch))
        .await
        .map_err(|e| e.to_string())?;

    let updated = csr_api.get(name).await.map_err(|e| e.to_string())?;
    let summary = extract_summary(&updated);
    serde_json::to_string_pretty(&summary).map_err(|e| e.to_string())
}

async fn deny_csr(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let name = args["name"].as_str().ok_or("name is required")?;
    let message = args
        .get("message")
        .and_then(|v| v.as_str())
        .unwrap_or("Denied via mcp-k8s");

    let csr_api = api(client);

    // Verify CSR exists and is still pending
    let csr = csr_api.get(name).await.map_err(|e| e.to_string())?;
    let current_status = csr_status_label(&csr);
    if current_status != "Pending" {
        return Err(format!("CSR '{name}' is already {current_status}"));
    }

    let patch = serde_json::json!({
        "status": {
            "conditions": [{
                "type": "Denied",
                "status": "True",
                "reason": "DeniedViaMcpK8s",
                "message": message,
            }]
        }
    });

    csr_api
        .patch_approval(name, &PatchParams::apply("mcp-k8s"), &Patch::Merge(&patch))
        .await
        .map_err(|e| e.to_string())?;

    let updated = csr_api.get(name).await.map_err(|e| e.to_string())?;
    let summary = extract_summary(&updated);
    serde_json::to_string_pretty(&summary).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::api::certificates::v1::{
        CertificateSigningRequestCondition, CertificateSigningRequestSpec,
        CertificateSigningRequestStatus,
    };
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
    use k8s_openapi::ByteString;
    use std::collections::BTreeMap;

    #[test]
    fn tool_definitions_returns_four_tools() {
        let defs = tool_definitions();
        assert_eq!(defs.len(), 4);

        let names: Vec<&str> = defs.iter().map(|d| d["name"].as_str().unwrap()).collect();

        let mut unique = names.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(names.len(), unique.len(), "tool names must be unique");

        assert!(names.contains(&"list_csrs"));
        assert!(names.contains(&"get_csr"));
        assert!(names.contains(&"approve_csr"));
        assert!(names.contains(&"deny_csr"));
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
    fn csr_summary_serialization() {
        let summary = CsrSummary {
            name: "my-csr".to_string(),
            signer_name: "kubernetes.io/kube-apiserver-client".to_string(),
            requestor: Some("system:admin".to_string()),
            status: "Pending".to_string(),
            created_at: Some("2024-01-01T00:00:00Z".to_string()),
        };

        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["name"], "my-csr");
        assert_eq!(json["signer_name"], "kubernetes.io/kube-apiserver-client");
        assert_eq!(json["requestor"], "system:admin");
        assert_eq!(json["status"], "Pending");
        assert_eq!(json["created_at"], "2024-01-01T00:00:00Z");
    }

    #[test]
    fn csr_summary_serialization_empty_fields() {
        let summary = CsrSummary {
            name: "csr-minimal".to_string(),
            signer_name: String::new(),
            requestor: None,
            status: "Pending".to_string(),
            created_at: None,
        };

        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["name"], "csr-minimal");
        assert_eq!(json["signer_name"], "");
        assert!(json["requestor"].is_null());
        assert_eq!(json["status"], "Pending");
        assert!(json["created_at"].is_null());
    }

    fn make_test_csr(name: &str, condition_type: Option<&str>) -> CertificateSigningRequest {
        let conditions = condition_type.map(|ct| {
            vec![CertificateSigningRequestCondition {
                type_: ct.to_string(),
                status: "True".to_string(),
                reason: Some(format!("{ct}ViaMcpK8s")),
                message: Some(format!("{ct} via mcp-k8s")),
                last_update_time: None,
                last_transition_time: None,
            }]
        });

        let mut labels = BTreeMap::new();
        labels.insert("app".to_string(), "test".to_string());

        let mut annotations = BTreeMap::new();
        annotations.insert("note".to_string(), "testing".to_string());

        CertificateSigningRequest {
            metadata: ObjectMeta {
                name: Some(name.to_string()),
                labels: Some(labels),
                annotations: Some(annotations),
                ..Default::default()
            },
            spec: CertificateSigningRequestSpec {
                signer_name: "kubernetes.io/kube-apiserver-client".to_string(),
                username: Some("system:node:test-node".to_string()),
                usages: Some(vec![
                    "client auth".to_string(),
                    "digital signature".to_string(),
                ]),
                request: ByteString(vec![]),
                ..Default::default()
            },
            status: Some(CertificateSigningRequestStatus {
                conditions,
                certificate: None,
            }),
        }
    }

    #[test]
    fn extract_summary_pending_csr() {
        let csr = make_test_csr("pending-csr", None);
        let summary = extract_summary(&csr);

        assert_eq!(summary.name, "pending-csr");
        assert_eq!(summary.signer_name, "kubernetes.io/kube-apiserver-client");
        assert_eq!(summary.requestor.as_deref(), Some("system:node:test-node"));
        assert_eq!(summary.status, "Pending");
    }

    #[test]
    fn extract_summary_approved_csr() {
        let csr = make_test_csr("approved-csr", Some("Approved"));
        let summary = extract_summary(&csr);

        assert_eq!(summary.name, "approved-csr");
        assert_eq!(summary.status, "Approved");
    }

    #[test]
    fn extract_summary_denied_csr() {
        let csr = make_test_csr("denied-csr", Some("Denied"));
        let summary = extract_summary(&csr);

        assert_eq!(summary.name, "denied-csr");
        assert_eq!(summary.status, "Denied");
    }

    #[test]
    fn extract_summary_default_spec() {
        let csr = CertificateSigningRequest {
            metadata: ObjectMeta {
                name: Some("default-spec-csr".to_string()),
                ..Default::default()
            },
            spec: CertificateSigningRequestSpec::default(),
            status: None,
        };

        let summary = extract_summary(&csr);
        assert_eq!(summary.name, "default-spec-csr");
        assert_eq!(summary.signer_name, "");
        assert!(summary.requestor.is_none());
        assert_eq!(summary.status, "Pending");
        assert!(summary.created_at.is_none());
    }

    #[test]
    fn csr_condition_summary_serialization() {
        let condition = CsrConditionSummary {
            condition_type: "Approved".to_string(),
            status: "True".to_string(),
            reason: Some("ApprovedViaMcpK8s".to_string()),
            message: Some("Approved via mcp-k8s".to_string()),
            last_update_time: Some("2024-06-15T10:30:00Z".to_string()),
        };

        let json = serde_json::to_value(&condition).unwrap();
        assert_eq!(json["condition_type"], "Approved");
        assert_eq!(json["status"], "True");
        assert_eq!(json["reason"], "ApprovedViaMcpK8s");
        assert_eq!(json["message"], "Approved via mcp-k8s");
        assert_eq!(json["last_update_time"], "2024-06-15T10:30:00Z");
    }

    #[test]
    fn csr_status_label_with_no_conditions() {
        let csr = CertificateSigningRequest {
            metadata: ObjectMeta::default(),
            spec: CertificateSigningRequestSpec::default(),
            status: Some(CertificateSigningRequestStatus {
                conditions: None,
                certificate: None,
            }),
        };
        assert_eq!(csr_status_label(&csr), "Pending");
    }

    #[test]
    fn csr_status_label_with_empty_conditions() {
        let csr = CertificateSigningRequest {
            metadata: ObjectMeta::default(),
            spec: CertificateSigningRequestSpec::default(),
            status: Some(CertificateSigningRequestStatus {
                conditions: Some(vec![]),
                certificate: None,
            }),
        };
        assert_eq!(csr_status_label(&csr), "Pending");
    }
}
