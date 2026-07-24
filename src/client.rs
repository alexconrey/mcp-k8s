use k8s_openapi::api::apps::v1::Deployment;
use k8s_openapi::api::batch::v1::Job;
use k8s_openapi::api::core::v1::{Event, Namespace, Pod, Service};
use k8s_openapi::api::networking::v1::Ingress;
use kube::Api;

use crate::error::Error;
use crate::permissions::ActionPermissions;

#[derive(Clone)]
pub struct K8sClient {
    client: kube::Client,
    allowed_namespaces: Vec<String>,
    permissions: ActionPermissions,
}

impl K8sClient {
    pub async fn try_default() -> Result<Self, kube::Error> {
        let client = kube::Client::try_default().await?;
        Ok(Self {
            client,
            allowed_namespaces: Vec::new(),
            permissions: ActionPermissions::default(),
        })
    }

    pub fn new(
        client: kube::Client,
        allowed_namespaces: Vec<String>,
        permissions: ActionPermissions,
    ) -> Self {
        Self {
            client,
            allowed_namespaces,
            permissions,
        }
    }

    pub fn permissions(&self) -> &ActionPermissions {
        &self.permissions
    }

    pub fn inner(&self) -> &kube::Client {
        &self.client
    }

    pub fn is_namespace_allowed(&self, ns: &str) -> bool {
        self.allowed_namespaces.is_empty() || self.allowed_namespaces.iter().any(|n| n == ns)
    }

    fn check_namespace(&self, ns: &str) -> Result<(), Error> {
        if !self.is_namespace_allowed(ns) {
            return Err(Error::NamespaceNotAllowed(ns.to_string()));
        }
        Ok(())
    }

    pub fn deployments_api(&self, ns: &str) -> Result<Api<Deployment>, Error> {
        self.check_namespace(ns)?;
        Ok(Api::namespaced(self.client.clone(), ns))
    }

    pub fn pods_api(&self, ns: &str) -> Result<Api<Pod>, Error> {
        self.check_namespace(ns)?;
        Ok(Api::namespaced(self.client.clone(), ns))
    }

    pub fn ingresses_api(&self, ns: &str) -> Result<Api<Ingress>, Error> {
        self.check_namespace(ns)?;
        Ok(Api::namespaced(self.client.clone(), ns))
    }

    pub fn services_api(&self, ns: &str) -> Result<Api<Service>, Error> {
        self.check_namespace(ns)?;
        Ok(Api::namespaced(self.client.clone(), ns))
    }

    pub fn jobs_api(&self, ns: &str) -> Result<Api<Job>, Error> {
        self.check_namespace(ns)?;
        Ok(Api::namespaced(self.client.clone(), ns))
    }

    pub fn events_api(&self, ns: &str) -> Result<Api<Event>, Error> {
        self.check_namespace(ns)?;
        Ok(Api::namespaced(self.client.clone(), ns))
    }

    pub fn namespaces_api(&self) -> Api<Namespace> {
        Api::all(self.client.clone())
    }
}
