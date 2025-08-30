// src/memory/qdrant/multi_store.rs
// PHASE 1: Qdrant multi-collection store for robust memory
// PHASE 2: Updated constructor to accept a collection base name

use std::{collections::HashMap, sync::Arc};

use anyhow::{anyhow, Result};
use tracing::{debug, info, warn};

use crate::{
    config::CONFIG,
    llm::embeddings::EmbeddingHead, // PHASE 2: Import the canonical EmbeddingHead
    memory::{qdrant::store::QdrantMemoryStore, traits::MemoryStore, types::MemoryEntry},
};

/// Multi-collection Qdrant store that routes operations to appropriate collections
#[derive(Debug)]
pub struct QdrantMultiStore {
    stores: HashMap<EmbeddingHead, Arc<QdrantMemoryStore>>,
}

impl QdrantMultiStore {
    /// Create a new multi-collection store
    pub async fn new(base_url: &str, collection_base_name: &str) -> Result<Self> {
        info!("🏗️ Initializing Qdrant multi-collection store with base: {}", collection_base_name);
        let mut stores = HashMap::new();
        let heads = CONFIG.get_embedding_heads();

        for head_str in &heads {
            if let Ok(head) = head_str.parse::<EmbeddingHead>() {
                // Corrected: Use the provided base name to construct the collection name
                let collection_name = format!("{}-{}", collection_base_name, head.as_str());
                info!(
                    "Initializing Qdrant collection for head '{}': {}",
                    head.as_str(),
                    collection_name
                );
                let store = QdrantMemoryStore::new(base_url, &collection_name).await?;
                stores.insert(head, Arc::new(store));
            } else {
                warn!("Invalid embedding head in config: '{}'", head_str);
            }
        }

        if stores.is_empty() {
            return Err(anyhow!("No valid Qdrant collections were initialized. Check MIRA_EMBED_HEADS in config."));
        }

        info!("✅ Multi-collection Qdrant store initialized with {} collections", stores.len());
        Ok(Self { stores })
    }

    /// Creates a compatibility wrapper from a single, existing Qdrant store.
    pub fn from_single_store(single_store: Arc<QdrantMemoryStore>) -> Self {
        let mut stores = HashMap::new();
        stores.insert(EmbeddingHead::Semantic, single_store);
        Self { stores }
    }

    /// Save a memory entry to a specific collection
    pub async fn save(&self, head: EmbeddingHead, entry: &MemoryEntry) -> Result<()> {
        debug!("💾 Saving memory to {} collection", head.as_str());
        if let Some(store) = self.stores.get(&head) {
            store.save(entry).await.map(|_| ())
        } else {
            warn!("No Qdrant store found for embedding head: {}", head.as_str());
            Ok(())
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
        debug!("🔍 Searching {} collection for session: {}", head.as_str(), session_id);
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
        info!("🔍 Searching across {} enabled collections", enabled_heads.len());

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

    /// Get list of enabled embedding heads based on configuration
    pub fn get_enabled_heads(&self) -> Vec<EmbeddingHead> {
        self.stores.keys().cloned().collect()
    }

    /// Get the default/primary store (semantic)
    pub fn get_semantic_store(&self) -> Option<&Arc<QdrantMemoryStore>> {
        self.stores.get(&EmbeddingHead::Semantic)
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
}
