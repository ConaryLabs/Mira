// crates/mira-server/src/context/cache.rs
// Lock-free LRU cache for injection results using moka

use moka::future::Cache;

pub struct InjectionCache {
    cache: Cache<String, String>,
}

impl InjectionCache {
    pub fn new() -> Self {
        Self {
            // Lock-free concurrent cache with 100 entry capacity
            cache: Cache::builder()
                .max_capacity(100)
                .time_to_live(std::time::Duration::from_secs(300))
                .build(),
        }
    }

    pub async fn get(&self, key: &str) -> Option<String> {
        self.cache.get(key).await
    }

    pub async fn put(&self, key: &str, value: String) {
        self.cache.insert(key.to_string(), value).await;
    }
}

impl Default for InjectionCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_cache_new() {
        let cache = InjectionCache::new();
        let result = cache.get("nonexistent").await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_cache_put_and_get() {
        let cache = InjectionCache::new();
        cache.put("key1", "value1".to_string()).await;

        let result = cache.get("key1").await;
        assert_eq!(result, Some("value1".to_string()));
    }

    #[tokio::test]
    async fn test_cache_overwrite() {
        let cache = InjectionCache::new();
        cache.put("key1", "original".to_string()).await;
        cache.put("key1", "updated".to_string()).await;

        let result = cache.get("key1").await;
        assert_eq!(result, Some("updated".to_string()));
    }

    #[tokio::test]
    async fn test_cache_multiple_keys() {
        let cache = InjectionCache::new();
        cache.put("key1", "value1".to_string()).await;
        cache.put("key2", "value2".to_string()).await;
        cache.put("key3", "value3".to_string()).await;

        assert_eq!(cache.get("key1").await, Some("value1".to_string()));
        assert_eq!(cache.get("key2").await, Some("value2".to_string()));
        assert_eq!(cache.get("key3").await, Some("value3".to_string()));
    }

    #[tokio::test]
    async fn test_cache_lru_eviction() {
        // The cache has a capacity of 100
        let cache = InjectionCache::new();

        // Fill the cache beyond capacity
        for i in 0..110 {
            cache.put(&format!("key{}", i), format!("value{}", i)).await;
        }

        // Run pending maintenance to trigger eviction
        cache.cache.run_pending_tasks().await;

        // Count how many entries remain (should be ~100 due to LRU eviction)
        let mut present = 0;
        for i in 0..110 {
            if cache.get(&format!("key{}", i)).await.is_some() {
                present += 1;
            }
        }
        assert!(
            present <= 100,
            "Cache should have evicted entries, found {}",
            present
        );
    }

    #[tokio::test]
    async fn test_cache_empty_value() {
        let cache = InjectionCache::new();
        cache.put("empty", "".to_string()).await;

        let result = cache.get("empty").await;
        assert_eq!(result, Some("".to_string()));
    }
}
