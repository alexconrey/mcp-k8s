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
    use http::{Request, Response};
    use kube::client::Body;

    // ClusterManager is HashMap + RwLock bookkeeping and never calls the API,
    // so an unconnected mock kube::Client is enough. (These tests used to load
    // the developer's kubeconfig behind #[ignore], so `cargo test -- --ignored`
    // could reach a real cluster.)
    fn mock_client() -> K8sClient {
        let (svc, _handle) = tower_test::mock::pair::<Request<Body>, Response<Body>>();
        K8sClient::new(
            kube::Client::new(svc, "default"),
            vec![],
            Default::default(),
        )
    }

    async fn make_manager() -> ClusterManager {
        ClusterManager::new(mock_client(), "test-cluster".to_string()).await
    }

    #[tokio::test]
    async fn test_active_name_defaults_to_initial() {
        let mgr = make_manager().await;
        assert_eq!(mgr.active_name().await, "test-cluster");
        assert!(mgr.active_client().await.is_some());
    }

    #[tokio::test]
    async fn test_list_clusters_contains_default() {
        let mgr = make_manager().await;
        let clusters = mgr.list_clusters().await;
        assert_eq!(clusters, vec!["test-cluster".to_string()]);
    }

    #[tokio::test]
    async fn test_switch_nonexistent_returns_error() {
        let mgr = make_manager().await;
        let result = mgr.switch("nonexistent").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not found"));
        assert_eq!(mgr.active_name().await, "test-cluster");
    }

    #[tokio::test]
    async fn test_add_and_switch() {
        let mgr = make_manager().await;
        mgr.add_cluster("second".to_string(), mock_client()).await;

        let mut clusters = mgr.list_clusters().await;
        clusters.sort();
        assert_eq!(
            clusters,
            vec!["second".to_string(), "test-cluster".to_string()]
        );

        assert!(mgr.switch("second").await.is_ok());
        assert_eq!(mgr.active_name().await, "second");

        assert!(mgr.switch("test-cluster").await.is_ok());
        assert_eq!(mgr.active_name().await, "test-cluster");
    }
}
