// src/memory/features/summarization.rs
// Rolling summaries and snapshot generation for memory compression

use std::sync::Arc;
use anyhow::Result;
use chrono::Utc;
use tracing::{info, debug};
use crate::llm::client::OpenAIClient;
use crate::memory::core::types::MemoryEntry;
use crate::config::CONFIG;
use crate::memory::features::memory_types::{SummaryRequest, SummaryType};

/// Manages rolling summaries and memory compression
pub struct SummarizationEngine {
    llm_client: Arc<OpenAIClient>,
    rolling_10_enabled: bool,
    rolling_100_enabled: bool,
    auto_pinning: bool,
}

impl SummarizationEngine {
    /// Creates a new summarization engine with default configuration
    pub fn new(llm_client: Arc<OpenAIClient>) -> Self {
        Self {
            llm_client,
            rolling_10_enabled: CONFIG.rolling_10_enabled(),
            rolling_100_enabled: CONFIG.rolling_100_enabled(),
            auto_pinning: true,  // Summaries are auto-pinned by default
        }
    }
    
    /// Creates engine with custom configuration
    pub fn with_config(
        llm_client: Arc<OpenAIClient>,
        enable_10: bool,
        enable_100: bool,
        auto_pin: bool,
    ) -> Self {
        Self {
            llm_client,
            rolling_10_enabled: enable_10,
            rolling_100_enabled: enable_100,
            auto_pinning: auto_pin,
        }
    }
    
    /// Checks if rolling summaries should be triggered based on message count
    pub async fn check_and_trigger_rolling_summaries(
        &self,
        session_id: &str,
        message_count: usize,
    ) -> Result<Option<SummaryRequest>> {
        debug!("Checking rolling summaries for session {} at message count {}", 
               session_id, message_count);
        
        // 10-message rolling summary
        if self.rolling_10_enabled && message_count > 0 && message_count % 10 == 0 {
            info!("TRIGGERING 10-message rolling summary for session {} at message {}", 
                  session_id, message_count);
            return Ok(Some(SummaryRequest {
                session_id: session_id.to_string(),
                window_size: 10,
                summary_type: SummaryType::Rolling10,
            }));
        }
        
        // 100-message mega summary
        if self.rolling_100_enabled && message_count > 0 && message_count % 100 == 0 {
            info!("TRIGGERING 100-message mega summary for session {} at message {}", 
                  session_id, message_count);
            return Ok(Some(SummaryRequest {
                session_id: session_id.to_string(),
                window_size: 100,
                summary_type: SummaryType::Rolling100,
            }));
        }
        
        Ok(None)
    }
    
    /// Creates a rolling summary from recent messages
    pub async fn create_rolling_summary(
        &self,
        messages: &[MemoryEntry],
        window_size: usize,
    ) -> Result<MemoryEntry> {
        if messages.is_empty() {
            return Err(anyhow::anyhow!("No messages to summarize"));
        }
        
        // Build content for summarization, filtering out existing summaries
        let content = self.build_summary_content(messages)?;
        
        if content.is_empty() {
            return Err(anyhow::anyhow!("No content to summarize after filtering"));
        }
        
        // Generate the summary using LLM
        let summary_prompt = self.build_summary_prompt(&content, window_size);
        let token_limit = self.get_token_limit(window_size);
        
        info!("Generating {}-message summary (token limit: {})", window_size, token_limit);
        let summary = self.llm_client
            .summarize_conversation(&summary_prompt, token_limit)
            .await?;
        
        // Create the summary entry
        let summary_entry = self.create_summary_entry(
            messages[0].session_id.clone(),
            summary,
            window_size,
        );
        
        info!("Created {}-message rolling summary for session {}", 
              window_size, messages[0].session_id);
        
        Ok(summary_entry)
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
    
    /// Builds the prompt for summary generation based on window size
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
    fn get_token_limit(&self, window_size: usize) -> usize {
        match window_size {
            100 => 800,  // Mega summaries get more space
            10 => 500,   // Rolling summaries are concise
            _ => 600,    // Default for custom sizes
        }
    }
    
    /// Creates a MemoryEntry for the summary
    fn create_summary_entry(
        &self,
        session_id: String,
        summary: String,
        window_size: usize,
    ) -> MemoryEntry {
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
                format!("summary:rolling:{}", window_size),
                "system".to_string(),
            ]),
            
            // Analysis fields
            mood: None,
            intensity: None,
            salience: Some(1.0),  // Summaries have max salience
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
    
    /// Creates a snapshot summary on demand
    pub async fn create_snapshot_summary(
        &self,
        session_id: &str,
        messages: &[MemoryEntry],
        max_tokens: Option<usize>,
    ) -> Result<MemoryEntry> {
        info!("Creating snapshot summary for session {} ({} messages)", 
              session_id, messages.len());
        
        let content = self.build_summary_content(messages)?;
        let token_limit = max_tokens.unwrap_or(1000);
        
        let prompt = format!(
            "Create a comprehensive snapshot of this conversation. \
            Include all key topics, decisions, and important context:\n\n{}",
            content
        );
        
        let summary = self.llm_client
            .summarize_conversation(&prompt, token_limit)
            .await?;
        
        let mut entry = self.create_summary_entry(
            session_id.to_string(),
            summary,
            messages.len(),
        );
        
        // Mark as snapshot instead of rolling
        if let Some(ref mut tags) = entry.tags {
            tags.push("summary:snapshot".to_string());
        }
        
        Ok(entry)
    }
    
    /// Hierarchical summarization for very long conversations
    pub async fn create_hierarchical_summary(
        &self,
        session_id: &str,
        existing_summaries: &[MemoryEntry],
    ) -> Result<MemoryEntry> {
        info!("Creating hierarchical summary from {} existing summaries", 
              existing_summaries.len());
        
        if existing_summaries.len() < 2 {
            return Err(anyhow::anyhow!("Need at least 2 summaries for hierarchical compression"));
        }
        
        // Combine existing summaries
        let mut combined = String::new();
        for (idx, summary) in existing_summaries.iter().enumerate() {
            combined.push_str(&format!("Summary {}: {}\n\n", idx + 1, summary.content));
        }
        
        let prompt = format!(
            "Create a master summary that synthesizes these rolling summaries. \
            Preserve the most important themes and progression of the conversation:\n\n{}",
            combined
        );
        
        let master_summary = self.llm_client
            .summarize_conversation(&prompt, 1000)
            .await?;
        
        let mut entry = self.create_summary_entry(
            session_id.to_string(),
            master_summary,
            existing_summaries.len() * 10,  // Approximate message count
        );
        
        // Mark as hierarchical
        if let Some(ref mut tags) = entry.tags {
            tags.push("summary:hierarchical".to_string());
        }
        
        Ok(entry)
    }
    
    /// Gets summary statistics for monitoring
    pub fn get_summary_stats(&self) -> String {
        format!(
            "Summarization Config - 10-msg: {}, 100-msg: {}, Auto-pin: {}",
            if self.rolling_10_enabled { "enabled" } else { "disabled" },
            if self.rolling_100_enabled { "enabled" } else { "disabled" },
            if self.auto_pinning { "enabled" } else { "disabled" }
        )
    }
}
