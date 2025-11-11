// src/memory/features/recall_engine/search/recent_search.rs

//! Recent message search - focused on SQLite recent queries only.
//! 
//! Single responsibility: retrieve recent messages with recency-based scoring.

use std::sync::Arc;
use anyhow::Result;
use tracing::debug;
use chrono::{DateTime, Utc};

use crate::memory::{
    core::types::MemoryEntry,
    core::traits::MemoryStore,
    storage::sqlite::store::SqliteMemoryStore,
};
use super::super::{ScoredMemory};

#[derive(Clone)]
pub struct RecentSearch {
    sqlite_store: Arc<SqliteMemoryStore>,
}

impl RecentSearch {
    pub fn new(sqlite_store: Arc<SqliteMemoryStore>) -> Self {
        Self { sqlite_store }
    }

    /// Search recent messages - clean, focused implementation
    pub async fn search(
        &self,
        session_id: &str,
        limit: usize,
    ) -> Result<Vec<ScoredMemory>> {
        debug!("RecentSearch: Searching {} recent messages for session {}", limit, session_id);
        
        // Load recent entries from SQLite
        let entries = self.sqlite_store.load_recent(session_id, limit).await?;
        
        // Convert to scored entries with recency-only scoring
        let now = Utc::now();
        let scored: Vec<ScoredMemory> = entries
            .into_iter()
            .map(|entry| {
                let recency_score = self.calculate_recency_score(&entry, now);
                ScoredMemory {
                    entry,
                    score: recency_score, // Only recency matters for this search
                    recency_score,
                    similarity_score: 0.0, // Not applicable for recent search
                    salience_score: 0.0,   // Not used in recent-only search
                }
            })
            .collect();
        
        debug!("RecentSearch: Found {} recent messages", scored.len());
        Ok(scored)
    }
    
    /// Calculate recency score using exponential decay (same algorithm as before)
    fn calculate_recency_score(&self, entry: &MemoryEntry, now: DateTime<Utc>) -> f32 {
        let age_hours = (now - entry.timestamp).num_hours() as f32;
        (-age_hours / 24.0).exp() // Exponential decay over days
    }
}
