use k8s_openapi::api::resource::v1beta1::{
    DeviceClass, ResourceClaim, ResourceClaimTemplate, ResourceSlice,
};
use kube::api::ListParams;
use serde::Serialize;

use crate::client::K8sClient;

// --- Namespaced API helpers ---

fn claim_api(client: &K8sClient, ns: &str) -> Result<kube::Api<ResourceClaim>, String> {
    if !client.is_namespace_allowed(ns) {
        return Err(format!("Namespace '{ns}' is not in the allowed list"));
    }
    Ok(kube::Api::namespaced(client.inner().clone(), ns))
}

fn claim_template_api(
    client: &K8sClient,
    ns: &str,
) -> Result<kube::Api<ResourceClaimTemplate>, String> {
    if !client.is_namespace_allowed(ns) {
        return Err(format!("Namespace '{ns}' is not in the allowed list"));
    }
    Ok(kube::Api::namespaced(client.inner().clone(), ns))
}

// --- Cluster-scoped API helpers ---

fn slice_api(client: &K8sClient) -> kube::Api<ResourceSlice> {
    kube::Api::all(client.inner().clone())
}

fn device_class_api(client: &K8sClient) -> kube::Api<DeviceClass> {
    kube::Api::all(client.inner().clone())
}

// --- Summary structs ---

#[derive(Serialize, Debug)]
pub struct ResourceClaimSummary {
    pub name: String,
    pub namespace: String,
    pub device_class_name: Option<String>,
    pub allocated: bool,
    pub created_at: Option<String>,
}

fn extract_claim_summary(claim: &ResourceClaim) -> ResourceClaimSummary {
    let meta = &claim.metadata;

    // Extract the first device_class_name from spec.devices.requests
    let device_class_name = claim
        .spec
        .devices
        .as_ref()
        .and_then(|dc| dc.requests.as_ref())
        .and_then(|reqs| reqs.first())
        .map(|req| req.device_class_name.clone());

    let allocated = claim
        .status
        .as_ref()
        .and_then(|s| s.allocation.as_ref())
        .is_some();

    ResourceClaimSummary {
        name: meta.name.clone().unwrap_or_default(),
        namespace: meta.namespace.clone().unwrap_or_default(),
        device_class_name,
        allocated,
        created_at: meta.creation_timestamp.as_ref().map(|t| t.0.to_string()),
    }
}

#[derive(Serialize, Debug)]
pub struct ResourceClaimTemplateSummary {
    pub name: String,
    pub namespace: String,
    pub created_at: Option<String>,
}

fn extract_claim_template_summary(tpl: &ResourceClaimTemplate) -> ResourceClaimTemplateSummary {
    let meta = &tpl.metadata;
    ResourceClaimTemplateSummary {
        name: meta.name.clone().unwrap_or_default(),
        namespace: meta.namespace.clone().unwrap_or_default(),
        created_at: meta.creation_timestamp.as_ref().map(|t| t.0.to_string()),
    }
}

#[derive(Serialize, Debug)]
pub struct ResourceSliceSummary {
    pub name: String,
    pub driver_name: String,
    pub node_name: Option<String>,
    pub pool: String,
    pub created_at: Option<String>,
}

fn extract_slice_summary(slice: &ResourceSlice) -> ResourceSliceSummary {
    let meta = &slice.metadata;
    ResourceSliceSummary {
        name: meta.name.clone().unwrap_or_default(),
        driver_name: slice.spec.driver.clone(),
        node_name: slice.spec.node_name.clone(),
        pool: slice.spec.pool.name.clone(),
        created_at: meta.creation_timestamp.as_ref().map(|t| t.0.to_string()),
    }
}

#[derive(Serialize, Debug)]
pub struct DeviceClassSummary {
    pub name: String,
    pub created_at: Option<String>,
}

fn extract_device_class_summary(dc: &DeviceClass) -> DeviceClassSummary {
    let meta = &dc.metadata;
    DeviceClassSummary {
        name: meta.name.clone().unwrap_or_default(),
        created_at: meta.creation_timestamp.as_ref().map(|t| t.0.to_string()),
    }
}

// --- Tool definitions ---

pub fn tool_definitions() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "name": "list_resourceclaims",
            "description": "List ResourceClaims in a namespace (resource.k8s.io/v1beta1). Returns name, namespace, device_class_name, allocation status, and created_at.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" }
                },
                "required": ["namespace"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "get_resourceclaim",
            "description": "Get a ResourceClaim by name (resource.k8s.io/v1beta1). Returns name, namespace, device_class_name, allocation status, spec details, status, labels, and annotations.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" },
                    "name": { "type": "string", "description": "ResourceClaim name" }
                },
                "required": ["namespace", "name"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "list_resourceclaimtemplates",
            "description": "List ResourceClaimTemplates in a namespace (resource.k8s.io/v1beta1). Returns name, namespace, and created_at.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace" }
                },
                "required": ["namespace"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "list_resourceslices",
            "description": "List ResourceSlices cluster-wide (resource.k8s.io/v1beta1). Returns name, driver_name, node_name, pool, and created_at.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "required": [],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "list_deviceclasses",
            "description": "List DeviceClasses cluster-wide (resource.k8s.io/v1beta1). Returns name and created_at.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "required": [],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "get_deviceclass",
            "description": "Get a DeviceClass by name (resource.k8s.io/v1beta1). Returns spec details including selectors and config, plus labels and annotations.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "DeviceClass name" }
                },
                "required": ["name"],
                "additionalProperties": false
            }
        }),
    ]
}

// --- Tool handler dispatch ---

pub async fn handle_tool(
    client: &K8sClient,
    name: &str,
    args: &serde_json::Value,
) -> Option<Result<String, String>> {
    let result = match name {
        "list_resourceclaims" => list_resourceclaims(client, args).await,
        "get_resourceclaim" => get_resourceclaim(client, args).await,
        "list_resourceclaimtemplates" => list_resourceclaimtemplates(client, args).await,
        "list_resourceslices" => list_resourceslices(client).await,
        "list_deviceclasses" => list_deviceclasses(client).await,
        "get_deviceclass" => get_deviceclass(client, args).await,
        _ => return None,
    };
    Some(result)
}

// --- Tool implementations ---

async fn list_resourceclaims(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let api = claim_api(client, ns)?;
    let list = api
        .list(&ListParams::default())
        .await
        .map_err(|e| e.to_string())?;

    let summaries: Vec<ResourceClaimSummary> = list.iter().map(extract_claim_summary).collect();

    serde_json::to_string_pretty(&summaries).map_err(|e| e.to_string())
}

async fn get_resourceclaim(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let name = args["name"].as_str().ok_or("name is required")?;
    let api = claim_api(client, ns)?;
    let claim = api.get(name).await.map_err(|e| e.to_string())?;

    let summary = extract_claim_summary(&claim);
    let meta = &claim.metadata;

    let spec_json = serde_json::to_value(&claim.spec).unwrap_or_default();
    let status_json = claim
        .status
        .as_ref()
        .map(|s| serde_json::to_value(s).unwrap_or_default());

    let result = serde_json::json!({
        "name": summary.name,
        "namespace": summary.namespace,
        "device_class_name": summary.device_class_name,
        "allocated": summary.allocated,
        "created_at": summary.created_at,
        "spec": spec_json,
        "status": status_json,
        "labels": meta.labels.clone().unwrap_or_default(),
        "annotations": meta.annotations.clone().unwrap_or_default(),
    });

    serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
}

async fn list_resourceclaimtemplates(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let ns = args["namespace"].as_str().ok_or("namespace is required")?;
    let api = claim_template_api(client, ns)?;
    let list = api
        .list(&ListParams::default())
        .await
        .map_err(|e| e.to_string())?;

    let summaries: Vec<ResourceClaimTemplateSummary> =
        list.iter().map(extract_claim_template_summary).collect();

    serde_json::to_string_pretty(&summaries).map_err(|e| e.to_string())
}

async fn list_resourceslices(client: &K8sClient) -> Result<String, String> {
    let api = slice_api(client);
    let list = api
        .list(&ListParams::default())
        .await
        .map_err(|e| e.to_string())?;

    let summaries: Vec<ResourceSliceSummary> = list.iter().map(extract_slice_summary).collect();

    serde_json::to_string_pretty(&summaries).map_err(|e| e.to_string())
}

async fn list_deviceclasses(client: &K8sClient) -> Result<String, String> {
    let api = device_class_api(client);
    let list = api
        .list(&ListParams::default())
        .await
        .map_err(|e| e.to_string())?;

    let summaries: Vec<DeviceClassSummary> =
        list.iter().map(extract_device_class_summary).collect();

    serde_json::to_string_pretty(&summaries).map_err(|e| e.to_string())
}

async fn get_deviceclass(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let name = args["name"].as_str().ok_or("name is required")?;
    let api = device_class_api(client);
    let dc = api.get(name).await.map_err(|e| e.to_string())?;

    let meta = &dc.metadata;
    let spec_json = serde_json::to_value(&dc.spec).unwrap_or_default();

    let result = serde_json::json!({
        "name": meta.name.clone().unwrap_or_default(),
        "created_at": meta.creation_timestamp.as_ref().map(|t| t.0.to_string()),
        "spec": spec_json,
        "labels": meta.labels.clone().unwrap_or_default(),
        "annotations": meta.annotations.clone().unwrap_or_default(),
    });

    serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_definitions_returns_six_tools() {
        let defs = tool_definitions();
        assert_eq!(defs.len(), 6);

        let names: Vec<&str> = defs.iter().map(|d| d["name"].as_str().unwrap()).collect();

        let mut unique = names.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(names.len(), unique.len(), "tool names must be unique");

        assert!(names.contains(&"list_resourceclaims"));
        assert!(names.contains(&"get_resourceclaim"));
        assert!(names.contains(&"list_resourceclaimtemplates"));
        assert!(names.contains(&"list_resourceslices"));
        assert!(names.contains(&"list_deviceclasses"));
        assert!(names.contains(&"get_deviceclass"));
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
    fn resource_claim_summary_serialization() {
        let summary = ResourceClaimSummary {
            name: "gpu-claim".to_string(),
            namespace: "default".to_string(),
            device_class_name: Some("gpu.nvidia.com".to_string()),
            allocated: true,
            created_at: Some("2024-01-01T00:00:00Z".to_string()),
        };

        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["name"], "gpu-claim");
        assert_eq!(json["namespace"], "default");
        assert_eq!(json["device_class_name"], "gpu.nvidia.com");
        assert_eq!(json["allocated"], true);
        assert_eq!(json["created_at"], "2024-01-01T00:00:00Z");
    }

    #[test]
    fn resource_claim_summary_serialization_empty() {
        let summary = ResourceClaimSummary {
            name: "empty".to_string(),
            namespace: "ns".to_string(),
            device_class_name: None,
            allocated: false,
            created_at: None,
        };

        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["name"], "empty");
        assert!(json["device_class_name"].is_null());
        assert_eq!(json["allocated"], false);
        assert!(json["created_at"].is_null());
    }

    #[test]
    fn resource_claim_template_summary_serialization() {
        let summary = ResourceClaimTemplateSummary {
            name: "my-template".to_string(),
            namespace: "default".to_string(),
            created_at: Some("2024-06-01T12:00:00Z".to_string()),
        };

        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["name"], "my-template");
        assert_eq!(json["namespace"], "default");
        assert_eq!(json["created_at"], "2024-06-01T12:00:00Z");
    }

    #[test]
    fn resource_slice_summary_serialization() {
        let summary = ResourceSliceSummary {
            name: "node1-gpu-slice".to_string(),
            driver_name: "gpu.nvidia.com".to_string(),
            node_name: Some("node1".to_string()),
            pool: "node1".to_string(),
            created_at: Some("2024-01-01T00:00:00Z".to_string()),
        };

        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["name"], "node1-gpu-slice");
        assert_eq!(json["driver_name"], "gpu.nvidia.com");
        assert_eq!(json["node_name"], "node1");
        assert_eq!(json["pool"], "node1");
        assert_eq!(json["created_at"], "2024-01-01T00:00:00Z");
    }

    #[test]
    fn resource_slice_summary_no_node() {
        let summary = ResourceSliceSummary {
            name: "shared-slice".to_string(),
            driver_name: "net.example.com".to_string(),
            node_name: None,
            pool: "shared-pool".to_string(),
            created_at: None,
        };

        let json = serde_json::to_value(&summary).unwrap();
        assert!(json["node_name"].is_null());
        assert!(json["created_at"].is_null());
    }

    #[test]
    fn device_class_summary_serialization() {
        let summary = DeviceClassSummary {
            name: "gpu.nvidia.com".to_string(),
            created_at: Some("2024-01-01T00:00:00Z".to_string()),
        };

        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["name"], "gpu.nvidia.com");
        assert_eq!(json["created_at"], "2024-01-01T00:00:00Z");
    }

    #[test]
    fn extract_claim_summary_from_resource_claim() {
        use k8s_openapi::api::resource::v1beta1::{
            AllocationResult, DeviceClaim, DeviceRequest, ResourceClaimSpec, ResourceClaimStatus,
        };
        use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

        let claim = ResourceClaim {
            metadata: ObjectMeta {
                name: Some("test-claim".to_string()),
                namespace: Some("prod".to_string()),
                ..Default::default()
            },
            spec: ResourceClaimSpec {
                devices: Some(DeviceClaim {
                    requests: Some(vec![DeviceRequest {
                        device_class_name: "gpu.nvidia.com".to_string(),
                        name: "req1".to_string(),
                        ..Default::default()
                    }]),
                    ..Default::default()
                }),
            },
            status: Some(ResourceClaimStatus {
                allocation: Some(AllocationResult {
                    ..Default::default()
                }),
                ..Default::default()
            }),
        };

        let summary = extract_claim_summary(&claim);
        assert_eq!(summary.name, "test-claim");
        assert_eq!(summary.namespace, "prod");
        assert_eq!(summary.device_class_name.as_deref(), Some("gpu.nvidia.com"));
        assert!(summary.allocated);
    }

    #[test]
    fn extract_claim_summary_no_allocation() {
        use k8s_openapi::api::resource::v1beta1::ResourceClaimSpec;
        use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

        let claim = ResourceClaim {
            metadata: ObjectMeta {
                name: Some("pending-claim".to_string()),
                namespace: Some("default".to_string()),
                ..Default::default()
            },
            spec: ResourceClaimSpec { devices: None },
            status: None,
        };

        let summary = extract_claim_summary(&claim);
        assert_eq!(summary.name, "pending-claim");
        assert!(summary.device_class_name.is_none());
        assert!(!summary.allocated);
    }

    #[test]
    fn extract_slice_summary_from_resource_slice() {
        use k8s_openapi::api::resource::v1beta1::{ResourcePool, ResourceSliceSpec};
        use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

        let slice = ResourceSlice {
            metadata: ObjectMeta {
                name: Some("node1-slice-0".to_string()),
                ..Default::default()
            },
            spec: ResourceSliceSpec {
                driver: "gpu.nvidia.com".to_string(),
                node_name: Some("node1".to_string()),
                pool: ResourcePool {
                    name: "node1-pool".to_string(),
                    generation: 1,
                    resource_slice_count: 1,
                },
                ..Default::default()
            },
        };

        let summary = extract_slice_summary(&slice);
        assert_eq!(summary.name, "node1-slice-0");
        assert_eq!(summary.driver_name, "gpu.nvidia.com");
        assert_eq!(summary.node_name.as_deref(), Some("node1"));
        assert_eq!(summary.pool, "node1-pool");
    }

    #[test]
    fn extract_device_class_summary_from_device_class() {
        use k8s_openapi::api::resource::v1beta1::DeviceClassSpec;
        use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

        let dc = DeviceClass {
            metadata: ObjectMeta {
                name: Some("gpu.nvidia.com".to_string()),
                ..Default::default()
            },
            spec: DeviceClassSpec {
                ..Default::default()
            },
        };

        let summary = extract_device_class_summary(&dc);
        assert_eq!(summary.name, "gpu.nvidia.com");
        assert!(summary.created_at.is_none());
    }

    #[test]
    fn extract_claim_template_summary_from_template() {
        use k8s_openapi::api::resource::v1beta1::{ResourceClaimSpec, ResourceClaimTemplateSpec};
        use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

        let tpl = ResourceClaimTemplate {
            metadata: ObjectMeta {
                name: Some("tpl-1".to_string()),
                namespace: Some("default".to_string()),
                ..Default::default()
            },
            spec: ResourceClaimTemplateSpec {
                metadata: None,
                spec: ResourceClaimSpec { devices: None },
            },
        };

        let summary = extract_claim_template_summary(&tpl);
        assert_eq!(summary.name, "tpl-1");
        assert_eq!(summary.namespace, "default");
        assert!(summary.created_at.is_none());
    }
}
