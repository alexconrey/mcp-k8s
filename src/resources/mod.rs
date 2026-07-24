pub mod admission;
pub mod admission_alpha;
pub mod auth;
pub mod cluster_mgmt;
pub mod cluster_trust_bundle;
pub mod clusterrole;
pub mod clusterrolebinding;
pub mod configmap;
pub mod crd;
pub mod cronjob;
pub mod csr;
pub mod daemonset;
pub mod deployment;
pub mod device_resources;
pub mod endpoints;
pub mod endpointslice;
pub mod flowcontrol;
pub mod generic;
pub mod hpa;
pub mod ingress;
pub mod ingressclass;
pub mod ip_networking;
pub mod job;
pub mod lease;
pub mod lease_candidate;
pub mod limitrange;
pub mod namespace;
pub mod networkpolicy;
pub mod node;
pub mod pdb;
pub mod pod;
pub mod priorityclass;
pub mod pv;
pub mod pvc;
pub mod replicaset;
pub mod resourcequota;
pub mod role;
pub mod rolebinding;
pub mod runtimeclass;
pub mod secret;
pub mod service;
pub mod serviceaccount;
pub mod statefulset;
pub mod storage_internals;
pub mod storage_migration;
pub mod storage_version;
pub mod storageclass;
pub mod volume_attributes;
pub mod watch;

use crate::client::K8sClient;

pub fn all_tool_definitions() -> Vec<serde_json::Value> {
    let mut tools = Vec::new();
    tools.extend(cluster_mgmt::tool_definitions());
    tools.extend(admission::tool_definitions());
    tools.extend(admission_alpha::tool_definitions());
    tools.extend(auth::tool_definitions());
    tools.extend(cluster_trust_bundle::tool_definitions());
    tools.extend(clusterrole::tool_definitions());
    tools.extend(clusterrolebinding::tool_definitions());
    tools.extend(configmap::tool_definitions());
    tools.extend(crd::tool_definitions());
    tools.extend(cronjob::tool_definitions());
    tools.extend(csr::tool_definitions());
    tools.extend(daemonset::tool_definitions());
    tools.extend(deployment::tool_definitions());
    tools.extend(device_resources::tool_definitions());
    tools.extend(endpoints::tool_definitions());
    tools.extend(endpointslice::tool_definitions());
    tools.extend(flowcontrol::tool_definitions());
    tools.extend(generic::tool_definitions());
    tools.extend(hpa::tool_definitions());
    tools.extend(ingress::tool_definitions());
    tools.extend(ingressclass::tool_definitions());
    tools.extend(ip_networking::tool_definitions());
    tools.extend(job::tool_definitions());
    tools.extend(lease::tool_definitions());
    tools.extend(lease_candidate::tool_definitions());
    tools.extend(limitrange::tool_definitions());
    tools.extend(namespace::tool_definitions());
    tools.extend(networkpolicy::tool_definitions());
    tools.extend(node::tool_definitions());
    tools.extend(pdb::tool_definitions());
    tools.extend(pod::tool_definitions());
    tools.extend(priorityclass::tool_definitions());
    tools.extend(pv::tool_definitions());
    tools.extend(pvc::tool_definitions());
    tools.extend(replicaset::tool_definitions());
    tools.extend(resourcequota::tool_definitions());
    tools.extend(role::tool_definitions());
    tools.extend(rolebinding::tool_definitions());
    tools.extend(runtimeclass::tool_definitions());
    tools.extend(secret::tool_definitions());
    tools.extend(service::tool_definitions());
    tools.extend(serviceaccount::tool_definitions());
    tools.extend(statefulset::tool_definitions());
    tools.extend(storage_internals::tool_definitions());
    tools.extend(storage_migration::tool_definitions());
    tools.extend(storage_version::tool_definitions());
    tools.extend(storageclass::tool_definitions());
    tools.extend(volume_attributes::tool_definitions());
    tools.extend(watch::tool_definitions());
    tools
}

pub async fn handle_tool(
    client: &K8sClient,
    name: &str,
    args: &serde_json::Value,
) -> Option<Result<String, String>> {
    if let result @ Some(_) = admission::handle_tool(client, name, args).await {
        return result;
    }
    if let result @ Some(_) = admission_alpha::handle_tool(client, name, args).await {
        return result;
    }
    if let result @ Some(_) = auth::handle_tool(client, name, args).await {
        return result;
    }
    if let result @ Some(_) = cluster_trust_bundle::handle_tool(client, name, args).await {
        return result;
    }
    if let result @ Some(_) = clusterrole::handle_tool(client, name, args).await {
        return result;
    }
    if let result @ Some(_) = clusterrolebinding::handle_tool(client, name, args).await {
        return result;
    }
    if let result @ Some(_) = configmap::handle_tool(client, name, args).await {
        return result;
    }
    if let result @ Some(_) = crd::handle_tool(client, name, args).await {
        return result;
    }
    if let result @ Some(_) = cronjob::handle_tool(client, name, args).await {
        return result;
    }
    if let result @ Some(_) = csr::handle_tool(client, name, args).await {
        return result;
    }
    if let result @ Some(_) = daemonset::handle_tool(client, name, args).await {
        return result;
    }
    if let result @ Some(_) = deployment::handle_tool(client, name, args).await {
        return result;
    }
    if let result @ Some(_) = device_resources::handle_tool(client, name, args).await {
        return result;
    }
    if let result @ Some(_) = endpoints::handle_tool(client, name, args).await {
        return result;
    }
    if let result @ Some(_) = endpointslice::handle_tool(client, name, args).await {
        return result;
    }
    if let result @ Some(_) = flowcontrol::handle_tool(client, name, args).await {
        return result;
    }
    if let result @ Some(_) = generic::handle_tool(client, name, args).await {
        return result;
    }
    if let result @ Some(_) = hpa::handle_tool(client, name, args).await {
        return result;
    }
    if let result @ Some(_) = ingress::handle_tool(client, name, args).await {
        return result;
    }
    if let result @ Some(_) = ingressclass::handle_tool(client, name, args).await {
        return result;
    }
    if let result @ Some(_) = ip_networking::handle_tool(client, name, args).await {
        return result;
    }
    if let result @ Some(_) = job::handle_tool(client, name, args).await {
        return result;
    }
    if let result @ Some(_) = lease::handle_tool(client, name, args).await {
        return result;
    }
    if let result @ Some(_) = lease_candidate::handle_tool(client, name, args).await {
        return result;
    }
    if let result @ Some(_) = limitrange::handle_tool(client, name, args).await {
        return result;
    }
    if let result @ Some(_) = namespace::handle_tool(client, name, args).await {
        return result;
    }
    if let result @ Some(_) = networkpolicy::handle_tool(client, name, args).await {
        return result;
    }
    if let result @ Some(_) = node::handle_tool(client, name, args).await {
        return result;
    }
    if let result @ Some(_) = pdb::handle_tool(client, name, args).await {
        return result;
    }
    if let result @ Some(_) = pod::handle_tool(client, name, args).await {
        return result;
    }
    if let result @ Some(_) = priorityclass::handle_tool(client, name, args).await {
        return result;
    }
    if let result @ Some(_) = pv::handle_tool(client, name, args).await {
        return result;
    }
    if let result @ Some(_) = pvc::handle_tool(client, name, args).await {
        return result;
    }
    if let result @ Some(_) = replicaset::handle_tool(client, name, args).await {
        return result;
    }
    if let result @ Some(_) = resourcequota::handle_tool(client, name, args).await {
        return result;
    }
    if let result @ Some(_) = role::handle_tool(client, name, args).await {
        return result;
    }
    if let result @ Some(_) = rolebinding::handle_tool(client, name, args).await {
        return result;
    }
    if let result @ Some(_) = runtimeclass::handle_tool(client, name, args).await {
        return result;
    }
    if let result @ Some(_) = secret::handle_tool(client, name, args).await {
        return result;
    }
    if let result @ Some(_) = service::handle_tool(client, name, args).await {
        return result;
    }
    if let result @ Some(_) = serviceaccount::handle_tool(client, name, args).await {
        return result;
    }
    if let result @ Some(_) = statefulset::handle_tool(client, name, args).await {
        return result;
    }
    if let result @ Some(_) = storage_internals::handle_tool(client, name, args).await {
        return result;
    }
    if let result @ Some(_) = storage_migration::handle_tool(client, name, args).await {
        return result;
    }
    if let result @ Some(_) = storage_version::handle_tool(client, name, args).await {
        return result;
    }
    if let result @ Some(_) = storageclass::handle_tool(client, name, args).await {
        return result;
    }
    if let result @ Some(_) = volume_attributes::handle_tool(client, name, args).await {
        return result;
    }
    if let result @ Some(_) = watch::handle_tool(client, name, args).await {
        return result;
    }
    None
}
