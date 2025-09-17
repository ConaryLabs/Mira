// src/memory/storage/qdrant/multi_store.rs  
// Qdrant multi-collection store for 4-head memory system

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
    /// Create a new multi-collection store with all 4 heads
    pub async fn new(base_url: &str, collection_base_name: &str) -> Result<Self> {
        info!("Initializing Qdrant multi-collection store with base: {}", collection_base_name);
        let mut stores = HashMap::new();
        let heads = CONFIG.get_embedding_heads();

        // Ensure we have all 4 heads
        if !heads.contains(&"documents".to_string()) {
            warn!("Documents head not found in config! Add 'documents' to MIRA_EMBED_HEADS");
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
        for expected in &[EmbeddingHead::Semantic, EmbeddingHead::Code, EmbeddingHead::Summary, EmbeddingHead::Documents] {
            if !stores.contains_key(expected) {
                warn!("Missing expected collection: {}", expected.as_str());
            }
        }
        
        Ok(Self { stores })
    }

    /// Save a memory entry to a specific collection
    pub async fn save(&self, head: EmbeddingHead, entry: &MemoryEntry) -> Result<()> {
        debug!("Saving memory to {} collection", head.as_str());
        
        // Ensure the entry has an embedding
        if entry.embedding.is_none() {
            return Err(anyhow!("Cannot save to Qdrant without embedding"));
        }
        
        if let Some(store) = self.stores.get(&head) {
            store.save(entry).await.map(|_| ())
        } else {
            warn!("No Qdrant store found for embedding head: {}", head.as_str());
            Err(anyhow!("Collection {} not initialized", head.as_str()))
        }
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

    /// Get store for a specific head
    pub fn get_store(&self, head: EmbeddingHead) -> Option<&Arc<QdrantMemoryStore>> {
        self.stores.get(&head)
    }

    /// Check if multi-head mode is enabled
    pub fn is_multi_head_enabled(&self) -> bool {
        CONFIG.is_robust_memory_enabled()
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
