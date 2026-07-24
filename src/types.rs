use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "snake_case")]
pub enum DeploymentPhase {
    Available,
    Progressing,
    Degraded,
    Failed,
    ScaledToZero,
}

#[derive(Serialize, Debug)]
pub struct ReplicaCounts {
    pub desired: i32,
    pub ready: i32,
    pub available: i32,
    pub updated: i32,
}

#[derive(Serialize, Debug)]
pub struct DeploymentConditionSummary {
    pub condition_type: String,
    pub status: String,
    pub reason: Option<String>,
    pub message: Option<String>,
    pub last_transition: Option<String>,
}

#[derive(Serialize, Debug)]
pub struct DeploymentSummary {
    pub name: String,
    pub namespace: String,
    pub image: String,
    pub replicas: ReplicaCounts,
    pub status: DeploymentPhase,
    pub created_at: Option<String>,
    pub labels: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_requests: Option<ResourceSpecOutput>,
}

#[derive(Serialize, Debug)]
pub struct DeploymentDetail {
    pub name: String,
    pub namespace: String,
    pub image: String,
    pub replicas: ReplicaCounts,
    pub status: DeploymentPhase,
    pub conditions: Vec<DeploymentConditionSummary>,
    pub created_at: Option<String>,
    pub labels: BTreeMap<String, String>,
    pub annotations: BTreeMap<String, String>,
    pub env: Vec<EnvVarOutput>,
    pub command: Vec<String>,
    pub args: Vec<String>,
    pub resource_limits: Option<ResourceSpecOutput>,
    pub resource_requests: Option<ResourceSpecOutput>,
    pub liveness_probe: Option<ProbeOutput>,
    pub readiness_probe: Option<ProbeOutput>,
    pub startup_probe: Option<ProbeOutput>,
}

#[derive(Serialize, Debug)]
pub struct EnvVarOutput {
    pub name: String,
    pub value: Option<String>,
}

#[derive(Serialize, Debug)]
pub struct ResourceSpecOutput {
    pub cpu: Option<String>,
    pub memory: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
pub struct ProbeOutput {
    pub probe_type: String,
    pub path: Option<String>,
    pub port: Option<i32>,
    pub command: Option<Vec<String>>,
    pub initial_delay_seconds: Option<i32>,
    pub period_seconds: Option<i32>,
    pub timeout_seconds: Option<i32>,
    pub failure_threshold: Option<i32>,
    pub success_threshold: Option<i32>,
}

#[derive(Serialize, Debug)]
pub struct PodConditionSummary {
    pub condition_type: String,
    pub status: bool,
    pub reason: Option<String>,
    pub message: Option<String>,
}

#[derive(Serialize, Debug)]
pub struct PodSummary {
    pub name: String,
    pub phase: String,
    pub ready: bool,
    pub restart_count: i32,
    pub node: Option<String>,
    pub started_at: Option<String>,
    pub conditions: Vec<PodConditionSummary>,
    pub container_statuses: Vec<ContainerStatusSummary>,
    pub oom_killed: bool,
}

#[derive(Serialize, Debug)]
pub struct ContainerStatusSummary {
    pub name: String,
    pub ready: bool,
    pub restart_count: i32,
    pub state: String,
    pub state_reason: Option<String>,
    pub image: String,
    pub oom_killed: bool,
}

#[derive(Serialize, Debug)]
pub struct IngressSummary {
    pub name: String,
    pub namespace: String,
    pub hosts: Vec<String>,
    pub ingress_class: Option<String>,
    pub created_at: Option<String>,
    pub labels: BTreeMap<String, String>,
    pub addresses: Vec<String>,
}

#[derive(Serialize, Debug)]
pub struct IngressDetail {
    pub name: String,
    pub namespace: String,
    pub hosts: Vec<String>,
    pub ingress_class: Option<String>,
    pub created_at: Option<String>,
    pub labels: BTreeMap<String, String>,
    pub addresses: Vec<String>,
    pub rules: Vec<IngressRuleSummary>,
    pub tls: Vec<IngressTlsSummary>,
    pub annotations: BTreeMap<String, String>,
}

#[derive(Serialize, Debug)]
pub struct IngressRuleSummary {
    pub host: Option<String>,
    pub paths: Vec<IngressPathSummary>,
}

#[derive(Serialize, Debug)]
pub struct IngressPathSummary {
    pub path: String,
    pub path_type: String,
    pub service_name: String,
    pub service_port: i32,
}

#[derive(Serialize, Debug)]
pub struct IngressTlsSummary {
    pub hosts: Vec<String>,
    pub secret_name: Option<String>,
}

#[derive(Serialize, Debug)]
pub struct CronJobSummary {
    pub name: String,
    pub namespace: String,
    pub schedule: String,
    pub suspend: bool,
    pub active_count: i32,
    pub last_schedule_time: Option<String>,
    pub created_at: Option<String>,
    pub labels: BTreeMap<String, String>,
}

#[derive(Serialize, Debug)]
pub struct NodeConditionSummary {
    pub condition_type: String,
    pub status: String,
    pub reason: Option<String>,
    pub message: Option<String>,
    pub last_transition: Option<String>,
}

#[derive(Serialize, Debug)]
pub struct NodeSummary {
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
    pub created_at: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "snake_case")]
pub enum ApplicationHealth {
    Healthy,
    Degraded,
    Unhealthy,
    Empty,
}

#[derive(Serialize, Debug)]
pub struct ApplicationSummary {
    pub name: String,
    pub namespace: String,
    pub description: String,
    pub created_at: Option<String>,
    pub deployment_count: usize,
    pub cronjob_count: usize,
    pub health: ApplicationHealth,
    pub gitops_enabled: bool,
}

#[derive(Serialize, Debug)]
pub struct ApplicationDetail {
    pub name: String,
    pub namespace: String,
    pub description: String,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub git: Option<ApplicationGitConfig>,
    pub deployments: Vec<DeploymentSummary>,
    pub cronjobs: Vec<CronJobSummary>,
    pub health: ApplicationHealth,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ApplicationGitConfig {
    pub repo_url: String,
    pub branch: Option<String>,
    pub token_secret: Option<String>,
}

#[derive(Serialize, Debug)]
pub struct EventSummary {
    pub namespace: String,
    pub name: String,
    pub event_type: String,
    pub reason: Option<String>,
    pub message: Option<String>,
    pub involved_object_kind: String,
    pub involved_object_name: String,
    pub involved_object_namespace: Option<String>,
    pub count: Option<i32>,
    pub first_timestamp: Option<String>,
    pub last_timestamp: Option<String>,
    pub source_component: Option<String>,
    pub source_host: Option<String>,
}

// Re-exports from per-resource modules.
//
// These make `crate::types` the canonical import path for all resource types,
// regardless of which module defines them.
pub use crate::resources::admission::{PolicySummary, WebhookConfigSummary, WebhookDetail};
pub use crate::resources::clusterrole::ClusterRoleSummary;
pub use crate::resources::clusterrolebinding::ClusterRoleBindingSummary;
pub use crate::resources::configmap::ConfigMapSummary;
pub use crate::resources::csr::CsrSummary;
pub use crate::resources::daemonset::{
    DaemonSetConditionSummary, DaemonSetDetail, DaemonSetSummary,
};
pub use crate::resources::endpoints::EndpointsSummary;
pub use crate::resources::endpointslice::EndpointSliceSummary;
pub use crate::resources::flowcontrol::{FlowSchemaSummary, PriorityLevelSummary};
pub use crate::resources::hpa::HpaSummary;
pub use crate::resources::ingressclass::IngressClassSummary;
pub use crate::resources::job::{JobConditionSummary, JobDetail, JobSummary};
pub use crate::resources::lease::LeaseSummary;
pub use crate::resources::limitrange::LimitRangeSummary;
pub use crate::resources::namespace::NamespaceDetail;
pub use crate::resources::networkpolicy::NetworkPolicySummary;
pub use crate::resources::node::{NodeAddress, NodeDetail, NodeTaint};
pub use crate::resources::pdb::PdbSummary;
pub use crate::resources::pod::PodDetail;
pub use crate::resources::priorityclass::PriorityClassSummary;
pub use crate::resources::pv::PvSummary;
pub use crate::resources::pvc::PvcSummary;
pub use crate::resources::replicaset::{ReplicaSetDetail, ReplicaSetSummary};
pub use crate::resources::resourcequota::ResourceQuotaSummary;
pub use crate::resources::role::RoleSummary;
pub use crate::resources::rolebinding::RoleBindingSummary;
pub use crate::resources::runtimeclass::RuntimeClassSummary;
pub use crate::resources::secret::SecretSummary;
pub use crate::resources::service::{ServicePortSummary, ServiceSummary};
pub use crate::resources::serviceaccount::ServiceAccountSummary;
pub use crate::resources::statefulset::{StatefulSetDetail, StatefulSetSummary};
pub use crate::resources::storage_internals::{
    CSIDriverSummary, CSINodeSummary, VolumeAttachmentSummary,
};
pub use crate::resources::storageclass::StorageClassSummary;
