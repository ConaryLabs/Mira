// src/api/ws/chat/unified_handler.rs
// Unified chat handler - delegates orchestration to ChatOrchestrator or StreamingOrchestrator

use std::sync::Arc;
use anyhow::{Result, anyhow};
use serde_json::Value;
use tracing::{info, warn, debug};

use crate::api::ws::message::MessageMetadata;
use crate::llm::structured::{CompleteResponse, LLMMetadata};
use crate::llm::structured::tool_schema::*;
use crate::llm::structured::types::StructuredLLMResponse;
use crate::llm::provider::{Message, StreamEvent};
use crate::memory::storage::sqlite::structured_ops::{save_structured_response, process_embeddings};
use crate::memory::features::recall_engine::RecallContext;
use crate::persona::PersonaOverlay;
use crate::prompt::unified_builder::UnifiedPromptBuilder;
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
        
        let user_message_id = self.save_user_message(&request).await?;
        debug!("User message saved: {}", user_message_id);
        
        // CRITICAL FIX: Process embeddings for user message
        self.process_user_message_embeddings(user_message_id, &request).await?;
        
        let context = self.build_context(&request.session_id, &request.content).await?;
        debug!("Context built - recent: {}, semantic: {}", context.recent.len(), context.semantic.len());
        
        let persona = self.select_persona(&request.metadata);
        let system_prompt = UnifiedPromptBuilder::build_system_prompt(
            &persona,
            &context,
            None,
            request.metadata.as_ref(),
            request.project_id.as_deref(),
        );
        
        let tools = self.get_tools(&request);
        debug!("Available tools: {}", tools.len());
        
        let messages = self.build_messages(&context, &request);
        
        let orchestrator = ChatOrchestrator::new(self.app_state.clone());
        let result = orchestrator.execute_with_tools(
            messages,
            system_prompt,
            tools,
            request.project_id.as_deref(),
        ).await?;
        
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
        
        let user_message_id = self.save_user_message(&request).await?;
        debug!("User message saved: {}", user_message_id);
        
        // CRITICAL FIX: Process embeddings for user message
        self.process_user_message_embeddings(user_message_id, &request).await?;
        
        let context = self.build_context(&request.session_id, &request.content).await?;
        debug!("Context built - recent: {}, semantic: {}", context.recent.len(), context.semantic.len());
        
        let persona = self.select_persona(&request.metadata);
        let system_prompt = UnifiedPromptBuilder::build_system_prompt(
            &persona,
            &context,
            None,
            request.metadata.as_ref(),
            request.project_id.as_deref(),
        );
        
        let tools = self.get_tools(&request);
        debug!("Available tools: {}", tools.len());
        
        let messages = self.build_messages(&context, &request);
        
        let orchestrator = StreamingOrchestrator::new(self.app_state.clone());
        let result = orchestrator.execute_with_tools_streaming(
            messages,
            system_prompt,
            tools,
            request.project_id.as_deref(),
            on_event,
        ).await?;
        
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
        use crate::llm::provider::lark_parser::parse_lark_output;
        
        parse_lark_output(text_output)
            .map_err(|e| anyhow!("Failed to parse Lark output: {}", e))
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
    
    /// Process embeddings for user message
    async fn process_user_message_embeddings(
        &self,
        user_message_id: i64,
        request: &ChatRequest,
    ) -> Result<()> {
        use crate::config::CONFIG;
        use crate::llm::embeddings::EmbeddingHead;
        use crate::memory::core::types::MemoryEntry;
        use crate::memory::storage::sqlite::structured_ops::track_embedding_in_db;
        use chrono::Utc;
        
        // Generate embedding for user message
        let embedding = match self.app_state.embedding_client.embed(&request.content).await {
            Ok(emb) => emb,
            Err(e) => {
                warn!("Failed to generate embedding for user message {}: {}", user_message_id, e);
                return Ok(()); // Don't fail the request
            }
        };
        
        debug!("Generated embedding for user message {} (dimension: {})", 
            user_message_id, embedding.len());
        
        // Determine which heads to route to (semantic by default for user messages)
        let heads_to_route = vec!["semantic".to_string()];
        
        // Store in each routed head
        for head_str in &heads_to_route {
            let head = match head_str.parse::<EmbeddingHead>() {
                Ok(h) => h,
                Err(e) => {
                    warn!("Invalid embedding head '{}' for user message {}: {}", head_str, user_message_id, e);
                    continue;
                }
            };
            
            if !CONFIG.embed_heads.contains(head_str) {
                debug!("Head '{}' not enabled in config, skipping", head_str);
                continue;
            }
            
            // Create memory entry for Qdrant
            let qdrant_entry = MemoryEntry {
                id: Some(user_message_id),
                session_id: request.session_id.clone(),
                response_id: None,
                parent_id: None,
                role: "user".to_string(),
                content: request.content.clone(),
                timestamp: Utc::now(),
                tags: request.project_id.as_ref().map(|pid| vec![format!("project:{}", pid)]),
                mood: None,
                intensity: None,
                salience: Some(5.0), // Default salience for user messages
                original_salience: Some(5.0),
                intent: None,
                topics: None,
                summary: None,
                relationship_impact: None,
                contains_code: Some(false),
                language: Some("en".to_string()),
                programming_lang: None,
                analyzed_at: Some(Utc::now()),
                analysis_version: Some("user_v1".to_string()),
                routed_to_heads: Some(heads_to_route.clone()),
                last_recalled: Some(Utc::now()),
                recall_count: Some(0),
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
                embedding: Some(embedding.clone()),
                embedding_heads: Some(heads_to_route.clone()),
                qdrant_point_ids: None,
            };
            
            // Save to Qdrant and track in DB
            match self.app_state.memory_service.get_multi_store().save(head, &qdrant_entry).await {
                Ok(point_id) => {
                    debug!("Stored user message {} embedding in {} collection (point_id: {})", 
                        user_message_id, head.as_str(), point_id);
                    
                    let collection_name = self.app_state.memory_service.get_multi_store()
                        .get_collection_name(head)
                        .unwrap_or_else(|| format!("unknown-{}", head.as_str()));
                    
                    if let Err(e) = track_embedding_in_db(
                        &self.app_state.sqlite_pool,
                        user_message_id,
                        &point_id,
                        &collection_name,
                        head_str,
                    ).await {
                        warn!("Failed to track user message {} embedding: {}", user_message_id, e);
                    }
                }
                Err(e) => {
                    warn!("Failed to store user message {} embedding in {} collection: {}", 
                        user_message_id, head.as_str(), e);
                }
            }
        }
        
        Ok(())
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
