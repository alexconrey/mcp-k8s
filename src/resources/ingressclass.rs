use std::collections::BTreeMap;

use k8s_openapi::api::networking::v1::{
    IngressClass, IngressClassParametersReference, IngressClassSpec,
};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::api::{DeleteParams, ListParams, PostParams};
use serde::Serialize;

use crate::client::K8sClient;

const DEFAULT_IC_ANNOTATION: &str = "ingressclass.kubernetes.io/is-default-class";

fn api(client: &K8sClient) -> kube::Api<IngressClass> {
    kube::Api::all(client.inner().clone())
}

#[derive(Serialize, Debug)]
pub struct IngressClassParametersRef {
    pub api_group: Option<String>,
    pub kind: String,
    pub name: String,
    pub namespace: Option<String>,
    pub scope: Option<String>,
}

#[derive(Serialize, Debug)]
pub struct IngressClassSummary {
    pub name: String,
    pub controller: Option<String>,
    pub is_default: bool,
    pub parameters: Option<IngressClassParametersRef>,
    pub created_at: Option<String>,
}

fn extract_summary(ic: &IngressClass) -> IngressClassSummary {
    let meta = &ic.metadata;
    let spec = ic.spec.as_ref();

    let controller = spec.and_then(|s| s.controller.clone());

    let is_default = meta
        .annotations
        .as_ref()
        .and_then(|a| a.get(DEFAULT_IC_ANNOTATION))
        .map(|v| v == "true")
        .unwrap_or(false);

    let parameters = spec.and_then(|s| {
        s.parameters.as_ref().map(|p| IngressClassParametersRef {
            api_group: p.api_group.clone(),
            kind: p.kind.clone(),
            name: p.name.clone(),
            namespace: p.namespace.clone(),
            scope: p.scope.clone(),
        })
    });

    let created_at = meta.creation_timestamp.as_ref().map(|t| t.0.to_string());

    IngressClassSummary {
        name: meta.name.clone().unwrap_or_default(),
        controller,
        is_default,
        parameters,
        created_at,
    }
}

pub fn tool_definitions() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "name": "list_ingressclasses",
            "description": "List all IngressClasses in the cluster. Returns name, controller, is_default, parameters, and created_at.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "required": [],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "get_ingressclass",
            "description": "Get an IngressClass by name. Returns name, controller, is_default, parameters, created_at, labels, and annotations.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "IngressClass name" }
                },
                "required": ["name"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "create_ingressclass",
            "description": "Create an IngressClass with the given controller and optional settings.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "IngressClass name" },
                    "controller": { "type": "string", "description": "Controller that implements this IngressClass (e.g. k8s.io/ingress-nginx)" },
                    "is_default": { "type": "boolean", "description": "Set as the default IngressClass in the cluster" },
                    "parameters_api_group": { "type": "string", "description": "API group for IngressClass parameters resource" },
                    "parameters_kind": { "type": "string", "description": "Kind of the IngressClass parameters resource" },
                    "parameters_name": { "type": "string", "description": "Name of the IngressClass parameters resource" },
                    "parameters_namespace": { "type": "string", "description": "Namespace of the IngressClass parameters resource" },
                    "parameters_scope": { "type": "string", "description": "Scope of the IngressClass parameters (Cluster or Namespace)" }
                },
                "required": ["name", "controller"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "update_ingressclass",
            "description": "Update an existing IngressClass. Fetches the current resource, applies changes, and replaces it.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "IngressClass name" },
                    "controller": { "type": "string", "description": "Controller that implements this IngressClass" },
                    "is_default": { "type": "boolean", "description": "Set as the default IngressClass in the cluster" },
                    "parameters_api_group": { "type": "string", "description": "API group for IngressClass parameters resource" },
                    "parameters_kind": { "type": "string", "description": "Kind of the IngressClass parameters resource" },
                    "parameters_name": { "type": "string", "description": "Name of the IngressClass parameters resource" },
                    "parameters_namespace": { "type": "string", "description": "Namespace of the IngressClass parameters resource" },
                    "parameters_scope": { "type": "string", "description": "Scope of the IngressClass parameters (Cluster or Namespace)" }
                },
                "required": ["name"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "delete_ingressclass",
            "description": "Delete an IngressClass by name.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "IngressClass name" }
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
        "list_ingressclasses" => list_ingressclasses(client).await,
        "get_ingressclass" => get_ingressclass(client, args).await,
        "create_ingressclass" => create_ingressclass(client, args).await,
        "update_ingressclass" => update_ingressclass(client, args).await,
        "delete_ingressclass" => delete_ingressclass(client, args).await,
        _ => return None,
    };
    Some(result)
}

async fn list_ingressclasses(client: &K8sClient) -> Result<String, String> {
    let ic_api = api(client);
    let list = ic_api
        .list(&ListParams::default())
        .await
        .map_err(|e| e.to_string())?;

    let summaries: Vec<IngressClassSummary> = list.iter().map(extract_summary).collect();

    serde_json::to_string_pretty(&summaries).map_err(|e| e.to_string())
}

async fn get_ingressclass(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let name = args["name"].as_str().ok_or("name is required")?;

    let ic_api = api(client);
    let ic = ic_api.get(name).await.map_err(|e| e.to_string())?;

    let summary = extract_summary(&ic);
    let meta = &ic.metadata;

    let result = serde_json::json!({
        "name": summary.name,
        "controller": summary.controller,
        "is_default": summary.is_default,
        "parameters": summary.parameters.map(|p| serde_json::json!({
            "api_group": p.api_group,
            "kind": p.kind,
            "name": p.name,
            "namespace": p.namespace,
            "scope": p.scope,
        })),
        "created_at": summary.created_at,
        "labels": meta.labels.clone().unwrap_or_default(),
        "annotations": meta.annotations.clone().unwrap_or_default(),
    });

    serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
}

fn build_parameters_ref(args: &serde_json::Value) -> Option<IngressClassParametersReference> {
    let kind = args
        .get("parameters_kind")
        .and_then(|v| v.as_str())
        .map(String::from);
    let name = args
        .get("parameters_name")
        .and_then(|v| v.as_str())
        .map(String::from);

    // kind and name are required for a valid parameters reference
    match (kind, name) {
        (Some(k), Some(n)) => Some(IngressClassParametersReference {
            api_group: args
                .get("parameters_api_group")
                .and_then(|v| v.as_str())
                .map(String::from),
            kind: k,
            name: n,
            namespace: args
                .get("parameters_namespace")
                .and_then(|v| v.as_str())
                .map(String::from),
            scope: args
                .get("parameters_scope")
                .and_then(|v| v.as_str())
                .map(String::from),
        }),
        _ => None,
    }
}

async fn create_ingressclass(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let name = args["name"].as_str().ok_or("name is required")?;
    let controller = args["controller"]
        .as_str()
        .ok_or("controller is required")?;

    let mut labels = BTreeMap::new();
    labels.insert(
        "app.kubernetes.io/managed-by".to_string(),
        "mcp-k8s".to_string(),
    );

    let mut annotations = BTreeMap::new();
    if let Some(is_default) = args.get("is_default").and_then(|v| v.as_bool()) {
        annotations.insert(DEFAULT_IC_ANNOTATION.to_string(), is_default.to_string());
    }

    let parameters = build_parameters_ref(args);

    let ic = IngressClass {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            labels: Some(labels),
            annotations: if annotations.is_empty() {
                None
            } else {
                Some(annotations)
            },
            ..Default::default()
        },
        spec: Some(IngressClassSpec {
            controller: Some(controller.to_string()),
            parameters,
        }),
    };

    let ic_api = api(client);
    let created = ic_api
        .create(&PostParams::default(), &ic)
        .await
        .map_err(|e| e.to_string())?;

    let summary = extract_summary(&created);
    serde_json::to_string_pretty(&summary).map_err(|e| e.to_string())
}

async fn update_ingressclass(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let name = args["name"].as_str().ok_or("name is required")?;

    let ic_api = api(client);
    let mut ic = ic_api.get(name).await.map_err(|e| e.to_string())?;

    // Update controller if provided
    if let Some(controller) = args.get("controller").and_then(|v| v.as_str()) {
        if let Some(ref mut spec) = ic.spec {
            spec.controller = Some(controller.to_string());
        } else {
            ic.spec = Some(IngressClassSpec {
                controller: Some(controller.to_string()),
                parameters: None,
            });
        }
    }

    // Update is_default annotation if provided
    if let Some(is_default) = args.get("is_default").and_then(|v| v.as_bool()) {
        let annotations = ic.metadata.annotations.get_or_insert_with(BTreeMap::new);
        annotations.insert(DEFAULT_IC_ANNOTATION.to_string(), is_default.to_string());
    }

    // Update parameters if any parameter field is provided
    if args.get("parameters_kind").is_some() || args.get("parameters_name").is_some() {
        let parameters = build_parameters_ref(args);
        if let Some(ref mut spec) = ic.spec {
            spec.parameters = parameters;
        } else {
            ic.spec = Some(IngressClassSpec {
                controller: None,
                parameters,
            });
        }
    }

    let updated = ic_api
        .replace(name, &PostParams::default(), &ic)
        .await
        .map_err(|e| e.to_string())?;

    let summary = extract_summary(&updated);
    serde_json::to_string_pretty(&summary).map_err(|e| e.to_string())
}

async fn delete_ingressclass(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let name = args["name"].as_str().ok_or("name is required")?;

    let ic_api = api(client);
    ic_api
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
    use k8s_openapi::api::networking::v1::{IngressClassParametersReference, IngressClassSpec};
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

        assert!(names.contains(&"list_ingressclasses"));
        assert!(names.contains(&"get_ingressclass"));
        assert!(names.contains(&"create_ingressclass"));
        assert!(names.contains(&"update_ingressclass"));
        assert!(names.contains(&"delete_ingressclass"));
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
    fn ingressclass_summary_serialization() {
        let summary = IngressClassSummary {
            name: "nginx".to_string(),
            controller: Some("k8s.io/ingress-nginx".to_string()),
            is_default: true,
            parameters: Some(IngressClassParametersRef {
                api_group: Some("example.com".to_string()),
                kind: "IngressParameters".to_string(),
                name: "nginx-params".to_string(),
                namespace: Some("ingress-nginx".to_string()),
                scope: Some("Namespace".to_string()),
            }),
            created_at: Some("2024-01-01T00:00:00Z".to_string()),
        };

        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["name"], "nginx");
        assert_eq!(json["controller"], "k8s.io/ingress-nginx");
        assert_eq!(json["is_default"], true);
        assert_eq!(json["parameters"]["api_group"], "example.com");
        assert_eq!(json["parameters"]["kind"], "IngressParameters");
        assert_eq!(json["parameters"]["name"], "nginx-params");
        assert_eq!(json["parameters"]["namespace"], "ingress-nginx");
        assert_eq!(json["parameters"]["scope"], "Namespace");
        assert_eq!(json["created_at"], "2024-01-01T00:00:00Z");
    }

    #[test]
    fn ingressclass_summary_serialization_minimal() {
        let summary = IngressClassSummary {
            name: "alb".to_string(),
            controller: None,
            is_default: false,
            parameters: None,
            created_at: None,
        };

        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["name"], "alb");
        assert!(json["controller"].is_null());
        assert_eq!(json["is_default"], false);
        assert!(json["parameters"].is_null());
        assert!(json["created_at"].is_null());
    }

    fn make_test_ingressclass() -> IngressClass {
        let mut annotations = BTreeMap::new();
        annotations.insert(
            "ingressclass.kubernetes.io/is-default-class".to_string(),
            "true".to_string(),
        );

        let mut labels = BTreeMap::new();
        labels.insert("app".to_string(), "nginx".to_string());

        IngressClass {
            metadata: ObjectMeta {
                name: Some("nginx".to_string()),
                labels: Some(labels),
                annotations: Some(annotations),
                ..Default::default()
            },
            spec: Some(IngressClassSpec {
                controller: Some("k8s.io/ingress-nginx".to_string()),
                parameters: Some(IngressClassParametersReference {
                    api_group: Some("example.com".to_string()),
                    kind: "IngressParameters".to_string(),
                    name: "nginx-params".to_string(),
                    namespace: Some("ingress-nginx".to_string()),
                    scope: Some("Namespace".to_string()),
                }),
            }),
        }
    }

    #[test]
    fn extract_summary_from_ingressclass() {
        let ic = make_test_ingressclass();
        let summary = extract_summary(&ic);

        assert_eq!(summary.name, "nginx");
        assert_eq!(summary.controller.as_deref(), Some("k8s.io/ingress-nginx"));
        assert!(summary.is_default);
        assert!(summary.parameters.is_some());

        let params = summary.parameters.unwrap();
        assert_eq!(params.api_group.as_deref(), Some("example.com"));
        assert_eq!(params.kind, "IngressParameters");
        assert_eq!(params.name, "nginx-params");
        assert_eq!(params.namespace.as_deref(), Some("ingress-nginx"));
        assert_eq!(params.scope.as_deref(), Some("Namespace"));
    }

    #[test]
    fn extract_summary_from_minimal_ingressclass() {
        let ic = IngressClass {
            metadata: ObjectMeta {
                name: Some("alb".to_string()),
                ..Default::default()
            },
            spec: None,
        };

        let summary = extract_summary(&ic);
        assert_eq!(summary.name, "alb");
        assert!(summary.controller.is_none());
        assert!(!summary.is_default);
        assert!(summary.parameters.is_none());
        assert!(summary.created_at.is_none());
    }

    #[test]
    fn extract_summary_non_default_class() {
        let mut annotations = BTreeMap::new();
        annotations.insert(
            "ingressclass.kubernetes.io/is-default-class".to_string(),
            "false".to_string(),
        );

        let ic = IngressClass {
            metadata: ObjectMeta {
                name: Some("traefik".to_string()),
                annotations: Some(annotations),
                ..Default::default()
            },
            spec: Some(IngressClassSpec {
                controller: Some("traefik.io/ingress-controller".to_string()),
                parameters: None,
            }),
        };

        let summary = extract_summary(&ic);
        assert_eq!(summary.name, "traefik");
        assert_eq!(
            summary.controller.as_deref(),
            Some("traefik.io/ingress-controller")
        );
        assert!(!summary.is_default);
        assert!(summary.parameters.is_none());
    }
}
