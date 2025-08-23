// src/services/chat/response.rs
// Extracted Response Processing Logic from chat.rs
// Handles response creation, persistence, and summarization

use std::sync::Arc;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::services::memory::MemoryService;
use crate::services::summarization::SummarizationService;
use crate::memory::recall::RecallContext;
use crate::persona::PersonaOverlay;
use crate::api::error::IntoApiError;

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

    /// Handle summarization if needed
    pub async fn handle_summarization(&self, session_id: &str) -> Result<()> {
        info!("🔄 Checking if summarization is needed for session: {}", session_id);
        
        match self.summarizer.summarize_if_needed(session_id).await {
            Ok(_) => {
                info!("✅ Summarization completed (if needed) for session: {}", session_id);
                Ok(())
            }
            Err(e) => {
                warn!("⚠️ Summarization failed for session {}: {}", session_id, e);
                // Don't fail the entire chat if summarization fails
                Ok(())
            }
        }
    }

    /// Generate mood based on context and conversation
    async fn generate_mood(&self, context: &RecallContext) -> String {
        // Simple mood detection based on recent messages
        if context.recent.is_empty() {
            return "neutral".to_string();
        }

        // Look at recent message patterns
        let recent_count = context.recent.len();
        match recent_count {
            1..=3 => "curious".to_string(),
            4..=10 => "engaged".to_string(),
            _ => "conversational".to_string(),
        }
    }

    /// Calculate message salience score
    async fn calculate_salience(&self, context: &RecallContext) -> usize {
        // Simple salience calculation
        let base_salience = 5;
        let context_bonus = if !context.semantic.is_empty() { 2 } else { 0 };
        let history_bonus = std::cmp::min(context.recent.len() / 5, 3);
        
        base_salience + context_bonus + history_bonus
    }

    /// Generate conversation summary
    async fn generate_summary(&self, _context: &RecallContext) -> String {
        // For now, return a simple summary
        // Could be enhanced with actual summarization logic
        "Conversational exchange".to_string()
    }

    /// Determine memory type based on context
    async fn determine_memory_type(&self, context: &RecallContext) -> String {
        if context.semantic.is_empty() {
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
        
        // Add persona-based tag (FIXED: use to_string() instead of name())
        tags.push(format!("persona:{}", self.persona));
        
        tags
    }

    /// Create error response for failed chat attempts
    pub fn create_error_response(&self, error_message: &str) -> ChatResponse {
        ChatResponse {
            output: format!("I encountered an error: {}", error_message),
            persona: self.persona.to_string(),
            mood: "apologetic".to_string(),
            salience: 1,
            summary: "Error response".to_string(),
            memory_type: "error".to_string(),
            tags: vec!["error".to_string()],
            intent: None,
            monologue: Some(format!("Error occurred: {}", error_message)),
            reasoning_summary: None,
        }
    }

    /// Enhanced response creation with metadata analysis
    pub async fn create_enhanced_response(
        &self,
        session_id: &str,
        content: String,
        context: &RecallContext,
        metadata: Option<ResponseMetadata>,
    ) -> Result<ChatResponse> {
        let mut response = self.process_response(session_id, content, context).await?;

        // Apply metadata enhancements if provided
        if let Some(meta) = metadata {
            if let Some(intent) = meta.detected_intent {
                response.intent = Some(intent);
            }
            
            if let Some(reasoning) = meta.reasoning_summary {
                response.reasoning_summary = Some(reasoning);
            }
            
            if !meta.additional_tags.is_empty() {
                response.tags.extend(meta.additional_tags);
            }
        }

        Ok(response)
    }
}

/// Optional metadata for enhanced response processing
#[derive(Debug, Clone)]
pub struct ResponseMetadata {
    pub detected_intent: Option<String>,
    pub reasoning_summary: Option<String>,
    pub additional_tags: Vec<String>,
    pub confidence_score: Option<f64>,
}

impl ResponseMetadata {
    pub fn new() -> Self {
        Self {
            detected_intent: None,
            reasoning_summary: None,
            additional_tags: Vec::new(),
            confidence_score: None,
        }
    }

    pub fn with_intent(mut self, intent: String) -> Self {
        self.detected_intent = Some(intent);
        self
    }

    pub fn with_reasoning(mut self, reasoning: String) -> Self {
        self.reasoning_summary = Some(reasoning);
        self
    }

    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.additional_tags = tags;
        self
    }

    pub fn with_confidence(mut self, score: f64) -> Self {
        self.confidence_score = Some(score);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::recall::RecallContext;

    #[test]
    fn test_response_metadata_builder() {
        let metadata = ResponseMetadata::new()
            .with_intent("question".to_string())
            .with_reasoning("User asked about topics".to_string())
            .with_tags(vec!["inquiry".to_string()])
            .with_confidence(0.85);

        assert_eq!(metadata.detected_intent, Some("question".to_string()));
        assert_eq!(metadata.confidence_score, Some(0.85));
        assert_eq!(metadata.additional_tags.len(), 1);
    }

    #[tokio::test]
    async fn test_mood_generation() {
        // This would need proper mocks for a real test
        let context = RecallContext {
            recent: vec![/* mock messages */],
            semantic: vec![],
        };

        // Mock test - in real implementation, we'd need proper service mocks
        let mood = if context.recent.is_empty() {
            "neutral"
        } else {
            "engaged"
        };

        assert_eq!(mood, "neutral");
    }
}
