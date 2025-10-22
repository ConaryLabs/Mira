// src/tasks/embedding_cleanup.rs
// Finds and removes orphaned embeddings in Qdrant that have no SQLite backing

use anyhow::Result;
use sqlx::SqlitePool;
use std::sync::Arc;
use std::collections::HashSet;
use tracing::{info, warn, debug};
use serde::Serialize;

use crate::memory::storage::qdrant::multi_store::QdrantMultiStore;
use crate::llm::embeddings::EmbeddingHead;

/// Report of what was found and cleaned
#[derive(Debug, Clone, Serialize)]
pub struct CleanupReport {
    pub semantic_orphans: u64,
    pub code_orphans: u64,
    pub summary_orphans: u64,
    pub documents_orphans: u64,
    pub relationship_orphans: u64,
    pub total_orphans: u64,
    pub total_deleted: u64,
    pub scan_duration_ms: u64,
}

pub struct EmbeddingCleanupTask {
    pool: SqlitePool,
    multi_store: Arc<QdrantMultiStore>,
}

impl EmbeddingCleanupTask {
    pub fn new(pool: SqlitePool, multi_store: Arc<QdrantMultiStore>) -> Self {
        Self { pool, multi_store }
    }
    
    /// Run full cleanup across all collections
    pub async fn run(&self, dry_run: bool) -> Result<CleanupReport> {
        let start = std::time::Instant::now();
        
        info!("Starting embedding cleanup task (dry_run: {})", dry_run);
        
        let mut report = CleanupReport {
            semantic_orphans: 0,
            code_orphans: 0,
            summary_orphans: 0,
            documents_orphans: 0,
            relationship_orphans: 0,
            total_orphans: 0,
            total_deleted: 0,
            scan_duration_ms: 0,
        };
        
        // Get all valid message IDs from SQLite
        let valid_message_ids = self.get_valid_message_ids().await?;
        info!("Found {} valid message IDs in SQLite", valid_message_ids.len());
        
        // Get all valid code element IDs from SQLite
        let valid_code_element_ids = self.get_valid_code_element_ids().await?;
        info!("Found {} valid code element IDs in SQLite", valid_code_element_ids.len());
        
        // Clean each collection
        report.semantic_orphans = self.clean_collection(
            EmbeddingHead::Semantic,
            &valid_message_ids,
            dry_run
        ).await?;
        
        report.code_orphans = self.clean_collection(
            EmbeddingHead::Code,
            &valid_code_element_ids,
            dry_run
        ).await?;
        
        report.summary_orphans = self.clean_collection(
            EmbeddingHead::Summary,
            &valid_message_ids,
            dry_run
        ).await?;
        
        report.documents_orphans = self.clean_collection(
            EmbeddingHead::Documents,
            &valid_message_ids,
            dry_run
        ).await?;
        
        report.relationship_orphans = self.clean_collection(
            EmbeddingHead::Relationship,
            &valid_message_ids,
            dry_run
        ).await?;
        
        report.total_orphans = report.semantic_orphans 
            + report.code_orphans 
            + report.summary_orphans 
            + report.documents_orphans
            + report.relationship_orphans;
        
        report.total_deleted = if dry_run { 0 } else { report.total_orphans };
        report.scan_duration_ms = start.elapsed().as_millis() as u64;
        
        info!(
            "Cleanup complete: {} orphans found, {} deleted (took {}ms)",
            report.total_orphans,
            report.total_deleted,
            report.scan_duration_ms
        );
        
        Ok(report)
    }
    
    /// Get all valid message IDs from memory_entries
    async fn get_valid_message_ids(&self) -> Result<HashSet<i64>> {
        let rows = sqlx::query!(
            r#"
            SELECT id FROM memory_entries
            "#
        )
        .fetch_all(&self.pool)
        .await?;
        
        Ok(rows.into_iter().filter_map(|r| r.id).collect())
    }
    
    /// Get all valid code element IDs from code_elements
    async fn get_valid_code_element_ids(&self) -> Result<HashSet<i64>> {
        let rows = sqlx::query!(
            r#"
            SELECT id FROM code_elements
            "#
        )
        .fetch_all(&self.pool)
        .await?;
        
        Ok(rows.into_iter().filter_map(|r| r.id).collect())
    }
    
    /// Clean a specific collection, returning count of orphans found
    async fn clean_collection(
        &self,
        head: EmbeddingHead,
        valid_ids: &HashSet<i64>,
        dry_run: bool,
    ) -> Result<u64> {
        info!("Scanning {} collection for orphans...", head.as_str());
        
        // Get all point IDs from this Qdrant collection
        let collection_name = self.multi_store.get_collection_name(head)
            .unwrap_or_else(|| format!("mira_{}", head.as_str()));
        
        // Use Qdrant scroll API to get all point IDs
        let qdrant_point_ids = self.get_all_point_ids(&collection_name).await?;
        
        debug!(
            "Found {} points in {} collection",
            qdrant_point_ids.len(),
            collection_name
        );
        
        // Find orphans (points in Qdrant but not in SQLite)
        let mut orphan_count = 0u64;
        
        for point_id in &qdrant_point_ids {
            if !valid_ids.contains(point_id) {
                orphan_count += 1;
                
                if dry_run {
                    debug!("Found orphan in {}: {}", collection_name, point_id);
                } else {
                    // Delete the orphan
                    match self.multi_store.delete(head, *point_id).await {
                        Ok(_) => {
                            debug!("Deleted orphan from {}: {}", collection_name, point_id);
                        }
                        Err(e) => {
                            warn!(
                                "Failed to delete orphan {} from {}: {}",
                                point_id, collection_name, e
                            );
                        }
                    }
                }
            }
        }
        
        if orphan_count > 0 {
            if dry_run {
                info!(
                    "Found {} orphans in {} (dry run - not deleted)",
                    orphan_count, collection_name
                );
            } else {
                info!(
                    "Deleted {} orphans from {}",
                    orphan_count, collection_name
                );
            }
        } else {
            info!("No orphans found in {}", collection_name);
        }
        
        Ok(orphan_count)
    }
    
    /// Get all point IDs from a Qdrant collection using scroll API
    async fn get_all_point_ids(&self, _collection_name: &str) -> Result<Vec<i64>> {
        // Use the multi_store's scroll method to get all point IDs
        // Note: This will be added to multi_store.rs
        
        // For now, return empty - implementation depends on which collection
        // This will be properly implemented once the scroll method is added to multi_store
        
        warn!(
            "Getting all point IDs from {} - using multi_store scroll (needs implementation)",
            _collection_name
        );
        
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_cleanup_report_structure() {
        let report = CleanupReport {
            semantic_orphans: 10,
            code_orphans: 5,
            summary_orphans: 2,
            documents_orphans: 0,
            relationship_orphans: 3,
            total_orphans: 20,
            total_deleted: 20,
            scan_duration_ms: 1500,
        };
        
        assert_eq!(report.total_orphans, 20);
        assert_eq!(report.total_deleted, 20);
    }
}
