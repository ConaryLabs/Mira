// src/api/ws/chat/unified_handler.rs

use std::sync::Arc;
use anyhow::Result;
use serde_json::Value;
use tracing::{debug, info};

use crate::api::ws::message::MessageMetadata;
use crate::llm::structured::CompleteResponse;
use crate::memory::storage::sqlite::structured_ops::save_structured_response;
use crate::memory::features::recall_engine::RecallContext;
use crate::persona::PersonaOverlay;
use crate::prompt::unified_builder::UnifiedPromptBuilder;
use crate::state::AppState;

#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub content: String,
    pub project_id: Option<String>,
    pub metadata: Option<MessageMetadata>,
    pub session_id: String,
    pub require_json: bool,
}

pub struct UnifiedChatHandler {
    app_state: Arc<AppState>,
}

impl UnifiedChatHandler {
    pub fn new(app_state: Arc<AppState>) -> Self {
        Self { app_state }
    }
    
    pub async fn handle_message(
        &self,
        request: ChatRequest,
    ) -> Result<CompleteResponse> {
        info!("Processing structured message for session: {}", request.session_id);
        
        // Save user message before processing
        self.app_state.memory_service
            .save_user_message(&request.session_id, &request.content, request.project_id.as_deref())
            .await?;
        info!("Saved user message for session: {}", request.session_id);
        
        // Build recall context
        let context = self.build_context(&request.session_id, &request.content).await?;
        debug!("Context built: {} recent, {} semantic", 
            context.recent.len(), 
            context.semantic.len()
        );
        
        // Build system prompt with project context
        let persona = self.select_persona(&request.metadata);
        let system_prompt = UnifiedPromptBuilder::build_system_prompt(
            &persona,
            &context,
            None, // tools - can be added later if needed
            request.metadata.as_ref(),
            request.project_id.as_deref(),
        );
        
        // Build context messages for LLM
        let context_messages = self.build_context_messages(&context).await?;
        
        // Get structured response from LLM
        let complete_response = self.app_state.llm_client
            .get_structured_response(
                &request.content,
                system_prompt,
                context_messages,
                &request.session_id,
            )
            .await?;
        
        // Save complete response with all metadata
        let message_id = save_structured_response(
            &self.app_state.sqlite_pool,
            &request.session_id,
            &complete_response,
            None,
        ).await?;
        
        info!("Saved structured response {} with tokens: {:?}", 
              message_id, complete_response.metadata.total_tokens);
        
        Ok(complete_response)
    }
    
    /// Build recall context using the memory service
    async fn build_context(&self, session_id: &str, user_message: &str) -> Result<RecallContext> {
        let recall_service = &self.app_state.memory_service.recall_engine;
        
        let context = recall_service.build_context(
            session_id,
            user_message,
        ).await?;
        
        Ok(context)
    }
    
    /// Select persona based on metadata (currently always Default)
    fn select_persona(&self, _metadata: &Option<MessageMetadata>) -> PersonaOverlay {
        PersonaOverlay::Default
    }
    
    /// Convert recall context into LLM message format
    async fn build_context_messages(&self, context: &RecallContext) -> Result<Vec<Value>> {
        let mut messages = Vec::new();
        
        // Add recent conversation messages
        for memory in &context.recent {
            messages.push(serde_json::json!({
                "role": memory.role,
                "content": [{
                    "type": "input_text",
                    "text": memory.content
                }]
            }));
        }
        
        // Add semantic memories
        for memory in &context.semantic {
            messages.push(serde_json::json!({
                "role": memory.role,
                "content": [{
                    "type": "input_text",
                    "text": memory.content
                }]
            }));
        }
        
        Ok(messages)
    }
}
