// src/tasks/embedding_cleanup.rs
//
// Finds and removes orphaned Qdrant entries that no longer have corresponding SQLite records.
// This handles the case where messages get deleted from SQLite but their embeddings remain in Qdrant.

use std::sync::Arc;
use std::collections::HashMap;
use anyhow::{Result, Context};
use tracing::{info, warn, error};
use sqlx::SqlitePool;

use crate::memory::storage::qdrant::multi_store::QdrantMultiStore;
use crate::llm::embeddings::EmbeddingHead;

/// Report of cleanup operation
#[derive(Debug, Clone)]
pub struct CleanupReport {
    pub total_checked: usize,
    pub orphans_found: usize,
    pub orphans_deleted: usize,
    pub errors: Vec<String>,
    pub by_collection: HashMap<String, CollectionReport>,
}

#[derive(Debug, Clone)]
pub struct CollectionReport {
    pub checked: usize,
    pub orphans: usize,
    pub deleted: usize,
}

impl CleanupReport {
    pub fn new() -> Self {
        Self {
            total_checked: 0,
            orphans_found: 0,
            orphans_deleted: 0,
            errors: Vec::new(),
            by_collection: HashMap::new(),
        }
    }

    pub fn add_collection(&mut self, collection: String, report: CollectionReport) {
        self.total_checked += report.checked;
        self.orphans_found += report.orphans;
        self.orphans_deleted += report.deleted;
        self.by_collection.insert(collection, report);
    }

    pub fn summary(&self) -> String {
        format!(
            "Checked: {} | Found: {} orphans | Deleted: {} | Errors: {}",
            self.total_checked,
            self.orphans_found,
            self.orphans_deleted,
            self.errors.len()
        )
    }
}

/// Task for cleaning up orphaned embeddings
pub struct EmbeddingCleanupTask {
    pool: Arc<SqlitePool>,
    multi_store: Arc<QdrantMultiStore>,
}

impl EmbeddingCleanupTask {
    pub fn new(pool: Arc<SqlitePool>, multi_store: Arc<QdrantMultiStore>) -> Self {
        Self { pool, multi_store }
    }

    /// Run the cleanup task
    /// 
    /// # Arguments
    /// * `dry_run` - If true, only reports orphans without deleting them
    pub async fn run(&self, dry_run: bool) -> Result<CleanupReport> {
        info!("Starting embedding cleanup task (dry_run: {})", dry_run);
        
        let mut report = CleanupReport::new();
        
        // Get all enabled embedding heads
        let heads = self.multi_store.get_enabled_heads();
        
        for head in heads {
            info!("Checking {} collection for orphans", head.as_str());
            
            match self.clean_collection(head, dry_run).await {
                Ok(collection_report) => {
                    info!(
                        "{} collection: {} orphans found",
                        head.as_str(),
                        collection_report.orphans
                    );
                    report.add_collection(head.as_str().to_string(), collection_report);
                }
                Err(e) => {
                    error!("Failed to clean {} collection: {}", head.as_str(), e);
                    report.errors.push(format!("{}: {}", head.as_str(), e));
                }
            }
        }
        
        info!("Cleanup task complete: {}", report.summary());
        Ok(report)
    }

    /// Clean a specific collection
    async fn clean_collection(
        &self,
        head: EmbeddingHead,
        dry_run: bool,
    ) -> Result<CollectionReport> {
        let mut report = CollectionReport {
            checked: 0,
            orphans: 0,
            deleted: 0,
        };

        // Scroll through all points in this collection
        let all_points = self.multi_store
            .scroll_all_points(head)
            .await
            .context("Failed to scroll collection")?;

        report.checked = all_points.len();

        // Check each point against SQLite
        let mut orphan_ids = Vec::new();
        
        for point_id in all_points {
            // Point ID should be the message_id as a string
            let message_id: i64 = match point_id.parse() {
                Ok(id) => id,
                Err(_) => {
                    warn!("Invalid point ID format: {}", point_id);
                    continue;
                }
            };

            // Check if this message_id exists in SQLite
            let exists = self.check_message_exists(message_id).await?;
            
            if !exists {
                orphan_ids.push(message_id);
                report.orphans += 1;
            }
        }

        // Delete orphans if not a dry run
        if !dry_run && !orphan_ids.is_empty() {
            info!("Deleting {} orphans from {} collection", orphan_ids.len(), head.as_str());
            
            for message_id in orphan_ids {
                match self.multi_store.delete(head, message_id).await {
                    Ok(_) => {
                        report.deleted += 1;
                    }
                    Err(e) => {
                        warn!("Failed to delete point {}: {}", message_id, e);
                    }
                }
            }
        }

        Ok(report)
    }

    /// Check if a message exists in SQLite
    async fn check_message_exists(&self, message_id: i64) -> Result<bool> {
        let result = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM memory_entries WHERE id = ?"
        )
        .bind(message_id)
        .fetch_one(&*self.pool)
        .await?;

        Ok(result > 0)
    }
}
