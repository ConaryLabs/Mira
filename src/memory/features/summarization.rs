// src/memory/features/summarization.rs
// Single source of truth for all summarization operations

use std::sync::Arc;
use anyhow::Result;
use chrono::Utc;
use tracing::{info, debug, warn};
use crate::llm::client::OpenAIClient;
use crate::llm::embeddings::EmbeddingHead;
use crate::memory::core::types::MemoryEntry;
use crate::memory::core::traits::MemoryStore;
use crate::memory::storage::sqlite::store::SqliteMemoryStore;
use crate::memory::storage::qdrant::multi_store::QdrantMultiStore;
use crate::config::CONFIG;
use crate::memory::features::memory_types::{SummaryRequest, SummaryType};

/// Manages ALL summarization operations - checking, creating, and storing
pub struct SummarizationEngine {
    llm_client: Arc<OpenAIClient>,
    sqlite_store: Arc<SqliteMemoryStore>,
    multi_store: Arc<QdrantMultiStore>,
    rolling_10_enabled: bool,
    rolling_100_enabled: bool,
}

impl SummarizationEngine {
    /// Creates a new summarization engine with all dependencies
    pub fn new(
        llm_client: Arc<OpenAIClient>,
        sqlite_store: Arc<SqliteMemoryStore>,
        multi_store: Arc<QdrantMultiStore>,
    ) -> Self {
        Self {
            llm_client,
            sqlite_store,
            multi_store,
            rolling_10_enabled: CONFIG.rolling_10_enabled(),
            rolling_100_enabled: CONFIG.rolling_100_enabled(),
        }
    }
    
    /// Main entry point for task manager - checks and creates if needed
    pub async fn check_and_process_summaries(
        &self,
        session_id: &str,
        message_count: usize,
    ) -> Result<Option<String>> {
        // Check if we should create a summary
        let summary_type = self.should_create_summary(message_count)?;
        
        if let Some(summary_type) = summary_type {
            let window_size = match summary_type {
                SummaryType::Rolling10 => 10,
                SummaryType::Rolling100 => 100,
                SummaryType::Snapshot => return Ok(None), // Snapshots are manual only
            };
            
            info!("Creating {}-message rolling summary for session {}", 
                  window_size, session_id);
            
            // Load messages
            let messages = self.sqlite_store
                .load_recent(session_id, window_size)
                .await?;
            
            if messages.len() < window_size / 2 {
                debug!("Not enough messages for summary ({} < {})", 
                       messages.len(), window_size / 2);
                return Ok(None);
            }
            
            // Create and store the summary
            self.create_and_store_summary(session_id, &messages, summary_type).await?;
            
            Ok(Some(format!("Created {}-message summary", window_size)))
        } else {
            Ok(None)
        }
    }
    
    /// Manual trigger for rolling summary (called by API/WebSocket)
    pub async fn create_rolling_summary(
        &self,
        session_id: &str,
        window_size: usize,
    ) -> Result<String> {
        let summary_type = if window_size == 100 {
            SummaryType::Rolling100
        } else {
            SummaryType::Rolling10
        };
        
        let messages = self.sqlite_store
            .load_recent(session_id, window_size)
            .await?;
        
        if messages.is_empty() {
            return Err(anyhow::anyhow!("No messages to summarize"));
        }
        
        self.create_and_store_summary(session_id, &messages, summary_type).await?;
        
        Ok(format!("Created {}-message rolling summary", window_size))
    }
    
    /// Manual trigger for snapshot summary (called by API/WebSocket)
    pub async fn create_snapshot_summary(
        &self,
        session_id: &str,
        max_tokens: Option<usize>,
    ) -> Result<String> {
        let messages = self.sqlite_store
            .load_recent(session_id, 50)
            .await?;
        
        if messages.is_empty() {
            return Err(anyhow::anyhow!("No messages to summarize"));
        }
        
        // Build and generate summary
        let content = self.build_summary_content(&messages)?;
        let token_limit = max_tokens.unwrap_or(1000);
        
        let prompt = format!(
            "Create a comprehensive snapshot of this conversation. \
            Include all key topics, decisions, and important context:\n\n{}",
            content
        );
        
        let summary = self.llm_client
            .summarize_conversation(&prompt, token_limit)
            .await?;
        
        // Create and store
        let mut entry = self.create_summary_entry(
            session_id.to_string(),
            summary.clone(),
            messages.len(),
            SummaryType::Snapshot,
        );
        
        self.store_summary(entry).await?;
        
        Ok(summary)
    }
    
    // ===== PRIVATE IMPLEMENTATION DETAILS =====
    
    /// Determines if a summary should be created based on message count
    fn should_create_summary(&self, message_count: usize) -> Result<Option<SummaryType>> {
        if message_count == 0 {
            return Ok(None);
        }
        
        if self.rolling_100_enabled && message_count % 100 == 0 {
            return Ok(Some(SummaryType::Rolling100));
        }
        
        if self.rolling_10_enabled && message_count % 10 == 0 {
            return Ok(Some(SummaryType::Rolling10));
        }
        
        Ok(None)
    }
    
    /// Core logic - creates summary and stores it everywhere needed
    async fn create_and_store_summary(
        &self,
        session_id: &str,
        messages: &[MemoryEntry],
        summary_type: SummaryType,
    ) -> Result<()> {
        // Build content excluding existing summaries
        let content = self.build_summary_content(messages)?;
        
        if content.is_empty() {
            return Err(anyhow::anyhow!("No content to summarize after filtering"));
        }
        
        // Generate summary with LLM
        let window_size = match summary_type {
            SummaryType::Rolling10 => 10,
            SummaryType::Rolling100 => 100,
            SummaryType::Snapshot => messages.len(),
        };
        
        let prompt = self.build_summary_prompt(&content, window_size);
        let token_limit = self.get_token_limit(&summary_type);
        
        debug!("Generating summary with {} token limit", token_limit);
        let summary = self.llm_client
            .summarize_conversation(&prompt, token_limit)
            .await?;
        
        // Create entry
        let entry = self.create_summary_entry(
            session_id.to_string(),
            summary,
            window_size,
            summary_type,
        );
        
        // Store in all locations
        self.store_summary(entry).await?;
        
        Ok(())
    }
    
    /// Stores summary in SQLite and optionally Qdrant
    async fn store_summary(&self, mut entry: MemoryEntry) -> Result<()> {
        // Save to SQLite first
        let saved = self.sqlite_store.save(&entry).await?;
        let summary_id = saved.id.unwrap_or(0);
        
        info!("Stored summary {} in SQLite", summary_id);
        
        // Generate embedding and store in Qdrant if configured
        if CONFIG.embed_heads.contains("summary") {
            match self.llm_client.get_embedding(&saved.content).await {
                Ok(embedding) => {
                    entry.id = saved.id;
                    entry.embedding = Some(embedding);
                    
                    self.multi_store
                        .save(EmbeddingHead::Summary, &entry)
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
    
    /// Builds content string from messages, excluding existing summaries
    fn build_summary_content(&self, messages: &[MemoryEntry]) -> Result<String> {
        let mut content = String::new();
        let mut included_count = 0;
        
        for msg in messages.iter().rev() {
            // Skip existing summaries to avoid recursive summarization
            if let Some(ref tags) = msg.tags {
                if tags.iter().any(|t| t.contains("summary")) {
                    debug!("Skipping existing summary in content building");
                    continue;
                }
            }
            
            content.push_str(&format!("{}: {}\n", msg.role, msg.content));
            included_count += 1;
        }
        
        debug!("Built summary content from {} messages", included_count);
        Ok(content)
    }
    
    /// Builds the prompt for summary generation
    fn build_summary_prompt(&self, content: &str, window_size: usize) -> String {
        match window_size {
            100 => format!(
                "Create a comprehensive mega-summary of the last {} messages. \
                Focus on key themes, important decisions, and critical information. \
                Preserve context and maintain continuity:\n\n{}",
                window_size, content
            ),
            10 => format!(
                "Create a concise rolling summary of the last {} messages. \
                Capture key points and maintain conversation context:\n\n{}",
                window_size, content
            ),
            _ => format!(
                "Summarize the last {} messages, preserving important details:\n\n{}",
                window_size, content
            ),
        }
    }
    
    /// Determines token limit based on summary type
    fn get_token_limit(&self, summary_type: &SummaryType) -> usize {
        match summary_type {
            SummaryType::Rolling100 => 800,
            SummaryType::Rolling10 => 500,
            SummaryType::Snapshot => 1000,
        }
    }
    
    /// Creates a MemoryEntry for the summary
    fn create_summary_entry(
        &self,
        session_id: String,
        summary: String,
        window_size: usize,
        summary_type: SummaryType,
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
            
            // GPT5 metadata fields
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
    
    /// Gets summary statistics for monitoring
    pub fn get_stats(&self) -> String {
        format!(
            "Summarization Config - 10-msg: {}, 100-msg: {}",
            if self.rolling_10_enabled { "enabled" } else { "disabled" },
            if self.rolling_100_enabled { "enabled" } else { "disabled" }
        )
    }
}
