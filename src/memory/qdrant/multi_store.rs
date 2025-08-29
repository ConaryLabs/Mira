// src/memory/qdrant/multi_store.rs
// PHASE 1: Multi-Collection Qdrant Support for GPT-5 Robust Memory
// Wraps multiple QdrantMemoryStore instances for semantic/code/summary heads

use anyhow::{Result, anyhow};
use std::sync::Arc;
use tracing::{info, debug, warn};

use crate::config::CONFIG;
use crate::memory::qdrant::store::QdrantMemoryStore;
use crate::memory::traits::MemoryStore;  // FIXED: Import trait for methods
use crate::memory::types::MemoryEntry;

/// Represents the different embedding heads/collections
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EmbeddingHead {
    Semantic,
    Code,
    Summary,
}

impl EmbeddingHead {
    /// Parse head name from string (case-insensitive)
    pub fn from_str(s: &str) -> Result<Self> {
        match s.trim().to_lowercase().as_str() {
            "semantic" => Ok(EmbeddingHead::Semantic),
            "code" => Ok(EmbeddingHead::Code),
            "summary" => Ok(EmbeddingHead::Summary),
            _ => Err(anyhow!("Invalid embedding head: '{}'. Must be one of: semantic, code, summary", s)),
        }
    }
    
    /// Get string representation
    pub fn as_str(&self) -> &'static str {
        match self {
            EmbeddingHead::Semantic => "semantic",
            EmbeddingHead::Code => "code",
            EmbeddingHead::Summary => "summary",
        }
    }
    
    /// Get collection name for this head based on configuration
    pub fn collection_name(&self) -> String {
        match self {
            EmbeddingHead::Semantic => {
                // For backward compatibility, semantic can use the default collection
                // or a specific semantic collection if multi-head is enabled
                if CONFIG.is_robust_memory_enabled() {
                    format!("{}-semantic", CONFIG.qdrant_collection)
                } else {
                    CONFIG.qdrant_collection.clone()
                }
            }
            EmbeddingHead::Code => format!("{}-code", CONFIG.qdrant_collection),
            EmbeddingHead::Summary => format!("{}-summary", CONFIG.qdrant_collection),
        }
    }
}

/// Multi-collection Qdrant store that routes operations to appropriate collections
pub struct QdrantMultiStore {
    semantic_store: Arc<QdrantMemoryStore>,
    code_store: Arc<QdrantMemoryStore>,
    summary_store: Arc<QdrantMemoryStore>,
    base_url: String,
}

impl QdrantMultiStore {
    /// Create a new multi-collection store
    pub async fn new(base_url: &str) -> Result<Self> {
        info!("🏗️  Initializing Qdrant multi-collection store");
        
        // Create collections for each head
        let semantic_collection = EmbeddingHead::Semantic.collection_name();
        let code_collection = EmbeddingHead::Code.collection_name();
        let summary_collection = EmbeddingHead::Summary.collection_name();
        
        debug!("Creating Qdrant stores:");
        debug!("  - Semantic: {}", semantic_collection);
        debug!("  - Code: {}", code_collection);
        debug!("  - Summary: {}", summary_collection);
        
        // Initialize all stores
        let semantic_store = Arc::new(
            QdrantMemoryStore::new(base_url, &semantic_collection).await?
        );
        
        let code_store = Arc::new(
            QdrantMemoryStore::new(base_url, &code_collection).await?
        );
        
        let summary_store = Arc::new(
            QdrantMemoryStore::new(base_url, &summary_collection).await?
        );
        
        info!("✅ Multi-collection Qdrant store initialized with {} collections", 
            if CONFIG.is_robust_memory_enabled() { 3 } else { 1 });
        
        Ok(Self {
            semantic_store,
            code_store,
            summary_store,
            base_url: base_url.to_string(),
        })
    }
    
    /// Get the appropriate store for a given head
    fn get_store_for_head(&self, head: EmbeddingHead) -> &Arc<QdrantMemoryStore> {
        match head {
            EmbeddingHead::Semantic => &self.semantic_store,
            EmbeddingHead::Code => &self.code_store,
            EmbeddingHead::Summary => &self.summary_store,
        }
    }
    
    /// Save a memory entry to a specific collection
    pub async fn save(&self, head: EmbeddingHead, entry: &MemoryEntry) -> Result<()> {
        debug!("💾 Saving memory to {} collection", head.as_str());
        
        // Create a copy of the entry with head metadata in payload
        let mut enhanced_entry = entry.clone();
        
        // Add head information to tags if not already present
        let head_tag = format!("head:{}", head.as_str());
        if let Some(ref mut tags) = enhanced_entry.tags {
            if !tags.contains(&head_tag) {
                tags.push(head_tag);
            }
        } else {
            enhanced_entry.tags = Some(vec![head_tag]);
        }
        
        // Save to the appropriate collection
        let store = self.get_store_for_head(head);
        store.save(&enhanced_entry).await
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
        
        let store = self.get_store_for_head(head);
        store.semantic_search(session_id, embedding, k).await
    }
    
    /// Search across all enabled collections and combine results
    pub async fn search_all(
        &self,
        session_id: &str,
        embedding: &[f32],
        k_per_head: usize,
    ) -> Result<Vec<(EmbeddingHead, Vec<MemoryEntry>)>> {
        let mut results = Vec::new();
        
        // Get enabled heads from configuration
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
                    // Continue with other collections even if one fails
                }
            }
        }
        
        Ok(results)
    }
    
    /// Get list of enabled embedding heads based on configuration
    pub fn get_enabled_heads(&self) -> Vec<EmbeddingHead> {
        if !CONFIG.is_robust_memory_enabled() {
            // When robust memory is disabled, only use semantic head
            return vec![EmbeddingHead::Semantic];
        }
        
        // Parse heads from configuration
        let head_names = CONFIG.get_embedding_heads();
        let mut heads = Vec::new();
        
        for name in head_names {
            match EmbeddingHead::from_str(&name) {
                Ok(head) => heads.push(head),
                Err(e) => {
                    warn!("Invalid embedding head in config: {}", e);
                }
            }
        }
        
        // Fallback to semantic if no valid heads found
        if heads.is_empty() {
            warn!("No valid embedding heads found in config, falling back to semantic");
            heads.push(EmbeddingHead::Semantic);
        }
        
        debug!("Enabled embedding heads: {:?}", heads);
        heads
    }
    
    /// Get the default/primary store (semantic)
    pub fn get_semantic_store(&self) -> &Arc<QdrantMemoryStore> {
        &self.semantic_store
    }
    
    /// Get store for a specific head (for backward compatibility)
    pub fn get_store(&self, head: EmbeddingHead) -> &Arc<QdrantMemoryStore> {
        self.get_store_for_head(head)
    }
    
    /// Check if multi-head mode is enabled
    pub fn is_multi_head_enabled(&self) -> bool {
        CONFIG.is_robust_memory_enabled()
    }
    
    /// Get collection information for debugging
    pub fn get_collection_info(&self) -> Vec<(EmbeddingHead, String)> {
        vec![
            (EmbeddingHead::Semantic, EmbeddingHead::Semantic.collection_name()),
            (EmbeddingHead::Code, EmbeddingHead::Code.collection_name()),
            (EmbeddingHead::Summary, EmbeddingHead::Summary.collection_name()),
        ]
    }
    
    /// Save to all enabled collections (Phase 1 behavior - same embedding for all)
    /// This will be enhanced in Phase 2 with different embeddings per head
    pub async fn save_to_all(&self, entry: &MemoryEntry) -> Result<()> {
        if !CONFIG.is_robust_memory_enabled() {
            // When robust memory is disabled, only save to semantic collection
            return self.save(EmbeddingHead::Semantic, entry).await;
        }
        
        let enabled_heads = self.get_enabled_heads();
        let mut errors = Vec::new();
        
        info!("💾 Saving memory to {} collections", enabled_heads.len());
        
        for head in enabled_heads {
            if let Err(e) = self.save(head, entry).await {
                warn!("Failed to save to {} collection: {}", head.as_str(), e);
                errors.push((head, e));
            }
        }
        
        // Return error if all saves failed, otherwise succeed
        if errors.len() == self.get_enabled_heads().len() {
            return Err(anyhow!("Failed to save to all collections: {:?}", errors));
        }
        
        if !errors.is_empty() {
            warn!("Partial save failure: {} out of {} collections failed", 
                errors.len(), self.get_enabled_heads().len());
        }
        
        Ok(())
    }
    
    /// PHASE 1: Create a multi-store that uses a single store for all collections
    /// (Backward compatibility helper for gradual migration)
    pub fn from_single_store(single_store: Arc<QdrantMemoryStore>) -> Self {
        Self {
            semantic_store: single_store.clone(),
            code_store: single_store.clone(), 
            summary_store: single_store,
            base_url: "compatibility-mode".to_string(),
        }
    }
}

// For debugging and testing
impl std::fmt::Debug for QdrantMultiStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QdrantMultiStore")
            .field("base_url", &self.base_url)
            .field("multi_head_enabled", &self.is_multi_head_enabled())
            .field("enabled_heads", &self.get_enabled_heads())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_embedding_head_parsing() {
        assert_eq!(EmbeddingHead::from_str("semantic").unwrap(), EmbeddingHead::Semantic);
        assert_eq!(EmbeddingHead::from_str("SEMANTIC").unwrap(), EmbeddingHead::Semantic);
        assert_eq!(EmbeddingHead::from_str("code").unwrap(), EmbeddingHead::Code);
        assert_eq!(EmbeddingHead::from_str("summary").unwrap(), EmbeddingHead::Summary);
        
        assert!(EmbeddingHead::from_str("invalid").is_err());
        assert!(EmbeddingHead::from_str("").is_err());
    }
    
    #[test]
    fn test_embedding_head_string_conversion() {
        assert_eq!(EmbeddingHead::Semantic.as_str(), "semantic");
        assert_eq!(EmbeddingHead::Code.as_str(), "code");
        assert_eq!(EmbeddingHead::Summary.as_str(), "summary");
    }
    
    #[test]
    fn test_collection_names() {
        // Note: These tests assume default config values
        // Collection names will be based on CONFIG.qdrant_collection
        let semantic_name = EmbeddingHead::Semantic.collection_name();
        let code_name = EmbeddingHead::Code.collection_name();
        let summary_name = EmbeddingHead::Summary.collection_name();
        
        // Should have different names
        assert_ne!(semantic_name, code_name);
        assert_ne!(semantic_name, summary_name);
        assert_ne!(code_name, summary_name);
        
        // Should contain the head type
        assert!(code_name.contains("code"));
        assert!(summary_name.contains("summary"));
    }
}
