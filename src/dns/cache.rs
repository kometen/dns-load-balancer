use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::RwLock;

#[derive(Clone)]
struct CacheEntry {
    response: Vec<u8>,
    expires_at: SystemTime,
}

pub struct DnsCache {
    cache: Arc<RwLock<HashMap<Vec<u8>, CacheEntry>>>,
}

impl DnsCache {
    pub fn new() -> Self {
        Self {
            cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn get(&self, query: &[u8]) -> Option<Vec<u8>> {
        let cache = self.cache.read().await;
        if let Some(entry) = cache.get(query) {
            if entry.expires_at > SystemTime::now() {
                return Some(entry.response.clone());
            }
        }
        None
    }

    pub async fn set(&self, query: Vec<u8>, response: Vec<u8>, ttl: Duration) {
        let expires_at = SystemTime::now() + ttl;
        let entry = CacheEntry {
            response,
            expires_at,
        };

        let mut cache = self.cache.write().await;
        cache.insert(query, entry);
    }

    pub async fn cleanup(&self) {
        let mut cache = self.cache.write().await;
        cache.retain(|_, entry| entry.expires_at > SystemTime::now());
    }
}
