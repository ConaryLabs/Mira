// src/api/ws/chat/unified_handler.rs

use std::sync::Arc;
use anyhow::Result;
use serde_json::Value;
use tracing::{debug, info};

use crate::api::ws::message::MessageMetadata;
use crate::config::CONFIG;
use crate::llm::structured::CompleteResponse;
use crate::memory::storage::sqlite::structured_ops::save_structured_response;
use crate::memory::RecallContext;
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
        
        let context = self.build_context(&request.session_id, &request.content).await?;
        debug!("Context built: {} recent, {} semantic", 
            context.recent.len(), 
            context.semantic.len()
        );
        
        let persona = self.select_persona(&request.metadata);
        let system_prompt = UnifiedPromptBuilder::build_system_prompt(
            &persona,
            &context,
            None,
            request.metadata.as_ref(),
            request.project_id.as_deref(),
            request.require_json,
        );
        
        let context_messages = self.build_context_messages(&context).await?;
        
        let complete_response = self.app_state.llm_client
            .get_structured_response(
                &request.content,
                system_prompt,
                context_messages,
                &request.session_id,
            )
            .await?;
        
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
    
    async fn build_context(&self, session_id: &str, user_message: &str) -> Result<RecallContext> {
        let recall_service = &self.app_state.memory_service.recall_engine;
        
        let context = recall_service.build_context(
            session_id,
            user_message,
        ).await?;
        
        Ok(context)
    }
    
    fn select_persona(&self, _metadata: &Option<MessageMetadata>) -> PersonaOverlay {
        PersonaOverlay::Default
    }
    
    async fn build_context_messages(&self, context: &RecallContext) -> Result<Vec<Value>> {
        let mut messages = Vec::new();
        
        for memory in &context.recent {
            messages.push(serde_json::json!({
                "role": memory.role,
                "content": [{
                    "type": "input_text",
                    "text": memory.content
                }]
            }));
        }
        
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
