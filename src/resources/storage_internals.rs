use std::collections::BTreeMap;

use k8s_openapi::api::storage::v1::{
    CSIDriver, CSINode, CSIStorageCapacity, VolumeAttachment,
};
use kube::api::ListParams;
use serde::Serialize;

use crate::client::K8sClient;

// ---------------------------------------------------------------------------
// CSIDriver helpers
// ---------------------------------------------------------------------------

fn csidriver_api(client: &K8sClient) -> kube::Api<CSIDriver> {
    kube::Api::all(client.inner().clone())
}

#[derive(Serialize, Debug)]
pub struct CSIDriverSummary {
    pub name: String,
    pub attach_required: Option<bool>,
    pub pod_info_on_mount: Option<bool>,
    pub created_at: Option<String>,
}

#[derive(Serialize, Debug)]
pub struct CSIDriverDetail {
    pub name: String,
    pub attach_required: Option<bool>,
    pub pod_info_on_mount: Option<bool>,
    pub created_at: Option<String>,
    pub volume_lifecycle_modes: Vec<String>,
    pub fs_group_policy: Option<String>,
    pub labels: BTreeMap<String, String>,
    pub annotations: BTreeMap<String, String>,
}

fn extract_csidriver_summary(drv: &CSIDriver) -> CSIDriverSummary {
    let meta = &drv.metadata;
    let spec = &drv.spec;

    CSIDriverSummary {
        name: meta.name.clone().unwrap_or_default(),
        attach_required: spec.attach_required,
        pod_info_on_mount: spec.pod_info_on_mount,
        created_at: meta.creation_timestamp.as_ref().map(|t| t.0.to_string()),
    }
}

fn extract_csidriver_detail(drv: &CSIDriver) -> CSIDriverDetail {
    let meta = &drv.metadata;
    let spec = &drv.spec;

    let volume_lifecycle_modes = spec
        .volume_lifecycle_modes
        .as_ref()
        .cloned()
        .unwrap_or_default();

    let fs_group_policy = spec.fs_group_policy.clone();

    CSIDriverDetail {
        name: meta.name.clone().unwrap_or_default(),
        attach_required: spec.attach_required,
        pod_info_on_mount: spec.pod_info_on_mount,
        created_at: meta.creation_timestamp.as_ref().map(|t| t.0.to_string()),
        volume_lifecycle_modes,
        fs_group_policy,
        labels: meta.labels.clone().unwrap_or_default(),
        annotations: meta.annotations.clone().unwrap_or_default(),
    }
}

// ---------------------------------------------------------------------------
// CSINode helpers
// ---------------------------------------------------------------------------

fn csinode_api(client: &K8sClient) -> kube::Api<CSINode> {
    kube::Api::all(client.inner().clone())
}

#[derive(Serialize, Debug)]
pub struct CSINodeDriverInfo {
    pub name: String,
    pub node_id: String,
    pub allocatable_count: Option<i32>,
}

#[derive(Serialize, Debug)]
pub struct CSINodeSummary {
    pub name: String,
    pub drivers: Vec<CSINodeDriverInfo>,
}

fn extract_csinode_summary(node: &CSINode) -> CSINodeSummary {
    let meta = &node.metadata;
    let spec = &node.spec;

    let drivers = spec
        .drivers
        .iter()
        .map(|d| CSINodeDriverInfo {
            name: d.name.clone(),
            node_id: d.node_id.clone(),
            allocatable_count: d
                .allocatable
                .as_ref()
                .and_then(|a| a.count),
        })
        .collect();

    CSINodeSummary {
        name: meta.name.clone().unwrap_or_default(),
        drivers,
    }
}

// ---------------------------------------------------------------------------
// VolumeAttachment helpers
// ---------------------------------------------------------------------------

fn volumeattachment_api(client: &K8sClient) -> kube::Api<VolumeAttachment> {
    kube::Api::all(client.inner().clone())
}

#[derive(Serialize, Debug)]
pub struct VolumeAttachmentSummary {
    pub name: String,
    pub attacher: String,
    pub node_name: String,
    pub pv_name: Option<String>,
    pub attached: bool,
}

#[derive(Serialize, Debug)]
pub struct VolumeAttachmentDetail {
    pub name: String,
    pub attacher: String,
    pub node_name: String,
    pub pv_name: Option<String>,
    pub attached: bool,
    pub attach_error: Option<String>,
    pub detach_error: Option<String>,
}

fn extract_va_summary(va: &VolumeAttachment) -> VolumeAttachmentSummary {
    let meta = &va.metadata;
    let spec = &va.spec;

    let pv_name = spec
        .source
        .persistent_volume_name
        .clone();

    let attached = va
        .status
        .as_ref()
        .map(|s| s.attached)
        .unwrap_or(false);

    VolumeAttachmentSummary {
        name: meta.name.clone().unwrap_or_default(),
        attacher: spec.attacher.clone(),
        node_name: spec.node_name.clone(),
        pv_name,
        attached,
    }
}

fn extract_va_detail(va: &VolumeAttachment) -> VolumeAttachmentDetail {
    let meta = &va.metadata;
    let spec = &va.spec;

    let pv_name = spec
        .source
        .persistent_volume_name
        .clone();

    let status = va.status.as_ref();
    let attached = status.map(|s| s.attached).unwrap_or(false);

    let attach_error = status
        .and_then(|s| s.attach_error.as_ref())
        .and_then(|e| e.message.clone());

    let detach_error = status
        .and_then(|s| s.detach_error.as_ref())
        .and_then(|e| e.message.clone());

    VolumeAttachmentDetail {
        name: meta.name.clone().unwrap_or_default(),
        attacher: spec.attacher.clone(),
        node_name: spec.node_name.clone(),
        pv_name,
        attached,
        attach_error,
        detach_error,
    }
}

// ---------------------------------------------------------------------------
// CSIStorageCapacity helpers
// ---------------------------------------------------------------------------

fn csistoragecapacity_api(
    client: &K8sClient,
    ns: &str,
) -> Result<kube::Api<CSIStorageCapacity>, String> {
    if !client.is_namespace_allowed(ns) {
        return Err(format!("Namespace '{ns}' is not in the allowed list"));
    }
    Ok(kube::Api::namespaced(client.inner().clone(), ns))
}

fn csistoragecapacity_api_all(client: &K8sClient) -> kube::Api<CSIStorageCapacity> {
    kube::Api::all(client.inner().clone())
}

#[derive(Serialize, Debug)]
pub struct CSIStorageCapacitySummary {
    pub name: String,
    pub namespace: String,
    pub storage_class_name: String,
    pub capacity: Option<String>,
    pub maximum_volume_size: Option<String>,
}

fn extract_csicap_summary(cap: &CSIStorageCapacity) -> CSIStorageCapacitySummary {
    let meta = &cap.metadata;

    CSIStorageCapacitySummary {
        name: meta.name.clone().unwrap_or_default(),
        namespace: meta.namespace.clone().unwrap_or_default(),
        storage_class_name: cap.storage_class_name.clone(),
        capacity: cap.capacity.as_ref().map(|q| q.0.clone()),
        maximum_volume_size: cap.maximum_volume_size.as_ref().map(|q| q.0.clone()),
    }
}

// ---------------------------------------------------------------------------
// Tool definitions
// ---------------------------------------------------------------------------

pub fn tool_definitions() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "name": "list_csidrivers",
            "description": "List all CSIDrivers in the cluster. Returns name, attach_required, pod_info_on_mount, and created_at.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "required": [],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "get_csidriver",
            "description": "Get a CSIDriver by name. Returns name, attach_required, pod_info_on_mount, created_at, volume_lifecycle_modes, fs_group_policy, labels, and annotations.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "CSIDriver name" }
                },
                "required": ["name"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "list_csinodes",
            "description": "List all CSINodes in the cluster. Returns name and drivers (each with name, node_id, allocatable_count).",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "required": [],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "list_volumeattachments",
            "description": "List all VolumeAttachments in the cluster. Returns name, attacher, node_name, pv_name, and attached status.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "required": [],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "get_volumeattachment",
            "description": "Get a VolumeAttachment by name. Returns name, attacher, node_name, pv_name, attached status, attach_error, and detach_error.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "VolumeAttachment name" }
                },
                "required": ["name"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "list_csistoragecapacities",
            "description": "List CSIStorageCapacities. Optionally filter by namespace; if omitted, lists across all namespaces. Returns name, namespace, storage_class_name, capacity, and maximum_volume_size.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace (optional, omit for all namespaces)" }
                },
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
        "list_csidrivers" => list_csidrivers(client).await,
        "get_csidriver" => get_csidriver(client, args).await,
        "list_csinodes" => list_csinodes(client).await,
        "list_volumeattachments" => list_volumeattachments(client).await,
        "get_volumeattachment" => get_volumeattachment(client, args).await,
        "list_csistoragecapacities" => list_csistoragecapacities(client, args).await,
        _ => return None,
    };
    Some(result)
}

// ---------------------------------------------------------------------------
// Tool implementations
// ---------------------------------------------------------------------------

async fn list_csidrivers(client: &K8sClient) -> Result<String, String> {
    let api = csidriver_api(client);
    let list = api
        .list(&ListParams::default())
        .await
        .map_err(|e| e.to_string())?;

    let summaries: Vec<CSIDriverSummary> = list
        .iter()
        .map(extract_csidriver_summary)
        .collect();

    serde_json::to_string_pretty(&summaries).map_err(|e| e.to_string())
}

async fn get_csidriver(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let name = args["name"].as_str().ok_or("name is required")?;
    let api = csidriver_api(client);
    let drv = api.get(name).await.map_err(|e| e.to_string())?;
    let detail = extract_csidriver_detail(&drv);

    serde_json::to_string_pretty(&detail).map_err(|e| e.to_string())
}

async fn list_csinodes(client: &K8sClient) -> Result<String, String> {
    let api = csinode_api(client);
    let list = api
        .list(&ListParams::default())
        .await
        .map_err(|e| e.to_string())?;

    let summaries: Vec<CSINodeSummary> = list
        .iter()
        .map(extract_csinode_summary)
        .collect();

    serde_json::to_string_pretty(&summaries).map_err(|e| e.to_string())
}

async fn list_volumeattachments(client: &K8sClient) -> Result<String, String> {
    let api = volumeattachment_api(client);
    let list = api
        .list(&ListParams::default())
        .await
        .map_err(|e| e.to_string())?;

    let summaries: Vec<VolumeAttachmentSummary> = list
        .iter()
        .map(extract_va_summary)
        .collect();

    serde_json::to_string_pretty(&summaries).map_err(|e| e.to_string())
}

async fn get_volumeattachment(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let name = args["name"].as_str().ok_or("name is required")?;
    let api = volumeattachment_api(client);
    let va = api.get(name).await.map_err(|e| e.to_string())?;
    let detail = extract_va_detail(&va);

    serde_json::to_string_pretty(&detail).map_err(|e| e.to_string())
}

async fn list_csistoragecapacities(
    client: &K8sClient,
    args: &serde_json::Value,
) -> Result<String, String> {
    let list = if let Some(ns) = args.get("namespace").and_then(|v| v.as_str()) {
        let api = csistoragecapacity_api(client, ns)?;
        api.list(&ListParams::default())
            .await
            .map_err(|e| e.to_string())?
    } else {
        let api = csistoragecapacity_api_all(client);
        api.list(&ListParams::default())
            .await
            .map_err(|e| e.to_string())?
    };

    let summaries: Vec<CSIStorageCapacitySummary> = list
        .iter()
        .map(extract_csicap_summary)
        .collect();

    serde_json::to_string_pretty(&summaries).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::api::storage::v1::{
        CSIDriverSpec, CSINodeDriver, CSINodeSpec, VolumeAttachmentSource,
        VolumeAttachmentSpec, VolumeAttachmentStatus, VolumeError,
    };
    use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
    use std::collections::BTreeMap;

    #[test]
    fn tool_definitions_returns_six_tools() {
        let defs = tool_definitions();
        assert_eq!(defs.len(), 6);

        let names: Vec<&str> = defs
            .iter()
            .map(|d| d["name"].as_str().unwrap())
            .collect();

        let mut unique = names.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(names.len(), unique.len(), "tool names must be unique");

        assert!(names.contains(&"list_csidrivers"));
        assert!(names.contains(&"get_csidriver"));
        assert!(names.contains(&"list_csinodes"));
        assert!(names.contains(&"list_volumeattachments"));
        assert!(names.contains(&"get_volumeattachment"));
        assert!(names.contains(&"list_csistoragecapacities"));
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
    fn csidriver_summary_serialization() {
        let summary = CSIDriverSummary {
            name: "ebs.csi.aws.com".to_string(),
            attach_required: Some(true),
            pod_info_on_mount: Some(false),
            created_at: Some("2024-01-01T00:00:00Z".to_string()),
        };

        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["name"], "ebs.csi.aws.com");
        assert_eq!(json["attach_required"], true);
        assert_eq!(json["pod_info_on_mount"], false);
        assert_eq!(json["created_at"], "2024-01-01T00:00:00Z");
    }

    #[test]
    fn csidriver_detail_serialization() {
        let detail = CSIDriverDetail {
            name: "ebs.csi.aws.com".to_string(),
            attach_required: Some(true),
            pod_info_on_mount: Some(true),
            created_at: Some("2024-06-15T10:00:00Z".to_string()),
            volume_lifecycle_modes: vec![
                "Persistent".to_string(),
                "Ephemeral".to_string(),
            ],
            fs_group_policy: Some("File".to_string()),
            labels: BTreeMap::from([("app".to_string(), "ebs-csi".to_string())]),
            annotations: BTreeMap::new(),
        };

        let json = serde_json::to_value(&detail).unwrap();
        assert_eq!(json["name"], "ebs.csi.aws.com");
        assert_eq!(json["attach_required"], true);
        assert_eq!(json["pod_info_on_mount"], true);
        assert_eq!(json["volume_lifecycle_modes"][0], "Persistent");
        assert_eq!(json["volume_lifecycle_modes"][1], "Ephemeral");
        assert_eq!(json["fs_group_policy"], "File");
        assert_eq!(json["labels"]["app"], "ebs-csi");
    }

    #[test]
    fn extract_csidriver_summary_from_object() {
        let drv = CSIDriver {
            metadata: ObjectMeta {
                name: Some("efs.csi.aws.com".to_string()),
                ..Default::default()
            },
            spec: CSIDriverSpec {
                attach_required: Some(false),
                pod_info_on_mount: Some(true),
                volume_lifecycle_modes: Some(vec!["Persistent".to_string()]),
                fs_group_policy: Some("ReadWriteOnceWithFSType".to_string()),
                ..Default::default()
            },
        };

        let summary = extract_csidriver_summary(&drv);
        assert_eq!(summary.name, "efs.csi.aws.com");
        assert_eq!(summary.attach_required, Some(false));
        assert_eq!(summary.pod_info_on_mount, Some(true));
        assert!(summary.created_at.is_none());
    }

    #[test]
    fn extract_csidriver_detail_from_object() {
        let mut labels = BTreeMap::new();
        labels.insert("managed-by".to_string(), "helm".to_string());

        let drv = CSIDriver {
            metadata: ObjectMeta {
                name: Some("efs.csi.aws.com".to_string()),
                labels: Some(labels),
                ..Default::default()
            },
            spec: CSIDriverSpec {
                attach_required: Some(false),
                pod_info_on_mount: Some(true),
                volume_lifecycle_modes: Some(vec![
                    "Persistent".to_string(),
                    "Ephemeral".to_string(),
                ]),
                fs_group_policy: Some("File".to_string()),
                ..Default::default()
            },
        };

        let detail = extract_csidriver_detail(&drv);
        assert_eq!(detail.name, "efs.csi.aws.com");
        assert_eq!(detail.attach_required, Some(false));
        assert_eq!(detail.pod_info_on_mount, Some(true));
        assert_eq!(
            detail.volume_lifecycle_modes,
            vec!["Persistent", "Ephemeral"]
        );
        assert_eq!(detail.fs_group_policy.as_deref(), Some("File"));
        assert_eq!(detail.labels.get("managed-by"), Some(&"helm".to_string()));
    }

    #[test]
    fn extract_csidriver_detail_minimal() {
        let drv = CSIDriver {
            metadata: ObjectMeta {
                name: Some("minimal.csi".to_string()),
                ..Default::default()
            },
            spec: CSIDriverSpec::default(),
        };

        let detail = extract_csidriver_detail(&drv);
        assert_eq!(detail.name, "minimal.csi");
        assert!(detail.attach_required.is_none());
        assert!(detail.pod_info_on_mount.is_none());
        assert!(detail.volume_lifecycle_modes.is_empty());
        assert!(detail.fs_group_policy.is_none());
        assert!(detail.labels.is_empty());
        assert!(detail.annotations.is_empty());
    }

    #[test]
    fn csinode_summary_serialization() {
        let summary = CSINodeSummary {
            name: "node-1".to_string(),
            drivers: vec![
                CSINodeDriverInfo {
                    name: "ebs.csi.aws.com".to_string(),
                    node_id: "i-abc123".to_string(),
                    allocatable_count: Some(25),
                },
                CSINodeDriverInfo {
                    name: "efs.csi.aws.com".to_string(),
                    node_id: "i-abc123".to_string(),
                    allocatable_count: None,
                },
            ],
        };

        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["name"], "node-1");
        assert_eq!(json["drivers"].as_array().unwrap().len(), 2);
        assert_eq!(json["drivers"][0]["name"], "ebs.csi.aws.com");
        assert_eq!(json["drivers"][0]["node_id"], "i-abc123");
        assert_eq!(json["drivers"][0]["allocatable_count"], 25);
        assert_eq!(json["drivers"][1]["name"], "efs.csi.aws.com");
        assert!(json["drivers"][1]["allocatable_count"].is_null());
    }

    #[test]
    fn extract_csinode_summary_from_object() {
        let node = CSINode {
            metadata: ObjectMeta {
                name: Some("worker-1".to_string()),
                ..Default::default()
            },
            spec: CSINodeSpec {
                drivers: vec![CSINodeDriver {
                    name: "ebs.csi.aws.com".to_string(),
                    node_id: "i-0123456789abcdef0".to_string(),
                    allocatable: None,
                    topology_keys: None,
                }],
            },
        };

        let summary = extract_csinode_summary(&node);
        assert_eq!(summary.name, "worker-1");
        assert_eq!(summary.drivers.len(), 1);
        assert_eq!(summary.drivers[0].name, "ebs.csi.aws.com");
        assert_eq!(summary.drivers[0].node_id, "i-0123456789abcdef0");
        assert!(summary.drivers[0].allocatable_count.is_none());
    }

    #[test]
    fn volumeattachment_summary_serialization() {
        let summary = VolumeAttachmentSummary {
            name: "csi-att-123".to_string(),
            attacher: "ebs.csi.aws.com".to_string(),
            node_name: "node-1".to_string(),
            pv_name: Some("pvc-abc".to_string()),
            attached: true,
        };

        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["name"], "csi-att-123");
        assert_eq!(json["attacher"], "ebs.csi.aws.com");
        assert_eq!(json["node_name"], "node-1");
        assert_eq!(json["pv_name"], "pvc-abc");
        assert_eq!(json["attached"], true);
    }

    #[test]
    fn volumeattachment_detail_serialization() {
        let detail = VolumeAttachmentDetail {
            name: "csi-att-456".to_string(),
            attacher: "ebs.csi.aws.com".to_string(),
            node_name: "node-2".to_string(),
            pv_name: None,
            attached: false,
            attach_error: Some("volume not found".to_string()),
            detach_error: None,
        };

        let json = serde_json::to_value(&detail).unwrap();
        assert_eq!(json["name"], "csi-att-456");
        assert_eq!(json["attached"], false);
        assert_eq!(json["attach_error"], "volume not found");
        assert!(json["detach_error"].is_null());
        assert!(json["pv_name"].is_null());
    }

    #[test]
    fn extract_va_summary_from_object() {
        let va = VolumeAttachment {
            metadata: ObjectMeta {
                name: Some("csi-att-789".to_string()),
                ..Default::default()
            },
            spec: VolumeAttachmentSpec {
                attacher: "ebs.csi.aws.com".to_string(),
                node_name: "worker-3".to_string(),
                source: VolumeAttachmentSource {
                    persistent_volume_name: Some("pv-data-001".to_string()),
                    ..Default::default()
                },
            },
            status: Some(VolumeAttachmentStatus {
                attached: true,
                attach_error: None,
                detach_error: None,
                attachment_metadata: None,
            }),
        };

        let summary = extract_va_summary(&va);
        assert_eq!(summary.name, "csi-att-789");
        assert_eq!(summary.attacher, "ebs.csi.aws.com");
        assert_eq!(summary.node_name, "worker-3");
        assert_eq!(summary.pv_name.as_deref(), Some("pv-data-001"));
        assert!(summary.attached);
    }

    #[test]
    fn extract_va_detail_with_errors() {
        let va = VolumeAttachment {
            metadata: ObjectMeta {
                name: Some("csi-att-err".to_string()),
                ..Default::default()
            },
            spec: VolumeAttachmentSpec {
                attacher: "efs.csi.aws.com".to_string(),
                node_name: "worker-5".to_string(),
                source: VolumeAttachmentSource {
                    persistent_volume_name: None,
                    ..Default::default()
                },
            },
            status: Some(VolumeAttachmentStatus {
                attached: false,
                attach_error: Some(VolumeError {
                    message: Some("attach timeout".to_string()),
                    time: None,
                }),
                detach_error: Some(VolumeError {
                    message: Some("detach failed".to_string()),
                    time: None,
                }),
                attachment_metadata: None,
            }),
        };

        let detail = extract_va_detail(&va);
        assert_eq!(detail.name, "csi-att-err");
        assert!(!detail.attached);
        assert_eq!(detail.attach_error.as_deref(), Some("attach timeout"));
        assert_eq!(detail.detach_error.as_deref(), Some("detach failed"));
        assert!(detail.pv_name.is_none());
    }

    #[test]
    fn extract_va_detail_no_status() {
        let va = VolumeAttachment {
            metadata: ObjectMeta {
                name: Some("csi-att-nostatus".to_string()),
                ..Default::default()
            },
            spec: VolumeAttachmentSpec {
                attacher: "ebs.csi.aws.com".to_string(),
                node_name: "worker-1".to_string(),
                source: VolumeAttachmentSource {
                    persistent_volume_name: Some("pv-123".to_string()),
                    ..Default::default()
                },
            },
            status: None,
        };

        let detail = extract_va_detail(&va);
        assert!(!detail.attached);
        assert!(detail.attach_error.is_none());
        assert!(detail.detach_error.is_none());
    }

    #[test]
    fn csistoragecapacity_summary_serialization() {
        let summary = CSIStorageCapacitySummary {
            name: "csisc-abc".to_string(),
            namespace: "kube-system".to_string(),
            storage_class_name: "gp3".to_string(),
            capacity: Some("500Gi".to_string()),
            maximum_volume_size: Some("16Ti".to_string()),
        };

        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["name"], "csisc-abc");
        assert_eq!(json["namespace"], "kube-system");
        assert_eq!(json["storage_class_name"], "gp3");
        assert_eq!(json["capacity"], "500Gi");
        assert_eq!(json["maximum_volume_size"], "16Ti");
    }

    #[test]
    fn extract_csicap_summary_from_object() {
        let cap = CSIStorageCapacity {
            metadata: ObjectMeta {
                name: Some("cap-001".to_string()),
                namespace: Some("storage-ns".to_string()),
                ..Default::default()
            },
            storage_class_name: "ebs-sc".to_string(),
            capacity: Some(Quantity("1Ti".to_string())),
            maximum_volume_size: Some(Quantity("16Ti".to_string())),
            ..Default::default()
        };

        let summary = extract_csicap_summary(&cap);
        assert_eq!(summary.name, "cap-001");
        assert_eq!(summary.namespace, "storage-ns");
        assert_eq!(summary.storage_class_name, "ebs-sc");
        assert_eq!(summary.capacity.as_deref(), Some("1Ti"));
        assert_eq!(summary.maximum_volume_size.as_deref(), Some("16Ti"));
    }

    #[test]
    fn extract_csicap_summary_minimal() {
        let cap = CSIStorageCapacity {
            metadata: ObjectMeta {
                name: Some("cap-min".to_string()),
                namespace: Some("default".to_string()),
                ..Default::default()
            },
            storage_class_name: "standard".to_string(),
            capacity: None,
            maximum_volume_size: None,
            ..Default::default()
        };

        let summary = extract_csicap_summary(&cap);
        assert_eq!(summary.name, "cap-min");
        assert_eq!(summary.namespace, "default");
        assert_eq!(summary.storage_class_name, "standard");
        assert!(summary.capacity.is_none());
        assert!(summary.maximum_volume_size.is_none());
    }
}
