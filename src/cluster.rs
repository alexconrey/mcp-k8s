use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;

use crate::client::K8sClient;

/// Manages multiple named Kubernetes cluster clients and tracks
/// which one is currently active.
#[derive(Clone)]
pub struct ClusterManager {
    clusters: Arc<RwLock<HashMap<String, K8sClient>>>,
    active: Arc<RwLock<String>>,
}

impl ClusterManager {
    /// Create a new `ClusterManager` seeded with a single default client.
    pub async fn new(default_client: K8sClient, default_name: String) -> Self {
        let mut clusters = HashMap::new();
        clusters.insert(default_name.clone(), default_client);
        Self {
            clusters: Arc::new(RwLock::new(clusters)),
            active: Arc::new(RwLock::new(default_name)),
        }
    }

    /// Return a clone of the currently active client, or `None` if the
    /// active name no longer maps to a client (should not happen in practice).
    pub async fn active_client(&self) -> Option<K8sClient> {
        let active = self.active.read().await;
        let clusters = self.clusters.read().await;
        clusters.get(&*active).cloned()
    }

    /// Return the name of the currently active cluster.
    pub async fn active_name(&self) -> String {
        self.active.read().await.clone()
    }

    /// Switch the active cluster to `name`. Returns an error if the name
    /// is not registered.
    pub async fn switch(&self, name: &str) -> Result<(), String> {
        let clusters = self.clusters.read().await;
        if clusters.contains_key(name) {
            drop(clusters);
            *self.active.write().await = name.to_string();
            Ok(())
        } else {
            Err(format!("cluster '{}' not found", name))
        }
    }

    /// List the names of all registered clusters.
    pub async fn list_clusters(&self) -> Vec<String> {
        let clusters = self.clusters.read().await;
        clusters.keys().cloned().collect()
    }

    /// Register a new named cluster client.
    pub async fn add_cluster(&self, name: String, client: K8sClient) {
        self.clusters.write().await.insert(name, client);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ClusterManager tests require a real kube::Client (K8sClient wraps one),
    // so they are integration-level tests gated behind `#[ignore]`.
    // The manager logic itself is straightforward HashMap + RwLock bookkeeping.

    async fn make_manager() -> ClusterManager {
        let kube_client = kube::Client::try_default()
            .await
            .expect("requires kubeconfig");
        let client = K8sClient::new(kube_client, vec![], Default::default());
        ClusterManager::new(client, "test-cluster".to_string()).await
    }

    #[tokio::test]
    #[ignore = "requires kubeconfig"]
    async fn test_active_name_defaults_to_initial() {
        let mgr = make_manager().await;
        assert_eq!(mgr.active_name().await, "test-cluster");
    }

    #[tokio::test]
    #[ignore = "requires kubeconfig"]
    async fn test_list_clusters_contains_default() {
        let mgr = make_manager().await;
        let clusters = mgr.list_clusters().await;
        assert!(clusters.contains(&"test-cluster".to_string()));
    }

    #[tokio::test]
    #[ignore = "requires kubeconfig"]
    async fn test_switch_nonexistent_returns_error() {
        let mgr = make_manager().await;
        let result = mgr.switch("nonexistent").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not found"));
    }

    #[tokio::test]
    #[ignore = "requires kubeconfig"]
    async fn test_add_and_switch() {
        let mgr = make_manager().await;
        let kube_client = kube::Client::try_default()
            .await
            .expect("requires kubeconfig");
        let client2 = K8sClient::new(kube_client, vec![], Default::default());
        mgr.add_cluster("second".to_string(), client2).await;

        assert!(mgr.switch("second").await.is_ok());
        assert_eq!(mgr.active_name().await, "second");

        assert!(mgr.switch("test-cluster").await.is_ok());
        assert_eq!(mgr.active_name().await, "test-cluster");
    }
}
