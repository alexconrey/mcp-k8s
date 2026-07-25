// Lightweight, opt-in in-memory cache for list operations.
//
// Usage in a handler:
//   let cache_key = format!("list_pods:{ns}:{label_selector}");
//   if let Some(cached) = cache.get(&cache_key).await { return Ok(cached); }
//   let result = /* actual K8s API call */;
//   cache.set(cache_key, result.clone()).await;
//   Ok(result)

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::RwLock;

#[derive(Clone)]
pub struct ResponseCache {
    cache: Arc<RwLock<HashMap<String, CacheEntry>>>,
    ttl: Duration,
    enabled: bool,
}

struct CacheEntry {
    value: String,
    inserted_at: Instant,
}

impl ResponseCache {
    pub fn new(ttl_seconds: u64, enabled: bool) -> Self {
        Self {
            cache: Arc::new(RwLock::new(HashMap::new())),
            ttl: Duration::from_secs(ttl_seconds),
            enabled,
        }
    }

    pub fn disabled() -> Self {
        Self::new(0, false)
    }

    pub async fn get(&self, key: &str) -> Option<String> {
        if !self.enabled {
            return None;
        }
        let cache = self.cache.read().await;
        cache.get(key).and_then(|entry| {
            if entry.inserted_at.elapsed() < self.ttl {
                Some(entry.value.clone())
            } else {
                None
            }
        })
    }

    pub async fn set(&self, key: String, value: String) {
        if !self.enabled {
            return;
        }
        let mut cache = self.cache.write().await;
        cache.insert(
            key,
            CacheEntry {
                value,
                inserted_at: Instant::now(),
            },
        );
    }

    pub async fn invalidate(&self, prefix: &str) {
        if !self.enabled {
            return;
        }
        let mut cache = self.cache.write().await;
        cache.retain(|k, _| !k.starts_with(prefix));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn get_set_roundtrip() {
        let cache = ResponseCache::new(60, true);
        cache.set("key1".to_string(), "value1".to_string()).await;
        assert_eq!(cache.get("key1").await, Some("value1".to_string()));
    }

    #[tokio::test]
    async fn get_missing_key_returns_none() {
        let cache = ResponseCache::new(60, true);
        assert_eq!(cache.get("nonexistent").await, None);
    }

    #[tokio::test]
    async fn ttl_expiry() {
        let cache = ResponseCache::new(0, true); // 0-second TTL = immediate expiry
        cache.set("key1".to_string(), "value1".to_string()).await;
        // With a 0s TTL, the entry should already be expired
        assert_eq!(cache.get("key1").await, None);
    }

    #[tokio::test]
    async fn disabled_cache_returns_none() {
        let cache = ResponseCache::disabled();
        cache.set("key1".to_string(), "value1".to_string()).await;
        assert_eq!(cache.get("key1").await, None);
    }

    #[tokio::test]
    async fn disabled_cache_does_not_store() {
        let cache = ResponseCache::disabled();
        cache.set("key1".to_string(), "value1".to_string()).await;
        let inner = cache.cache.read().await;
        assert!(inner.is_empty());
    }

    #[tokio::test]
    async fn invalidate_by_prefix() {
        let cache = ResponseCache::new(60, true);
        cache
            .set("list_pods:ns1:".to_string(), "pods1".to_string())
            .await;
        cache
            .set("list_pods:ns2:".to_string(), "pods2".to_string())
            .await;
        cache
            .set("list_deployments:ns1:".to_string(), "deps1".to_string())
            .await;

        cache.invalidate("list_pods:").await;

        assert_eq!(cache.get("list_pods:ns1:").await, None);
        assert_eq!(cache.get("list_pods:ns2:").await, None);
        assert_eq!(
            cache.get("list_deployments:ns1:").await,
            Some("deps1".to_string())
        );
    }

    #[tokio::test]
    async fn invalidate_on_disabled_cache_is_noop() {
        let cache = ResponseCache::disabled();
        // Should not panic or error
        cache.invalidate("anything").await;
    }

    #[tokio::test]
    async fn set_overwrites_existing_entry() {
        let cache = ResponseCache::new(60, true);
        cache.set("key1".to_string(), "value1".to_string()).await;
        cache.set("key1".to_string(), "value2".to_string()).await;
        assert_eq!(cache.get("key1").await, Some("value2".to_string()));
    }

    #[tokio::test]
    async fn clone_shares_state() {
        let cache = ResponseCache::new(60, true);
        let cache2 = cache.clone();
        cache.set("key1".to_string(), "value1".to_string()).await;
        assert_eq!(cache2.get("key1").await, Some("value1".to_string()));
    }
}
