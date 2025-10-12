// src/api/ws/chat/unified_handler.rs
// Unified chat handler - THIN ROUTER, delegates everything to orchestrators
// NO PROMPT BUILDING - just gathers context and routes

use std::sync::Arc;
use anyhow::{Result, anyhow};
use serde_json::Value;
use tracing::{info, warn, debug, error};

use crate::api::ws::message::MessageMetadata;
use crate::llm::structured::{CompleteResponse, LLMMetadata};
use crate::llm::structured::tool_schema::*;
use crate::llm::structured::types::StructuredLLMResponse;
use crate::llm::provider::{Message, StreamEvent};
use crate::memory::storage::sqlite::structured_ops::{save_structured_response, process_embeddings};
use crate::memory::features::recall_engine::RecallContext;
use crate::persona::PersonaOverlay;
use crate::state::AppState;
use crate::tools::{ChatOrchestrator, StreamingOrchestrator};

#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub content: String,
    pub project_id: Option<String>,
    pub metadata: Option<MessageMetadata>,
    pub session_id: String,
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
        info!("Processing message for session: {}", request.session_id);
        
        // 1. Save user message
        let user_message_id = self.save_user_message(&request).await?;
        debug!("User message saved: {}", user_message_id);
        
        // 2. Build context (recall)
        let context = self.build_context(&request.session_id, &request.content).await?;
        debug!("Context built - recent: {}, semantic: {}", context.recent.len(), context.semantic.len());
        
        // 3. Select persona
        let persona = self.select_persona(&request.metadata);
        
        // 4. Get tools
        let tools = self.get_tools(&request);
        debug!("Available tools: {}", tools.len());
        
        // 5. Build messages
        let messages = self.build_messages(&context, &request);
        
        // 6. Delegate to orchestrator (which builds prompt and executes)
        let orchestrator = ChatOrchestrator::new(self.app_state.clone());
        let result = orchestrator.execute_with_tools(
            messages,
            persona,
            context,
            tools,
            request.metadata.clone(),
            request.project_id.as_deref(),
        ).await?;
        
        // 7. Save response
        self.finalize_response(result, user_message_id, &request).await
    }
    
    pub async fn handle_message_streaming<F>(
        &self,
        request: ChatRequest,
        on_event: F,
    ) -> Result<CompleteResponse>
    where
        F: FnMut(StreamEvent) -> Result<()> + Send,
    {
        info!("Processing streaming message for session: {}", request.session_id);
        
        // 1. Save user message
        let user_message_id = self.save_user_message(&request).await?;
        debug!("User message saved: {}", user_message_id);
        
        // 2. Build context (recall)
        let context = self.build_context(&request.session_id, &request.content).await?;
        debug!("Context built - recent: {}, semantic: {}", context.recent.len(), context.semantic.len());
        
        // 3. Select persona
        let persona = self.select_persona(&request.metadata);
        
        // 4. Get tools
        let tools = self.get_tools(&request);
        debug!("Available tools: {}", tools.len());
        
        // 5. Build messages
        let messages = self.build_messages(&context, &request);
        
        // 6. Delegate to streaming orchestrator (which builds prompt and executes)
        let orchestrator = StreamingOrchestrator::new(self.app_state.clone());
        let result = orchestrator.execute_with_tools_streaming(
            messages,
            persona,
            context,
            tools,
            request.metadata.clone(),
            request.project_id.as_deref(),
            on_event,
        ).await?;
        
        // 7. Save response
        self.finalize_streaming_response(result, user_message_id, &request).await
    }
    
    fn get_tools(&self, request: &ChatRequest) -> Vec<Value> {
        if request.project_id.is_some() {
            vec![
                get_create_artifact_tool_schema(),
                get_read_file_tool_schema(),
                get_code_search_tool_schema(),
                get_list_files_tool_schema(),
                get_project_context_tool_schema(),
                get_read_files_tool_schema(),
                get_write_files_tool_schema(),
            ]
        } else {
            vec![]
        }
    }
    
    fn build_messages(&self, context: &RecallContext, request: &ChatRequest) -> Vec<Message> {
        let mut messages = Vec::new();
        
        for entry in context.recent.iter().rev() {
            messages.push(Message {
                role: if entry.role == "user" { "user".to_string() } else { "assistant".to_string() },
                content: entry.content.clone(),
            });
        }
        
        messages.push(Message {
            role: "user".to_string(),
            content: request.content.clone(),
        });
        
        messages
    }
    
    async fn finalize_response(
        &self,
        result: crate::tools::ChatResult,
        user_message_id: i64,
        request: &ChatRequest,
    ) -> Result<CompleteResponse> {
        let structured = self.parse_structured_output(&result.content)?;
        
        let metadata = LLMMetadata {
            response_id: None,
            prompt_tokens: Some(result.tokens.input),
            completion_tokens: Some(result.tokens.output),
            thinking_tokens: Some(result.tokens.reasoning),
            total_tokens: Some(result.tokens.input + result.tokens.output + result.tokens.reasoning),
            model_version: "gpt-5".to_string(),
            finish_reason: Some("stop".to_string()),
            latency_ms: result.latency_ms,
            temperature: 0.7,
            max_tokens: 128000,
        };
        
        let complete_response = CompleteResponse {
            structured,
            metadata,
            raw_response: Value::Null,
            artifacts: if result.artifacts.is_empty() { None } else { Some(result.artifacts) },
        };
        
        let message_id = save_structured_response(
            &self.app_state.sqlite_pool,
            &request.session_id,
            &complete_response,
            Some(user_message_id),
        ).await?;
        
        if let Err(e) = process_embeddings(
            &self.app_state.sqlite_pool,
            message_id,
            &request.session_id,
            &complete_response.structured,
            &self.app_state.embedding_client,
            &self.app_state.memory_service.get_multi_store(),
        ).await {
            warn!("Failed to process embeddings: {}", e);
        }
        
        Ok(complete_response)
    }
    
    async fn finalize_streaming_response(
        &self,
        result: crate::tools::StreamingResult,
        user_message_id: i64,
        request: &ChatRequest,
    ) -> Result<CompleteResponse> {
        let structured = self.parse_structured_output(&result.content)?;
        
        let metadata = LLMMetadata {
            response_id: None,
            prompt_tokens: Some(result.tokens.input),
            completion_tokens: Some(result.tokens.output),
            thinking_tokens: Some(result.tokens.reasoning),
            total_tokens: Some(result.tokens.input + result.tokens.output + result.tokens.reasoning),
            model_version: "gpt-5".to_string(),
            finish_reason: Some("stop".to_string()),
            latency_ms: 0,
            temperature: 0.7,
            max_tokens: 128000,
        };
        
        let complete_response = CompleteResponse {
            structured,
            metadata,
            raw_response: Value::Null,
            artifacts: if result.artifacts.is_empty() { None } else { Some(result.artifacts) },
        };
        
        let message_id = save_structured_response(
            &self.app_state.sqlite_pool,
            &request.session_id,
            &complete_response,
            Some(user_message_id),
        ).await?;
        
        if let Err(e) = process_embeddings(
            &self.app_state.sqlite_pool,
            message_id,
            &request.session_id,
            &complete_response.structured,
            &self.app_state.embedding_client,
            &self.app_state.memory_service.get_multi_store(),
        ).await {
            warn!("Failed to process embeddings: {}", e);
        }
        
        Ok(complete_response)
    }
    
    fn parse_structured_output(&self, text_output: &str) -> Result<StructuredLLMResponse> {
        // Parse JSON response from json_schema format
        debug!("Parsing JSON structured output: {} chars", text_output.len());
        
        serde_json::from_str(text_output)
            .map_err(|e| {
                error!("Failed to parse JSON response: {}", e);
                error!("Raw output (first 500 chars): {}", 
                    &text_output.chars().take(500).collect::<String>());
                anyhow!("Failed to parse structured output: {}", e)
            })
    }
    
    async fn save_user_message(&self, request: &ChatRequest) -> Result<i64> {
        self.app_state.memory_service
            .save_user_message(
                &request.session_id,
                &request.content,
                request.project_id.as_deref()
            )
            .await
    }
    
    async fn build_context(
        &self,
        session_id: &str,
        user_message: &str,
    ) -> Result<RecallContext> {
        let mut context = self.app_state.memory_service
            .parallel_recall_context(session_id, user_message, 20, 15)
            .await?;
        
        context.rolling_summary = self.app_state.memory_service
            .get_rolling_summary(session_id)
            .await?;
        
        context.session_summary = self.app_state.memory_service
            .get_session_summary(session_id)
            .await?;
        
        Ok(context)
    }
    
    fn select_persona(&self, _metadata: &Option<MessageMetadata>) -> PersonaOverlay {
        PersonaOverlay::Default
    }
}
