// src/api/ws/chat/unified_handler.rs
// Unified chat handler - delegates orchestration to ChatOrchestrator

use std::sync::Arc;
use anyhow::{Result, anyhow};
use serde_json::Value;
use tracing::{info, warn, debug};

use crate::api::ws::message::MessageMetadata;
use crate::llm::structured::{CompleteResponse, LLMMetadata};
use crate::llm::structured::tool_schema::*;
use crate::llm::structured::types::{StructuredLLMResponse, MessageAnalysis};
use crate::llm::provider::Message;
use crate::memory::storage::sqlite::structured_ops::{save_structured_response, process_embeddings};
use crate::memory::features::recall_engine::RecallContext;
use crate::persona::PersonaOverlay;
use crate::prompt::unified_builder::UnifiedPromptBuilder;
use crate::state::AppState;
use crate::tools::ChatOrchestrator;

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
        
        // Save user message
        let user_message_id = self.save_user_message(&request).await?;
        debug!("User message saved: {}", user_message_id);
        
        // Build context
        let context = self.build_context(&request.session_id, &request.content).await?;
        debug!("Context built - recent: {}, semantic: {}", context.recent.len(), context.semantic.len());
        
        // Build system prompt
        let persona = self.select_persona(&request.metadata);
        let system_prompt = UnifiedPromptBuilder::build_system_prompt(
            &persona,
            &context,
            None,
            request.metadata.as_ref(),
            request.project_id.as_deref(),
        );
        
        // Get available tools
        let tools = self.get_tools(&request);
        debug!("Available tools: {}", tools.len());
        
        // Build message history
        let messages = self.build_messages(&context, &request);
        
        // Delegate to orchestrator
        let orchestrator = ChatOrchestrator::new(self.app_state.clone());
        let result = orchestrator.execute_with_tools(
            messages,
            system_prompt,
            tools,
            request.project_id.as_deref(),
        ).await?;
        
        // Parse structured output and convert to CompleteResponse
        self.finalize_response(result, user_message_id, &request).await
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
        
        // Add recent conversation history
        for entry in context.recent.iter().rev() {
            messages.push(Message {
                role: if entry.role == "user" { "user".to_string() } else { "assistant".to_string() },
                content: entry.content.clone(),
            });
        }
        
        // Add current user message
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
        // Parse structured output from orchestrator result
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
        
        // Save to database
        let message_id = save_structured_response(
            &self.app_state.sqlite_pool,
            &request.session_id,
            &complete_response,
            Some(user_message_id),
        ).await?;
        
        // Process embeddings
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
        let parsed: Value = serde_json::from_str(text_output)
            .map_err(|e| anyhow!("Failed to parse structured output: {}", e))?;
        
        let output = parsed["output"]
            .as_str()
            .ok_or_else(|| anyhow!("Missing 'output' field"))?
            .to_string();
        
        let analysis_obj = parsed["analysis"]
            .as_object()
            .ok_or_else(|| anyhow!("Missing 'analysis' field"))?;
        
        // Ensure routed_to_heads always has at least one entry, defaulting to semantic
        let routed_to_heads = {
            let heads = analysis_obj["routed_to_heads"]
                .as_array()
                .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_else(|| vec![]);
            
            // If GPT-5 returns empty array, default to semantic
            if heads.is_empty() {
                vec!["semantic".to_string()]
            } else {
                heads
            }
        };
        
        let analysis = MessageAnalysis {
            salience: analysis_obj["salience"].as_f64().unwrap_or(0.5),
            topics: analysis_obj["topics"]
                .as_array()
                .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_else(|| vec!["general".to_string()]),
            contains_code: analysis_obj["contains_code"].as_bool().unwrap_or(false),
            programming_lang: analysis_obj["programming_lang"].as_str().map(String::from),
            contains_error: analysis_obj["contains_error"].as_bool().unwrap_or(false),
            error_type: analysis_obj["error_type"].as_str().map(String::from),
            routed_to_heads,
            language: analysis_obj["language"]
                .as_str()
                .unwrap_or("en")
                .to_string(),
            mood: None,
            intensity: None,
            intent: None,
            summary: None,
            relationship_impact: None,
            error_file: None,
            error_severity: None,
        };
        
        Ok(StructuredLLMResponse {
            output,
            analysis,
            reasoning: None,
            schema_name: Some("gpt5_structured".to_string()),
            validation_status: Some("valid".to_string()),
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
