use std::sync::Arc;
use anyhow::Result;
use chrono::Utc;
use tracing::{info, warn};
use crate::llm::client::OpenAIClient;
use crate::llm::embeddings::EmbeddingHead;
use crate::memory::core::types::MemoryEntry;
use crate::memory::core::traits::MemoryStore;
use crate::memory::storage::sqlite::store::SqliteMemoryStore;
use crate::memory::storage::qdrant::multi_store::QdrantMultiStore;
use crate::memory::features::memory_types::SummaryType;
use crate::config::CONFIG;

/// Handles all summary storage operations (SQLite + Qdrant)
pub struct SummaryStorage {
    llm_client: Arc<OpenAIClient>,
    sqlite_store: Arc<SqliteMemoryStore>,
    multi_store: Arc<QdrantMultiStore>,
}

impl SummaryStorage {
    pub fn new(
        llm_client: Arc<OpenAIClient>,
        sqlite_store: Arc<SqliteMemoryStore>,
        multi_store: Arc<QdrantMultiStore>,
    ) -> Self {
        Self {
            llm_client,
            sqlite_store,
            multi_store,
        }
    }

    /// Stores summary in both SQLite and Qdrant
    pub async fn store_summary(
        &self,
        session_id: &str,
        summary: &str,
        summary_type: SummaryType,
        message_count: usize,
    ) -> Result<()> {
        // Create memory entry for the summary
        let entry = self.create_summary_entry(
            session_id.to_string(),
            summary.to_string(),
            summary_type,
            message_count,
        );

        // Save to SQLite first
        let saved = self.sqlite_store.save(&entry).await?;
        let summary_id = saved.id.unwrap_or(0);
        
        info!("Stored summary {} in SQLite", summary_id);

        // Generate embedding and store in Qdrant if configured
        if CONFIG.embed_heads.contains("summary") {
            match self.llm_client.get_embedding(&saved.content).await {
                Ok(embedding) => {
                    let mut entry_with_embedding = saved;
                    entry_with_embedding.embedding = Some(embedding);
                    
                    self.multi_store
                        .save(EmbeddingHead::Summary, &entry_with_embedding)
                        .await?;
                    
                    info!("Stored summary {} in Qdrant Summary collection", summary_id);
                }
                Err(e) => {
                    warn!("Failed to generate embedding for summary: {}", e);
                    // Don't fail the whole operation if embedding fails
                }
            }
        }

        Ok(())
    }

    fn create_summary_entry(
        &self,
        session_id: String,
        summary: String,
        summary_type: SummaryType,
        _message_count: usize,
    ) -> MemoryEntry {
        let type_tag = match summary_type {
            SummaryType::Rolling10 => "summary:rolling:10",
            SummaryType::Rolling100 => "summary:rolling:100",
            SummaryType::Snapshot => "summary:snapshot",
        };
        
        MemoryEntry {
            id: None,
            session_id,
            response_id: None,
            parent_id: None,
            role: "system".to_string(),
            content: summary.clone(),
            timestamp: Utc::now(),
            tags: Some(vec![
                "summary".to_string(),
                type_tag.to_string(),
                "system".to_string(),
            ]),
            
            // Analysis fields
            mood: None,
            intensity: None,
            salience: Some(10.0),  // Summaries have max salience
            intent: None,
            topics: None,
            summary: Some(summary),
            relationship_impact: None,
            contains_code: Some(false),
            language: None,
            programming_lang: None,
            analyzed_at: None,
            analysis_version: None,
            routed_to_heads: None,
            last_recalled: Some(Utc::now()),
            recall_count: None,
            
            // GPT5 metadata fields - all None for summaries
            model_version: None,
            prompt_tokens: None,
            completion_tokens: None,
            reasoning_tokens: None,
            total_tokens: None,
            latency_ms: None,
            generation_time_ms: None,
            finish_reason: None,
            tool_calls: None,
            temperature: None,
            max_tokens: None,
            reasoning_effort: None,
            verbosity: None,
            
            // Embedding info
            embedding: None,
            embedding_heads: Some(vec!["summary".to_string()]),
            qdrant_point_ids: None,
        }
    }
}
