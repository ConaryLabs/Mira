// src/services/chat/response.rs
// Extracted Response Processing Logic from chat.rs
// Handles response creation, persistence, and summarization

use std::sync::Arc;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use crate::services::memory::MemoryService;
use crate::services::summarization::SummarizationService;
use crate::memory::recall::RecallContext;
use crate::persona::PersonaOverlay;
use crate::api::error::IntoApiError;
use crate::config::CONFIG;

/// Response data structure - Re-exported from main chat.rs
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
        info!("💾 Persisting user message for session: {}", session_id);
        
        self.memory_service
            .save_user_message(session_id, user_text, project_id)
            .await
            .into_api_error("Failed to persist user message")?;
        
        Ok(())
    }

    /// Process and create response structure
    pub async fn process_response(
        &self,
        session_id: &str,
        content: String,
        context: &RecallContext,
    ) -> Result<ChatResponse> {
        info!("🔄 Processing response for session: {}", session_id);

        // Create response structure with metadata
        let response = ChatResponse {
            output: content,
            persona: self.persona.to_string(),
            mood: self.generate_mood(context).await,
            salience: self.calculate_salience(&context).await,
            summary: self.generate_summary(&context).await,
            memory_type: self.determine_memory_type(&context).await,
            tags: self.extract_tags(&context).await,
            intent: None, // Could be enhanced with intent detection
            monologue: None, // Could be enhanced with internal reasoning
            reasoning_summary: None, // Could be enhanced with reasoning chains
        };

        // Persist assistant response
        self.memory_service
            .save_assistant_response(session_id, &response)
            .await
            .into_api_error("Failed to persist assistant response")?;

        info!("✅ Response processed and persisted for session: {}", session_id);
        Ok(response)
    }

    /// ── Phase 4: Enhanced handle_summarization with rolling summaries support ──
    /// This method now chooses between legacy and rolling summarization based on config.
    pub async fn handle_summarization(&self, session_id: &str) -> Result<()> {
        info!("🔄 Checking summarization strategy for session: {}", session_id);
        
        // Check if rolling summaries are enabled
        if CONFIG.is_robust_memory_enabled() && self.summarizer.should_use_rolling_summaries() {
            // ── Phase 4: Rolling summaries path ──
            debug!("Using rolling summarization strategy");
            
            // The rolling summarization logic is already handled in MemoryService.save_assistant_response
            // via check_and_trigger_rolling_summaries, so we don't need to do anything here.
            // This is by design to avoid double-triggering summaries.
            
            // However, we can check if any rolling summaries were just created
            let message_count = self.memory_service.get_session_message_count(session_id).await;
            if message_count % 10 == 0 || message_count % 100 == 0 {
                info!("✅ Rolling summarization handled automatically at message count {}", message_count);
            }
            
            Ok(())
        } else {
            // ── Legacy summarization path ──
            debug!("Using legacy summarization strategy");
            
            match self.summarizer.summarize_if_needed(session_id).await {
                Ok(_) => {
                    info!("✅ Legacy summarization completed (if needed) for session: {}", session_id);
                    Ok(())
                }
                Err(e) => {
                    warn!("⚠️ Legacy summarization failed for session {}: {}", session_id, e);
                    // Don't fail the entire chat if summarization fails
                    Ok(())
                }
            }
        }
    }

    /// ── Phase 4: Manual rolling summary trigger ──
    /// Allows manual creation of rolling summaries via API calls.
    pub async fn trigger_rolling_summary(&self, session_id: &str, window_size: usize) -> Result<()> {
        if !CONFIG.is_robust_memory_enabled() {
            return Err(anyhow::anyhow!("Rolling summaries require robust memory to be enabled"));
        }

        info!("🔄 Manually triggering {}-message rolling summary for session: {}", window_size, session_id);
        
        match self.summarizer.create_rolling_summary(session_id, window_size).await {
            Ok(_) => {
                info!("✅ Manual rolling summary created for session: {}", session_id);
                Ok(())
            }
            Err(e) => {
                warn!("⚠️ Manual rolling summary failed for session {}: {}", session_id, e);
                Err(e)
            }
        }
    }

    /// ── Phase 4: Snapshot summary trigger ──
    /// Creates on-demand summaries for any number of messages.
    pub async fn trigger_snapshot_summary(&self, session_id: &str, message_count: usize) -> Result<()> {
        if !CONFIG.is_robust_memory_enabled() {
            return Err(anyhow::anyhow!("Snapshot summaries require robust memory to be enabled"));
        }

        info!("📸 Creating snapshot summary of {} messages for session: {}", message_count, session_id);
        
        match self.summarizer.create_snapshot_summary(session_id, message_count).await {
            Ok(_) => {
                info!("✅ Snapshot summary created for session: {}", session_id);
                Ok(())
            }
            Err(e) => {
                warn!("⚠️ Snapshot summary failed for session {}: {}", session_id, e);
                Err(e)
            }
        }
    }

    /// ── Phase 4: Get session summarization status ──
    /// Returns information about the current summarization state.
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

    /// Generate mood based on context and conversation
    async fn generate_mood(&self, context: &RecallContext) -> String {
        // Simple mood detection based on recent messages
        if context.recent.is_empty() {
            return "neutral".to_string();
        }

        // Look at recent message patterns
        let recent_count = context.recent.len();
        
        // Check if we have rolling summaries in context (Phase 4 enhancement)
        let has_summaries = context.recent.iter().any(|msg| {
            msg.tags.as_ref()
                .map(|tags| tags.iter().any(|tag| tag.contains("summary")))
                .unwrap_or(false)
        });

        match recent_count {
            1..=3 => "curious".to_string(),
            4..=10 => if has_summaries { "reflective".to_string() } else { "engaged".to_string() },
            _ => if has_summaries { "comprehensive".to_string() } else { "conversational".to_string() },
        }
    }

    /// Calculate message salience score
    async fn calculate_salience(&self, context: &RecallContext) -> usize {
        // Enhanced salience calculation with Phase 4 awareness
        let base_salience = 5;
        let context_bonus = if !context.semantic.is_empty() { 2 } else { 0 };
        let history_bonus = std::cmp::min(context.recent.len() / 5, 3);
        
        // Bonus for sessions with rolling summaries (indicates long conversations)
        let summary_bonus = if context.recent.iter().any(|msg| {
            msg.tags.as_ref()
                .map(|tags| tags.iter().any(|tag| tag.starts_with("summary:rolling:")))
                .unwrap_or(false)
        }) {
            2
        } else {
            0
        };
        
        base_salience + context_bonus + history_bonus + summary_bonus
    }

    /// Generate conversation summary
    async fn generate_summary(&self, context: &RecallContext) -> String {
        // Enhanced summary generation with Phase 4 awareness
        let recent_count = context.recent.len();
        let semantic_count = context.semantic.len();
        
        // Check for rolling summaries in context
        let has_rolling_summaries = context.recent.iter().any(|msg| {
            msg.tags.as_ref()
                .map(|tags| tags.iter().any(|tag| tag.starts_with("summary:rolling:")))
                .unwrap_or(false)
        });
        
        if has_rolling_summaries {
            format!("Extended conversation with {} recent messages and {} semantic matches (with rolling summaries)", 
                recent_count, semantic_count)
        } else if recent_count > 10 {
            format!("Extended conversation with {} messages and {} semantic matches", 
                recent_count, semantic_count)
        } else {
            "Conversational exchange".to_string()
        }
    }

    /// Determine memory type based on context
    async fn determine_memory_type(&self, context: &RecallContext) -> String {
        // Enhanced memory type determination with Phase 4 awareness
        let has_summaries = context.recent.iter().any(|msg| {
            msg.tags.as_ref()
                .map(|tags| tags.iter().any(|tag| tag.contains("summary")))
                .unwrap_or(false)
        });
        
        if has_summaries {
            "compressed".to_string()
        } else if context.semantic.is_empty() {
            "episodic".to_string()
        } else {
            "semantic".to_string()
        }
    }

    /// Extract tags from context
    async fn extract_tags(&self, context: &RecallContext) -> Vec<String> {
        let mut tags = Vec::new();
        
        if !context.recent.is_empty() {
            tags.push("conversational".to_string());
        }
        
        if !context.semantic.is_empty() {
            tags.push("contextual".to_string());
        }
        
        // Add persona-based tag
        tags.push(format!("persona:{}", self.persona));
        
        // ── Phase 4: Add rolling summary awareness tags ──
        let has_rolling_10 = context.recent.iter().any(|msg| {
            msg.tags.as_ref()
                .map(|tags| tags.iter().any(|tag| tag == "summary:rolling:10"))
                .unwrap_or(false)
        });
        
        let has_rolling_100 = context.recent.iter().any(|msg| {
            msg.tags.as_ref()
                .map(|tags| tags.iter().any(|tag| tag == "summary:rolling:100"))
                .unwrap_or(false)
        });
        
        if has_rolling_10 {
            tags.push("has_rolling_10".to_string());
        }
        
        if has_rolling_100 {
            tags.push("has_rolling_100".to_string());
        }
        
        if CONFIG.is_robust_memory_enabled() {
            tags.push("robust_memory".to_string());
        }
        
        tags
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

    /// Get memory service reference (for external access)
    pub fn memory_service(&self) -> &Arc<MemoryService> {
        &self.memory_service
    }

    /// Get summarization service reference (for external access)
    pub fn summarization_service(&self) -> &Arc<SummarizationService> {
        &self.summarizer
    }

    /// Get persona reference (for external access)
    pub fn persona(&self) -> &PersonaOverlay {
        &self.persona
    }
}

/// ── Phase 4: Summarization status information ──
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
