use std::collections::BTreeMap;

use k8s_openapi::api::core::v1::{Node, Pod};
use kube::api::{DeleteParams, ListParams, Patch, PatchParams};
use serde::{Deserialize, Serialize};

use crate::client::K8sClient;
use crate::extract::node_summary;
use crate::types::NodeConditionSummary;

fn api(client: &K8sClient) -> kube::Api<Node> {
    kube::Api::all(client.inner().clone())
}

#[derive(Serialize, Debug)]
pub struct NodeAddress {
    pub address_type: String,
    pub address: String,
}

#[derive(Serialize, Debug)]
pub struct NodeTaint {
    pub key: String,
    pub value: Option<String>,
    pub effect: String,
}

#[derive(Serialize, Debug)]
pub struct NodeDetail {
    pub name: String,
    pub status: String,
    pub roles: Vec<String>,
    pub cpu_capacity: Option<String>,
    pub memory_capacity: Option<String>,
    pub cpu_allocatable: Option<String>,
    pub memory_allocatable: Option<String>,
    pub os_image: Option<String>,
    pub kernel_version: Option<String>,
    pub kubelet_version: Option<String>,
    pub conditions: Vec<NodeConditionSummary>,
    pub addresses: Vec<NodeAddress>,
    pub taints: Vec<NodeTaint>,
    pub labels: BTreeMap<String, String>,
    pub annotations: BTreeMap<String, String>,
    pub created_at: Option<String>,
}

fn node_detail(node: &Node) -> NodeDetail {
    let summary = node_summary(node);
    let meta = &node.metadata;
    let node_status = node.status.as_ref();

    let addresses: Vec<NodeAddress> = node_status
        .and_then(|s| s.addresses.as_ref())
        .map(|addrs| {
            addrs
                .iter()
                .map(|a| NodeAddress {
                    address_type: a.type_.clone(),
                    address: a.address.clone(),
                })
                .collect()
        })
        .unwrap_or_default();

    let taints: Vec<NodeTaint> = node
        .spec
        .as_ref()
        .and_then(|s| s.taints.as_ref())
        .map(|t| {
            t.iter()
                .map(|taint| NodeTaint {
                    key: taint.key.clone(),
                    value: taint.value.clone(),
                    effect: taint.effect.clone(),
                })
                .collect()
        })
        .unwrap_or_default();

    NodeDetail {
        name: summary.name,
        status: summary.status,
        roles: summary.roles,
        cpu_capacity: summary.cpu_capacity,
        memory_capacity: summary.memory_capacity,
        cpu_allocatable: summary.cpu_allocatable,
        memory_allocatable: summary.memory_allocatable,
        os_image: summary.os_image,
        kernel_version: summary.kernel_version,
        kubelet_version: summary.kubelet_version,
        conditions: summary.conditions,
        addresses,
        taints,
        labels: meta.labels.clone().unwrap_or_default(),
        annotations: meta.annotations.clone().unwrap_or_default(),
        created_at: summary.created_at,
    }
}

pub fn tool_definitions() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "name": "list_nodes",
            "description": "List all nodes in the cluster with status, roles, capacity, and version info.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "label_selector": { "type": "string", "description": "Label selector to filter nodes (e.g. kubernetes.io/os=linux)" },
                    "field_selector": { "type": "string", "description": "Field selector to filter results (e.g. metadata.name=node1)" }
                },
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "get_node",
            "description": "Get detailed information for a single node including conditions, addresses, allocatable resources, taints, labels, and annotations.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Node name" }
                },
                "required": ["name"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "get_node_metrics",
            "description": "Get node-level CPU and memory usage metrics from metrics-server.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "cordon_node",
            "description": "Mark a node as unschedulable (cordon). Prevents new pods from being scheduled on the node.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Node name" }
                },
                "required": ["name"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "uncordon_node",
            "description": "Mark a node as schedulable (uncordon). Allows new pods to be scheduled on the node.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Node name" }
                },
                "required": ["name"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "drain_node",
            "description": "Drain a node by cordoning it and evicting all non-DaemonSet pods.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Node name" },
                    "grace_period_seconds": { "type": "integer", "description": "Grace period in seconds for pod deletion" }
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
        "list_nodes" => list_nodes(client, args).await,
        "get_node" => get_node(client, args).await,
        "get_node_metrics" => get_node_metrics(client).await,
        "cordon_node" => cordon_node(client, args).await,
        "uncordon_node" => uncordon_node(client, args).await,
        "drain_node" => drain_node(client, args).await,
        _ => return None,
    };
    Some(result)
}

async fn list_nodes(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let node_api = api(client);
    let mut lp = ListParams::default();
    if let Some(sel) = args["label_selector"].as_str() {
        lp = lp.labels(sel);
    }
    if let Some(sel) = args["field_selector"].as_str() {
        lp = lp.fields(sel);
    }
    let list = node_api.list(&lp).await.map_err(|e| e.to_string())?;

    let summaries: Vec<serde_json::Value> = list
        .iter()
        .map(|node| {
            let s = node_summary(node);
            serde_json::to_value(s).unwrap_or_default()
        })
        .collect();

    serde_json::to_string_pretty(&summaries).map_err(|e| e.to_string())
}

async fn get_node(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let name = args["name"].as_str().ok_or("name is required")?;

    let node_api = api(client);
    let node = node_api.get(name).await.map_err(|e| e.to_string())?;
    let detail = node_detail(&node);

    serde_json::to_string_pretty(&serde_json::to_value(detail).unwrap_or_default())
        .map_err(|e| e.to_string())
}

async fn get_node_metrics(client: &K8sClient) -> Result<String, String> {
    let uri = "/apis/metrics.k8s.io/v1beta1/nodes";

    let req = http::Request::builder()
        .uri(uri)
        .body(Vec::<u8>::new())
        .map_err(|e: http::Error| e.to_string())?;

    #[derive(Deserialize)]
    struct NodeMetricsList {
        items: Vec<RawNodeMetrics>,
    }
    #[derive(Deserialize)]
    struct RawNodeMetrics {
        metadata: NodeMetricsMeta,
        usage: NodeUsage,
    }
    #[derive(Deserialize)]
    struct NodeMetricsMeta {
        name: String,
    }
    #[derive(Deserialize)]
    struct NodeUsage {
        cpu: String,
        memory: String,
    }

    let result: Result<NodeMetricsList, kube::Error> = client.inner().request(req).await;

    match result {
        Ok(list) => {
            let lines: Vec<String> = list
                .items
                .iter()
                .map(|node| {
                    format!(
                        "node={} cpu={} memory={}",
                        node.metadata.name, node.usage.cpu, node.usage.memory,
                    )
                })
                .collect();
            if lines.is_empty() {
                Ok("No node metrics found. Is metrics-server installed?".to_string())
            } else {
                Ok(lines.join("\n"))
            }
        }
        Err(kube::Error::Api(api_err)) if api_err.code == 404 => {
            Ok("metrics-server does not appear to be installed in this cluster.".to_string())
        }
        Err(kube::Error::Api(api_err)) if api_err.code == 503 => {
            Ok("metrics-server is installed but not ready. Give it 60s after startup.".to_string())
        }
        Err(e) => Err(e.to_string()),
    }
}

async fn cordon_node(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let name = args["name"].as_str().ok_or("name is required")?;

    let node_api = api(client);
    let patch = serde_json::json!({
        "spec": {
            "unschedulable": true
        }
    });
    let patched = node_api
        .patch(name, &PatchParams::apply("mcp-k8s"), &Patch::Merge(&patch))
        .await
        .map_err(|e| e.to_string())?;

    let unschedulable = patched
        .spec
        .as_ref()
        .and_then(|s| s.unschedulable)
        .unwrap_or(false);

    let result = serde_json::json!({
        "node": name,
        "action": "cordon",
        "unschedulable": unschedulable
    });
    serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
}

async fn uncordon_node(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let name = args["name"].as_str().ok_or("name is required")?;

    let node_api = api(client);
    let patch = serde_json::json!({
        "spec": {
            "unschedulable": false
        }
    });
    let patched = node_api
        .patch(name, &PatchParams::apply("mcp-k8s"), &Patch::Merge(&patch))
        .await
        .map_err(|e| e.to_string())?;

    let unschedulable = patched
        .spec
        .as_ref()
        .and_then(|s| s.unschedulable)
        .unwrap_or(false);

    let result = serde_json::json!({
        "node": name,
        "action": "uncordon",
        "unschedulable": unschedulable
    });
    serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
}

async fn drain_node(client: &K8sClient, args: &serde_json::Value) -> Result<String, String> {
    let name = args["name"].as_str().ok_or("name is required")?;
    let grace_period = args["grace_period_seconds"].as_i64();

    // Step 1: Cordon the node
    let node_api = api(client);
    let cordon_patch = serde_json::json!({
        "spec": {
            "unschedulable": true
        }
    });
    node_api
        .patch(
            name,
            &PatchParams::apply("mcp-k8s"),
            &Patch::Merge(&cordon_patch),
        )
        .await
        .map_err(|e| e.to_string())?;

    // Step 2: List all pods on this node
    let pod_api: kube::Api<Pod> = kube::Api::all(client.inner().clone());
    let lp = ListParams::default().fields(&format!("spec.nodeName={name}"));
    let pod_list = pod_api.list(&lp).await.map_err(|e| e.to_string())?;

    // Step 3: Evict non-DaemonSet pods
    let mut dp = DeleteParams::default();
    if let Some(grace) = grace_period {
        dp = dp.grace_period(grace as u32);
    }

    let mut evicted_count: u32 = 0;
    for pod in &pod_list {
        // Skip pods owned by a DaemonSet
        let is_daemonset = pod
            .metadata
            .owner_references
            .as_ref()
            .map(|refs| refs.iter().any(|r| r.kind == "DaemonSet"))
            .unwrap_or(false);
        if is_daemonset {
            continue;
        }

        let pod_name = pod.metadata.name.as_deref().unwrap_or("");
        let pod_ns = pod.metadata.namespace.as_deref().unwrap_or("default");

        let ns_pod_api: kube::Api<Pod> = kube::Api::namespaced(client.inner().clone(), pod_ns);
        ns_pod_api
            .delete(pod_name, &dp)
            .await
            .map_err(|e| e.to_string())?;
        evicted_count += 1;
    }

    let result = serde_json::json!({
        "node": name,
        "action": "drain",
        "cordoned": true,
        "evicted_pods": evicted_count
    });
    serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::api::core::v1::{
        NodeAddress as K8sNodeAddress, NodeCondition, NodeSpec, NodeStatus, NodeSystemInfo, Taint,
    };
    use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
    use std::collections::BTreeMap;

    #[test]
    fn tool_definitions_returns_six_tools() {
        let defs = tool_definitions();
        assert_eq!(defs.len(), 6);
        assert_eq!(defs[0]["name"], "list_nodes");
        assert_eq!(defs[1]["name"], "get_node");
        assert_eq!(defs[2]["name"], "get_node_metrics");
        assert_eq!(defs[3]["name"], "cordon_node");
        assert_eq!(defs[4]["name"], "uncordon_node");
        assert_eq!(defs[5]["name"], "drain_node");
    }

    #[test]
    fn get_node_metrics_tool_has_no_required_params() {
        let defs = tool_definitions();
        let metrics_tool = &defs[2];
        assert_eq!(metrics_tool["name"], "get_node_metrics");
        assert!(metrics_tool["inputSchema"]["required"].is_null());
    }

    #[test]
    fn cordon_node_tool_requires_name() {
        let defs = tool_definitions();
        let cordon_tool = &defs[3];
        assert_eq!(cordon_tool["name"], "cordon_node");
        let required = cordon_tool["inputSchema"]["required"]
            .as_array()
            .expect("required should be an array");
        assert_eq!(required.len(), 1);
        assert_eq!(required[0], "name");
    }

    #[test]
    fn uncordon_node_tool_requires_name() {
        let defs = tool_definitions();
        let uncordon_tool = &defs[4];
        assert_eq!(uncordon_tool["name"], "uncordon_node");
        let required = uncordon_tool["inputSchema"]["required"]
            .as_array()
            .expect("required should be an array");
        assert_eq!(required.len(), 1);
        assert_eq!(required[0], "name");
    }

    #[test]
    fn drain_node_tool_requires_name_with_optional_grace_period() {
        let defs = tool_definitions();
        let drain_tool = &defs[5];
        assert_eq!(drain_tool["name"], "drain_node");
        let required = drain_tool["inputSchema"]["required"]
            .as_array()
            .expect("required should be an array");
        assert_eq!(required.len(), 1);
        assert_eq!(required[0], "name");
        // grace_period_seconds is defined but not required
        assert!(drain_tool["inputSchema"]["properties"]["grace_period_seconds"].is_object());
    }

    #[test]
    fn node_detail_serialization() {
        let detail = NodeDetail {
            name: "node-1".to_string(),
            status: "Ready".to_string(),
            roles: vec!["control-plane".to_string()],
            cpu_capacity: Some("4".to_string()),
            memory_capacity: Some("8Gi".to_string()),
            cpu_allocatable: Some("3800m".to_string()),
            memory_allocatable: Some("7Gi".to_string()),
            os_image: Some("Ubuntu 22.04".to_string()),
            kernel_version: Some("5.15.0".to_string()),
            kubelet_version: Some("v1.32.0".to_string()),
            conditions: vec![NodeConditionSummary {
                condition_type: "Ready".to_string(),
                status: "True".to_string(),
                reason: Some("KubeletReady".to_string()),
                message: Some("kubelet is posting ready status".to_string()),
                last_transition: None,
            }],
            addresses: vec![NodeAddress {
                address_type: "InternalIP".to_string(),
                address: "10.0.0.1".to_string(),
            }],
            taints: vec![NodeTaint {
                key: "node-role.kubernetes.io/control-plane".to_string(),
                value: None,
                effect: "NoSchedule".to_string(),
            }],
            labels: BTreeMap::from([("kubernetes.io/os".to_string(), "linux".to_string())]),
            annotations: BTreeMap::new(),
            created_at: Some("2024-01-01T00:00:00Z".to_string()),
        };

        let json = serde_json::to_string_pretty(&detail).expect("serialization should succeed");
        let parsed: serde_json::Value =
            serde_json::from_str(&json).expect("should parse back as JSON");

        assert_eq!(parsed["name"], "node-1");
        assert_eq!(parsed["status"], "Ready");
        assert_eq!(parsed["roles"][0], "control-plane");
        assert_eq!(parsed["cpu_capacity"], "4");
        assert_eq!(parsed["addresses"][0]["address_type"], "InternalIP");
        assert_eq!(parsed["addresses"][0]["address"], "10.0.0.1");
        assert_eq!(
            parsed["taints"][0]["key"],
            "node-role.kubernetes.io/control-plane"
        );
        assert_eq!(parsed["taints"][0]["effect"], "NoSchedule");
        assert!(parsed["taints"][0]["value"].is_null());
        assert_eq!(parsed["conditions"][0]["condition_type"], "Ready");
    }

    fn make_test_node() -> Node {
        let mut labels = BTreeMap::new();
        labels.insert(
            "node-role.kubernetes.io/control-plane".to_string(),
            String::new(),
        );
        labels.insert("kubernetes.io/os".to_string(), "linux".to_string());

        let mut annotations = BTreeMap::new();
        annotations.insert("node.alpha.kubernetes.io/ttl".to_string(), "0".to_string());

        let mut capacity = BTreeMap::new();
        capacity.insert("cpu".to_string(), Quantity("4".to_string()));
        capacity.insert("memory".to_string(), Quantity("8Gi".to_string()));

        let mut allocatable = BTreeMap::new();
        allocatable.insert("cpu".to_string(), Quantity("3800m".to_string()));
        allocatable.insert("memory".to_string(), Quantity("7Gi".to_string()));

        Node {
            metadata: ObjectMeta {
                name: Some("test-node".to_string()),
                labels: Some(labels),
                annotations: Some(annotations),
                ..Default::default()
            },
            spec: Some(NodeSpec {
                taints: Some(vec![Taint {
                    key: "node-role.kubernetes.io/control-plane".to_string(),
                    value: None,
                    effect: "NoSchedule".to_string(),
                    ..Default::default()
                }]),
                ..Default::default()
            }),
            status: Some(NodeStatus {
                capacity: Some(capacity),
                allocatable: Some(allocatable),
                conditions: Some(vec![NodeCondition {
                    type_: "Ready".to_string(),
                    status: "True".to_string(),
                    reason: Some("KubeletReady".to_string()),
                    message: Some("kubelet is posting ready status".to_string()),
                    ..Default::default()
                }]),
                addresses: Some(vec![
                    K8sNodeAddress {
                        type_: "InternalIP".to_string(),
                        address: "10.0.0.5".to_string(),
                    },
                    K8sNodeAddress {
                        type_: "Hostname".to_string(),
                        address: "test-node".to_string(),
                    },
                ]),
                node_info: Some(NodeSystemInfo {
                    os_image: "Ubuntu 22.04.3 LTS".to_string(),
                    kernel_version: "5.15.0-91-generic".to_string(),
                    kubelet_version: "v1.32.0".to_string(),
                    ..Default::default()
                }),
                ..Default::default()
            }),
        }
    }

    #[test]
    fn extract_node_detail_from_constructed_node() {
        let node = make_test_node();
        let detail = node_detail(&node);

        assert_eq!(detail.name, "test-node");
        assert_eq!(detail.status, "Ready");
        assert_eq!(detail.roles, vec!["control-plane"]);
        assert_eq!(detail.cpu_capacity.as_deref(), Some("4"));
        assert_eq!(detail.memory_capacity.as_deref(), Some("8Gi"));
        assert_eq!(detail.cpu_allocatable.as_deref(), Some("3800m"));
        assert_eq!(detail.memory_allocatable.as_deref(), Some("7Gi"));
        assert_eq!(detail.os_image.as_deref(), Some("Ubuntu 22.04.3 LTS"));
        assert_eq!(detail.kernel_version.as_deref(), Some("5.15.0-91-generic"));
        assert_eq!(detail.kubelet_version.as_deref(), Some("v1.32.0"));

        assert_eq!(detail.conditions.len(), 1);
        assert_eq!(detail.conditions[0].condition_type, "Ready");
        assert_eq!(detail.conditions[0].status, "True");

        assert_eq!(detail.addresses.len(), 2);
        assert_eq!(detail.addresses[0].address_type, "InternalIP");
        assert_eq!(detail.addresses[0].address, "10.0.0.5");
        assert_eq!(detail.addresses[1].address_type, "Hostname");
        assert_eq!(detail.addresses[1].address, "test-node");

        assert_eq!(detail.taints.len(), 1);
        assert_eq!(
            detail.taints[0].key,
            "node-role.kubernetes.io/control-plane"
        );
        assert_eq!(detail.taints[0].effect, "NoSchedule");
        assert!(detail.taints[0].value.is_none());

        assert_eq!(
            detail.labels.get("kubernetes.io/os"),
            Some(&"linux".to_string())
        );
        assert_eq!(
            detail.annotations.get("node.alpha.kubernetes.io/ttl"),
            Some(&"0".to_string())
        );
    }

    #[test]
    fn extract_node_summary_from_constructed_node() {
        let node = make_test_node();
        let summary = node_summary(&node);

        assert_eq!(summary.name, "test-node");
        assert_eq!(summary.status, "Ready");
        assert_eq!(summary.roles, vec!["control-plane"]);
        assert_eq!(summary.cpu_capacity.as_deref(), Some("4"));
        assert_eq!(summary.memory_capacity.as_deref(), Some("8Gi"));
        assert_eq!(summary.kubelet_version.as_deref(), Some("v1.32.0"));
        assert_eq!(summary.os_image.as_deref(), Some("Ubuntu 22.04.3 LTS"));
    }
}
