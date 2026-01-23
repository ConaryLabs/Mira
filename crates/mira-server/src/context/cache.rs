// crates/mira-server/src/context/cache.rs
// LRU cache for injection results

use lru::LruCache;
use std::num::NonZeroUsize;
use tokio::sync::Mutex;

pub struct InjectionCache {
    cache: Mutex<LruCache<String, String>>,
}

impl InjectionCache {
    pub fn new() -> Self {
        Self {
            cache: Mutex::new(LruCache::new(NonZeroUsize::new(100).unwrap())),
        }
    }

    pub async fn get(&self, key: &str) -> Option<String> {
        let mut cache = self.cache.lock().await;
        cache.get(key).cloned()
    }

    pub async fn put(&self, key: &str, value: String) {
        let mut cache = self.cache.lock().await;
        cache.put(key.to_string(), value);
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

        // The first 10 entries should have been evicted
        for i in 0..10 {
            assert!(cache.get(&format!("key{}", i)).await.is_none(),
                "key{} should have been evicted", i);
        }

        // The last 100 entries should still be present
        for i in 10..110 {
            assert!(cache.get(&format!("key{}", i)).await.is_some(),
                "key{} should still be present", i);
        }
    }

    #[tokio::test]
    async fn test_cache_empty_value() {
        let cache = InjectionCache::new();
        cache.put("empty", "".to_string()).await;

        let result = cache.get("empty").await;
        assert_eq!(result, Some("".to_string()));
    }
}