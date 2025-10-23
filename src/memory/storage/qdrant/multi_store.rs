// src/memory/storage/qdrant/multi_store.rs  
// Qdrant multi-collection store for 5-head memory system

use std::{collections::HashMap, sync::Arc};
use anyhow::{anyhow, Result};
use tracing::{debug, info, warn};

use crate::{
    config::CONFIG,
    llm::embeddings::EmbeddingHead,
    memory::{
        storage::qdrant::store::QdrantMemoryStore, 
        core::traits::MemoryStore, 
        core::types::MemoryEntry
    },
};

/// Multi-collection Qdrant store that routes operations to appropriate collections
#[derive(Debug, Clone)]
pub struct QdrantMultiStore {
    stores: HashMap<EmbeddingHead, Arc<QdrantMemoryStore>>,
}

impl QdrantMultiStore {
    /// Create a new multi-collection store with all 5 heads
    pub async fn new(base_url: &str, collection_base_name: &str) -> Result<Self> {
        info!("Initializing Qdrant multi-collection store with base: {}", collection_base_name);
        let mut stores = HashMap::new();
        let heads = CONFIG.get_embedding_heads();

        // Ensure we have all 5 heads
        let expected_heads = ["semantic", "code", "summary", "documents", "relationship"];
        for expected in &expected_heads {
            if !heads.contains(&expected.to_string()) {
                warn!("{} head not found in config! Add '{}' to MIRA_EMBED_HEADS", expected, expected);
            }
        }

        for head_str in &heads {
            if let Ok(head) = head_str.parse::<EmbeddingHead>() {
                let collection_name = format!("{}-{}", collection_base_name, head.as_str());
                info!(
                    "Initializing Qdrant collection for head '{}': {}",
                    head.as_str(),
                    collection_name
                );
                
                match QdrantMemoryStore::new(base_url, &collection_name).await {
                    Ok(store) => {
                        stores.insert(head, Arc::new(store));
                        info!("Created {} collection", collection_name);
                    },
                    Err(e) => {
                        warn!("Failed to create {} collection: {}", collection_name, e);
                    }
                }
            } else {
                warn!("Invalid embedding head in config: '{}'", head_str);
            }
        }

        if stores.is_empty() {
            return Err(anyhow!("No valid Qdrant collections were initialized. Check MIRA_EMBED_HEADS in config."));
        }

        info!("Multi-collection Qdrant store initialized with {} collections", stores.len());
        
        // Verify we have all expected heads
        for expected in &[
            EmbeddingHead::Semantic, 
            EmbeddingHead::Code, 
            EmbeddingHead::Summary, 
            EmbeddingHead::Documents,
            EmbeddingHead::Relationship
        ] {
            if !stores.contains_key(expected) {
                warn!("Missing expected collection: {}", expected.as_str());
            }
        }
        
        Ok(Self { stores })
    }

    /// Save a memory entry to a specific collection
    /// Returns the point_id used in Qdrant (message_id as string)
    pub async fn save(&self, head: EmbeddingHead, entry: &MemoryEntry) -> Result<String> {
        debug!("Saving memory to {} collection", head.as_str());
        
        // Ensure the entry has an embedding
        if entry.embedding.is_none() {
            return Err(anyhow!("Cannot save to Qdrant without embedding"));
        }
        
        // Get the point_id that will be used (message_id as string)
        let point_id = entry.id
            .ok_or_else(|| anyhow!("Cannot save to Qdrant without message_id"))?
            .to_string();
        
        if let Some(store) = self.stores.get(&head) {
            store.save(entry).await?;
            Ok(point_id)
        } else {
            warn!("No Qdrant store found for embedding head: {}", head.as_str());
            Err(anyhow!("Collection {} not initialized", head.as_str()))
        }
    }

    /// Delete a memory entry from a specific collection by message_id
    pub async fn delete(&self, head: EmbeddingHead, message_id: i64) -> Result<()> {
        debug!("Deleting message {} from {} collection", message_id, head.as_str());
        
        if let Some(store) = self.stores.get(&head) {
            store.delete(message_id).await
        } else {
            warn!("No Qdrant store found for embedding head: {}", head.as_str());
            Ok(()) // Don't fail if collection doesn't exist
        }
    }

    /// Delete a memory entry from all collections
    pub async fn delete_from_all(&self, message_id: i64) -> Result<()> {
        debug!("Deleting message {} from all collections", message_id);
        
        for (head, store) in &self.stores {
            match store.delete(message_id).await {
                Ok(_) => debug!("Deleted message {} from {} collection", message_id, head.as_str()),
                Err(e) => warn!("Failed to delete message {} from {} collection: {}", message_id, head.as_str(), e),
            }
        }
        
        Ok(())
    }

    /// Get the Qdrant point ID for a message (just the message_id as string)
    pub fn get_point_id(message_id: i64) -> String {
        message_id.to_string()
    }

    /// Get collection name for a specific head
    pub fn get_collection_name(&self, head: EmbeddingHead) -> Option<String> {
        self.stores.get(&head).map(|store| store.collection_name.clone())
    }

    /// Search within a specific collection
    pub async fn search(
        &self,
        head: EmbeddingHead,
        session_id: &str,
        embedding: &[f32],
        k: usize,
    ) -> Result<Vec<MemoryEntry>> {
        debug!("Searching {} collection for session: {}", head.as_str(), session_id);
        
        if let Some(store) = self.stores.get(&head) {
            store.semantic_search(session_id, embedding, k).await
        } else {
            warn!("No Qdrant store found for embedding head: {}", head.as_str());
            Ok(Vec::new())
        }
    }

    /// Search across all enabled collections and combine results
    pub async fn search_all(
        &self,
        session_id: &str,
        embedding: &[f32],
        k_per_head: usize,
    ) -> Result<Vec<(EmbeddingHead, Vec<MemoryEntry>)>> {
        let mut results = Vec::new();
        let enabled_heads = self.get_enabled_heads();
        info!("Searching across {} enabled collections", enabled_heads.len());

        for head in enabled_heads {
            match self.search(head, session_id, embedding, k_per_head).await {
                Ok(entries) => {
                    debug!("Found {} entries in {} collection", entries.len(), head.as_str());
                    results.push((head, entries));
                }
                Err(e) => {
                    warn!("Failed to search {} collection: {}", head.as_str(), e);
                }
            }
        }
        Ok(results)
    }

    /// Parallel search across all collections (optimized version)
    pub async fn parallel_search(
        &self,
        session_id: &str,
        embedding: &[f32],
        k_per_head: usize,
    ) -> Result<HashMap<EmbeddingHead, Vec<MemoryEntry>>> {
        use futures::future::join_all;
        
        let mut search_futures = vec![];
        
        for (head, store) in &self.stores {
            let head = *head;
            let store = store.clone();
            let session_id = session_id.to_string();
            let embedding = embedding.to_vec();
            
            search_futures.push(async move {
                let results = store.semantic_search(&session_id, &embedding, k_per_head).await
                    .unwrap_or_else(|e| {
                        warn!("Search failed for {}: {}", head.as_str(), e);
                        vec![]
                    });
                (head, results)
            });
        }
        
        let results = join_all(search_futures).await;
        Ok(results.into_iter().collect())
    }

    /// Scroll through all points in a collection
    /// 
    /// Returns a list of point IDs as strings (message_ids)
    /// 
    /// # Arguments
    /// * `head` - Which collection to scroll
    /// * `offset` - Optional offset to start from (for pagination)
    /// * `limit` - How many points to return per scroll
    pub async fn scroll_collection(
        &self,
        head: EmbeddingHead,
        offset: Option<u64>,
        limit: usize,
    ) -> Result<Vec<String>> {
        let store = self.stores.get(&head)
            .ok_or_else(|| anyhow!("Collection for {} not initialized", head.as_str()))?;

        // Call the store's scroll method
        let point_ids = store.scroll_points(offset, limit).await?;
        
        // Convert u64 IDs to strings
        Ok(point_ids.into_iter().map(|id| id.to_string()).collect())
    }

    /// Scroll through ALL points in a collection (handles pagination automatically)
    /// 
    /// This is a convenience wrapper that scrolls through the entire collection
    /// by automatically handling pagination.
    pub async fn scroll_all_points(
        &self,
        head: EmbeddingHead,
    ) -> Result<Vec<String>> {
        let store = self.stores.get(&head)
            .ok_or_else(|| anyhow!("Collection for {} not initialized", head.as_str()))?;

        // Call the store's scroll_all method
        let point_ids = store.scroll_all_points().await?;
        
        // Convert u64 IDs to strings
        Ok(point_ids.into_iter().map(|id| id.to_string()).collect())
    }

    /// Get list of enabled embedding heads
    pub fn get_enabled_heads(&self) -> Vec<EmbeddingHead> {
        self.stores.keys().cloned().collect()
    }

    /// Get the default/primary store (semantic)
    pub fn get_semantic_store(&self) -> Option<&Arc<QdrantMemoryStore>> {
        self.stores.get(&EmbeddingHead::Semantic)
    }

    /// Get the code-specific store
    pub fn get_code_store(&self) -> Option<&Arc<QdrantMemoryStore>> {
        self.stores.get(&EmbeddingHead::Code)
    }
    
    /// Get the summary store
    pub fn get_summary_store(&self) -> Option<&Arc<QdrantMemoryStore>> {
        self.stores.get(&EmbeddingHead::Summary)
    }
    
    /// Get the documents store
    pub fn get_documents_store(&self) -> Option<&Arc<QdrantMemoryStore>> {
        self.stores.get(&EmbeddingHead::Documents)
    }
    
    /// Get the relationship store
    pub fn get_relationship_store(&self) -> Option<&Arc<QdrantMemoryStore>> {
        self.stores.get(&EmbeddingHead::Relationship)
    }

    /// Get store for a specific head
    pub fn get_store(&self, head: EmbeddingHead) -> Option<&Arc<QdrantMemoryStore>> {
        self.stores.get(&head)
    }

    /// Check if multi-head mode is enabled
    pub fn is_multi_head_enabled(&self) -> bool {
        // HARDCODED: Multi-head embedding is always enabled
        true  // was CONFIG.is_robust_memory_enabled()
    }

    /// Get collection information for debugging
    pub fn get_collection_info(&self) -> Vec<(EmbeddingHead, String)> {
        self.stores
            .iter()
            .map(|(head, store)| (*head, store.collection_name.clone()))
            .collect()
    }
    
    /// Verify all collections are healthy
    pub async fn health_check(&self) -> Result<HashMap<EmbeddingHead, bool>> {
        let mut health = HashMap::new();
        
        for (head, store) in &self.stores {
            // Try a simple operation to verify connectivity
            let is_healthy = store.semantic_search("health_check", &[0.0; 3072], 1)
                .await
                .is_ok();
            health.insert(*head, is_healthy);
        }
        
        Ok(health)
    }
}
