use hickory_proto::op::Message;
use hickory_proto::serialize::binary::BinDecodable;
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

    fn create_cache_key(query: &[u8]) -> Option<Vec<u8>> {
        if let Ok(message) = Message::from_bytes(query) {
            // Only use the query name and type as the cache key
            let mut key = Vec::new();
            for question in message.queries() {
                key.extend_from_slice(question.name().to_ascii().as_bytes());
                key.extend_from_slice(&question.query_type().to_string().as_bytes());
            }
            Some(key)
        } else {
            None
        }
    }

    pub async fn get(&self, query: &[u8]) -> Option<Vec<u8>> {
        if let Some(key) = Self::create_cache_key(query) {
            let cache = self.cache.read().await;
            if let Some(entry) = cache.get(&key) {
                if entry.expires_at > SystemTime::now() {
                    if let (Ok(query_message), Ok(mut cached_message)) = (
                        Message::from_bytes(query),
                        Message::from_bytes(&entry.response),
                    ) {
                        cached_message.set_id(query_message.id());
                        if let Ok(updated_response) = cached_message.to_vec() {
                            return Some(updated_response);
                        }
                    }
                    return Some(entry.response.clone());
                }
            }
        }
        None
    }

    pub async fn set(&self, query: Vec<u8>, response: Vec<u8>, ttl: Duration) {
        if let Some(key) = Self::create_cache_key(&query) {
            let expires_at = SystemTime::now() + ttl;
            let entry = CacheEntry {
                response,
                expires_at,
            };

            let mut cache = self.cache.write().await;
            cache.insert(key, entry);
        }
    }

    pub async fn cleanup(&self) {
        let mut cache = self.cache.write().await;
        cache.retain(|_, entry| entry.expires_at > SystemTime::now());
    }

    pub async fn dump_cache(&self) {
        let cache = self.cache.read().await;
        println!("Current cache contents:");
        println!("Total entries: {}", cache.len());
        for (query, entry) in cache.iter() {
            println!(
                "Query size: {}, Response size: {}, Expires at: {:?}",
                query.len(),
                entry.response.len(),
                entry.expires_at
            );
        }
    }
}
