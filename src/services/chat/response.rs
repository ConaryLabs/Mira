// src/services/chat/response.rs
// Response processing logic for chat conversations
// Handles response creation, persistence, and summarization

use std::sync::Arc;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use crate::services::memory::MemoryService;
use crate::services::summarization::SummarizationService;
use crate::persona::PersonaOverlay;
use crate::api::error::IntoApiError;
use crate::config::CONFIG;

/// Response data structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatResponse {
    pub output: String,
    pub persona: String,
    pub mood: String,
    pub salience: usize,
    pub summary: String,
    pub memory_type: String,
    pub tags: Vec<String>,
    pub intent: Option<String>,
    pub monologue: Option<String>,
    pub reasoning_summary: Option<String>,
}

/// Response processor for chat conversations
pub struct ResponseProcessor {
    memory_service: Arc<MemoryService>,
    summarizer: Arc<SummarizationService>,
    persona: PersonaOverlay,
}

impl ResponseProcessor {
    /// Create new response processor
    pub fn new(
        memory_service: Arc<MemoryService>,
        summarizer: Arc<SummarizationService>,
        persona: PersonaOverlay,
    ) -> Self {
        Self {
            memory_service,
            summarizer,
            persona,
        }
    }

    /// Persist user message to memory
    pub async fn persist_user_message(
        &self,
        session_id: &str,
        user_text: &str,
        project_id: Option<&str>,
    ) -> Result<()> {
        info!("Persisting user message for session: {}", session_id);
        
        self.memory_service
            .save_user_message(session_id, user_text, project_id)
            .await
            .into_api_error("Failed to persist user message")?;
        
        Ok(())
    }

    /// Process and create response structure
    /// Context parameter kept for compatibility until full GPT-5 integration
    /// GPT-5 structured output will provide all metadata directly
    pub async fn process_response(
        &self,
        session_id: &str,
        content: String,
        _context: &crate::memory::recall::RecallContext,
    ) -> Result<ChatResponse> {
        info!("Processing response for session: {}", session_id);

        // Temporary: Using defaults until GPT-5 structured output integration is complete
        // GPT-5 will provide all of this metadata via structured output
        let response = ChatResponse {
            output: content,
            persona: self.persona.to_string(),
            mood: "neutral".to_string(),
            salience: 5,
            summary: "Response generated".to_string(),
            memory_type: "conversational".to_string(),
            tags: vec!["response".to_string(), format!("persona:{}", self.persona)],
            intent: None,
            monologue: None,
            reasoning_summary: None,
        };

        // Persist assistant response
        self.memory_service
            .save_assistant_response(session_id, &response)
            .await
            .into_api_error("Failed to persist assistant response")?;

        info!("Response processed and persisted for session: {}", session_id);
        Ok(response)
    }

    /// Handle summarization based on configuration
    /// Supports both legacy and rolling summarization strategies
    pub async fn handle_summarization(&self, session_id: &str) -> Result<()> {
        info!("Checking summarization strategy for session: {}", session_id);
        
        // Check if rolling summaries are enabled
        if CONFIG.is_robust_memory_enabled() && self.summarizer.should_use_rolling_summaries() {
            // Rolling summaries path
            debug!("Using rolling summarization strategy");
            
            // Rolling summarization is handled automatically in MemoryService.save_assistant_response
            // via check_and_trigger_rolling_summaries to avoid double-triggering
            
            let message_count = self.memory_service.get_session_message_count(session_id).await;
            if message_count % 10 == 0 || message_count % 100 == 0 {
                info!("Rolling summarization handled automatically at message count {}", message_count);
            }
            
            Ok(())
        } else {
            // Legacy summarization path
            debug!("Using legacy summarization strategy");
            
            // Check if we should trigger summarization based on message count
            let message_count = self.memory_service.get_session_message_count(session_id).await;
            let should_summarize = message_count > 0 && message_count % CONFIG.summarize_after_messages == 0;
            
            if should_summarize {
                info!("Triggering legacy summarization for session: {}", session_id);
                // Use summarize_last_n with the configured chunk size
                let summary = self.summarizer.summarize_last_n(
                    session_id, 
                    CONFIG.summary_chunk_size
                ).await?;
                info!("Legacy summarization completed: {}", summary);
            } else {
                debug!("No summarization needed for session: {}", session_id);
            }
            
            Ok(())
        }
    }

    /// Create manual snapshot summary
    /// Allows manual triggering of snapshot summaries at any message count
    pub async fn create_snapshot_summary(&self, session_id: &str, message_count: usize) -> Result<()> {
        if !CONFIG.is_robust_memory_enabled() {
            return Err(anyhow::anyhow!(
                "Snapshot summaries require robust memory to be enabled"));
        }

        info!("Creating snapshot summary of {} messages for session: {}", message_count, session_id);
        
        match self.summarizer.create_snapshot_summary(session_id, message_count).await {
            Ok(_) => {
                info!("Snapshot summary created for session: {}", session_id);
                Ok(())
            }
            Err(e) => {
                warn!("Snapshot summary failed for session {}: {}", session_id, e);
                Err(e)
            }
        }
    }

    /// Get session summarization status
    /// Returns information about the current summarization state
    pub async fn get_summarization_status(&self, session_id: &str) -> Result<SummarizationStatus> {
        let message_count = self.memory_service.get_session_message_count(session_id).await;
        let stats = self.summarizer.get_summarization_stats(session_id).await?;
        let using_rolling = self.summarizer.should_use_rolling_summaries();
        
        let status = SummarizationStatus {
            session_id: session_id.to_string(),
            total_messages: message_count,
            using_rolling_summaries: using_rolling,
            rolling_10_summaries: stats.rolling_10_count,
            rolling_100_summaries: stats.rolling_100_count,
            snapshot_summaries: stats.snapshot_count,
            legacy_summaries: stats.legacy_count,
            next_10_trigger: if using_rolling && CONFIG.summary_rolling_10 {
                Some((message_count / 10 + 1) * 10)
            } else {
                None
            },
            next_100_trigger: if using_rolling && CONFIG.summary_rolling_100 {
                Some((message_count / 100 + 1) * 100)
            } else {
                None
            },
        };
        
        debug!("Summarization status for session {}: {:?}", session_id, status);
        Ok(status)
    }

    /// Create error response for failed chat attempts
    pub fn create_error_response(&self, error_message: &str) -> ChatResponse {
        ChatResponse {
            output: format!("I encountered an error: {}", error_message),
            persona: self.persona.to_string(),
            mood: "apologetic".to_string(),
            salience: 3,
            summary: format!("Error occurred: {}", error_message),
            memory_type: "error".to_string(),
            tags: vec!["error".to_string(), format!("persona:{}", self.persona)],
            intent: Some("error_handling".to_string()),
            monologue: Some(format!("Something went wrong: {}", error_message)),
            reasoning_summary: None,
        }
    }

    /// Get memory service reference
    pub fn memory_service(&self) -> &Arc<MemoryService> {
        &self.memory_service
    }

    /// Get summarization service reference
    pub fn summarization_service(&self) -> &Arc<SummarizationService> {
        &self.summarizer
    }

    /// Get persona reference
    pub fn persona(&self) -> &PersonaOverlay {
        &self.persona
    }
}

/// Summarization status information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SummarizationStatus {
    pub session_id: String,
    pub total_messages: usize,
    pub using_rolling_summaries: bool,
    pub rolling_10_summaries: usize,
    pub rolling_100_summaries: usize,
    pub snapshot_summaries: usize,
    pub legacy_summaries: usize,
    pub next_10_trigger: Option<usize>,
    pub next_100_trigger: Option<usize>,
}

impl SummarizationStatus {
    pub fn total_summaries(&self) -> usize {
        self.rolling_10_summaries + self.rolling_100_summaries + 
        self.snapshot_summaries + self.legacy_summaries
    }

    pub fn is_long_conversation(&self) -> bool {
        self.total_messages >= 50
    }

    pub fn has_any_summaries(&self) -> bool {
        self.total_summaries() > 0
    }

    pub fn compression_ratio(&self) -> f64 {
        if self.total_messages == 0 {
            return 0.0;
        }
        self.total_summaries() as f64 / self.total_messages as f64
    }
}
