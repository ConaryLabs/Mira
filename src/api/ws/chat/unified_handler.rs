// src/api/ws/chat/unified_handler.rs

use std::sync::Arc;
use anyhow::Result;
use serde_json::{json, Value};
use tracing::{debug, info, warn};

use crate::api::ws::message::MessageMetadata;
use crate::llm::structured::{CompleteResponse, code_fix_processor};
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
        
        // Check for error patterns first
        if let Some(mut error_context) = code_fix_processor::detect_error_context(&request.content) {
            if let Some(project_id) = &request.project_id {
                info!("Detected {} error in file: {}", error_context.error_type, error_context.file_path);
                return self.handle_error_fix(request, error_context).await;
            } else {
                warn!("Error detected but no project context available");
            }
        }
        
        // Save user message before processing
        let user_message_id = self.save_user_message(&request).await?;
        
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
            None,
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
            Some(user_message_id),
        ).await?;
        
        info!("Saved structured response {} with tokens: {:?}", 
              message_id, complete_response.metadata.total_tokens);
        
        Ok(complete_response)
    }
    
    async fn handle_error_fix(
        &self,
        request: ChatRequest,
        mut error_context: code_fix_processor::ErrorContext,
    ) -> Result<CompleteResponse> {
        let project_id = request.project_id.as_ref()
            .ok_or_else(|| anyhow::anyhow!("No project ID for error fix"))?;
        
        // Get project to find the path
        let project = self.app_state.project_service
            .get(project_id)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to get project: {}", e))?;
        
        // Load the complete file and get line count
        let (file_content, line_count) = code_fix_processor::load_complete_file(
            &project.path,
            &error_context.file_path
        ).await?;
        
        // Update error context with line count
        error_context.original_line_count = line_count;
        
        info!("Loaded {} with {} lines for error fix", 
              error_context.file_path, line_count);
        
        // Save user message
        let user_message_id = self.save_user_message(&request).await?;
        
        // Build context for memory recall
        let context = self.build_context(&request.session_id, &request.content).await?;
        
        // Select persona
        let persona = self.select_persona(&request.metadata);
        
        // Build specialized code fix prompt
        let system_prompt = UnifiedPromptBuilder::build_code_fix_prompt(
            &persona,
            &context,
            &error_context,
            &file_content,
            request.metadata.as_ref(),
            request.project_id.as_deref(),
        );
        
        // Build the request with code fix schema
        let request_body = json!({
            "messages": [
                {"role": "system", "content": system_prompt},
                {"role": "user", "content": request.content}
            ],
            "model": "gpt-5",
            "response_format": {
                "type": "json_schema",
                "json_schema": {
                    "name": "code_fix_response",
                    "schema": code_fix_processor::get_code_fix_schema(),
                    "strict": true
                }
            },
            "temperature": 0.3,
            "max_tokens": 16384,
            "stream": false
        });
        
        // Get response from LLM
        let start = std::time::Instant::now();
        let raw_response = self.app_state.llm_client
            .post_response_with_retry(request_body)
            .await?;
        let duration = start.elapsed();
        
        info!("Got code fix response in {:?}", duration);
        
        // Extract and validate the response
        let code_fix = code_fix_processor::extract_code_fix_response(&raw_response)?;
        
        // Validate line counts
        let warnings = code_fix.validate_line_counts(&error_context);
        for warning in &warnings {
            warn!("{}", warning);
        }
        
        // Extract metadata
        let metadata = code_fix_processor::extract_metadata(&raw_response, 0)?;
        
        // Convert to CompleteResponse with artifacts
        let complete_response = code_fix.into_complete_response(metadata, raw_response);
        
        // Save the response
        let message_id = save_structured_response(
            &self.app_state.sqlite_pool,
            &request.session_id,
            &complete_response,
            Some(user_message_id),
        ).await?;
        
        info!("Saved code fix response {} with {} file(s)", 
              message_id, 
              complete_response.artifacts.as_ref().map(|a| a.len()).unwrap_or(0));
        
        Ok(complete_response)
    }
    
    async fn save_user_message(&self, request: &ChatRequest) -> Result<i64> {
        self.app_state.memory_service
            .save_user_message(&request.session_id, &request.content, request.project_id.as_deref())
            .await
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
        
        // Add recent conversation messages
        for memory in &context.recent {
            messages.push(json!({
                "role": memory.role,
                "content": [{
                    "type": "input_text",
                    "text": memory.content
                }]
            }));
        }
        
        // Add semantic memories
        for memory in &context.semantic {
            messages.push(json!({
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
