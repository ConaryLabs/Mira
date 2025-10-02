// src/memory/storage/sqlite/core/embedding_operations.rs

use anyhow::Result;
use sqlx::SqlitePool;
use tracing::debug;

/// Handles embedding reference management and utilities
pub struct EmbeddingOperations {
    #[allow(dead_code)]  // Reserved for future embedding reference tracking
    pool: SqlitePool,
}

impl EmbeddingOperations {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
    
    /// Store embedding reference (future: track which Qdrant collections contain this message)
    pub async fn store_embedding_reference(&self, message_id: i64, embedding_heads: &[String]) -> Result<()> {
        // Future: track which Qdrant collections contain embeddings for this message
        // For now, this is handled by the routed_to_heads field in message_analysis
        debug!("Embedding reference for message {}: {:?}", message_id, embedding_heads);
        Ok(())
    }
    
    /// Helper: Convert Vec<f32> to BLOB for SQLite storage
    pub fn embedding_to_blob(embedding: &Option<Vec<f32>>) -> Option<Vec<u8>> {
        embedding.as_ref().map(|vec| {
            vec.iter()
                .flat_map(|f| f.to_le_bytes())
                .collect::<Vec<u8>>()
        })
    }
    
    /// Helper: Convert BLOB to Vec<f32> from SQLite
    pub fn blob_to_embedding(blob: Option<Vec<u8>>) -> Option<Vec<f32>> {
        blob.map(|bytes| {
            bytes
                .chunks_exact(4)
                .map(|chunk| f32::from_le_bytes(chunk.try_into().unwrap()))
                .collect()
        })
    }
}
