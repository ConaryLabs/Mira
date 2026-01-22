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