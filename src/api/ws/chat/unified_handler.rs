// src/api/ws/chat/unified_handler.rs

use std::sync::Arc;
use std::path::Path;
use anyhow::Result;
use serde_json::{json, Value};
use tracing::{debug, info, warn};

use crate::api::ws::message::MessageMetadata;
use crate::llm::structured::{CompleteResponse, code_fix_processor};
use crate::llm::structured::code_fix_processor::ErrorContext;
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
            if let Some(_project_id) = &request.project_id {
                info!("Detected error in file: {}", error_context.file_path);
                
                // Load file and update line count
                if let Ok(content) = self.load_complete_file(
                    &error_context.file_path,
                    request.project_id.as_deref()
                ).await {
                    error_context.original_line_count = content.lines().count();
                }
                
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
        
        // Build context messages for LLM (with prompt caching)
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
        
        // Save to database with user message link
        let _message_id = save_structured_response(
            &self.app_state.sqlite_pool,
            &request.session_id,
            &complete_response,
            Some(user_message_id),
        ).await?;
        
        Ok(complete_response)
    }
    
    async fn handle_error_fix(
        &self,
        request: ChatRequest,
        error_context: ErrorContext,
    ) -> Result<CompleteResponse> {
        info!("Handling error fix in {}", error_context.file_path);
        
        // Load complete file
        let file_content = self.load_complete_file(
            &error_context.file_path,
            request.project_id.as_deref()
        ).await?;
        
        // Build context and persona
        let context = self.build_context(&request.session_id, &request.content).await?;
        let persona = self.select_persona(&request.metadata);
        
        // Use UnifiedPromptBuilder for code fix prompts (no duplication!)
        let system_prompt = UnifiedPromptBuilder::build_code_fix_prompt(
            &persona,
            &context,
            &error_context,
            &file_content,
            request.metadata.as_ref(),
            request.project_id.as_deref(),
        );
        
        // Build code fix request
        let request_body = code_fix_processor::build_code_fix_request(
            &error_context.error_message,
            &error_context.file_path,
            &file_content,
            system_prompt,
            vec![],
        )?;
        
        // Get response
        let raw_response = self.app_state.llm_client
            .post_response_with_retry(request_body)
            .await?;
        
        // Extract and validate
        let code_fix = code_fix_processor::extract_code_fix_response(&raw_response)?;
        
        // Check line count warnings
        let warnings = code_fix.validate_line_counts(&error_context);
        for warning in &warnings {
            warn!("{}", warning);
        }
        
        // Extract metadata using the processor module directly
        let metadata = crate::llm::structured::processor::extract_metadata(&raw_response, 0)?;
        
        // Convert to CompleteResponse
        let complete_response = code_fix.into_complete_response(metadata, raw_response);
        
        // Save to database
        let user_message_id = self.save_user_message(&request).await?;
        let _message_id = save_structured_response(
            &self.app_state.sqlite_pool,
            &request.session_id,
            &complete_response,
            Some(user_message_id),
        ).await?;
        
        Ok(complete_response)
    }
    
    /// Load complete file contents, preferring project-scoped paths
    /// 
    /// Attempts to load from project context first (via git attachment local path),
    /// falling back to direct filesystem read if project context is unavailable.
    async fn load_complete_file(
        &self,
        file_path: &str,
        project_id: Option<&str>
    ) -> Result<String> {
        // Try project-scoped read first
        if let Some(proj_id) = project_id {
            if let Some(project) = self.app_state.project_store.get_project(proj_id).await? {
                debug!("Loading file from project context: {}", project.name);
                
                if let Some(attachment) = self.app_state.git_client.store
                    .get_attachment(proj_id)
                    .await? 
                {
                    let full_path = Path::new(&attachment.local_path).join(file_path);
                    
                    match tokio::fs::read_to_string(&full_path).await {
                        Ok(content) => {
                            debug!("Loaded {} bytes from project file: {}", content.len(), file_path);
                            return Ok(content);
                        }
                        Err(e) => {
                            warn!(
                                "Failed to read project file {}: {}. Trying direct path fallback.", 
                                full_path.display(), 
                                e
                            );
                        }
                    }
                }
            }
        }
        
        // Fallback: try direct path (useful for non-project files or development)
        tokio::fs::read_to_string(file_path).await
            .map_err(|e| anyhow::anyhow!("Failed to load file '{}': {}", file_path, e))
    }
    
    /// Save user message and return the message ID
    async fn save_user_message(&self, request: &ChatRequest) -> Result<i64> {
        self.app_state.memory_service
            .save_user_message(
                &request.session_id,
                &request.content,
                request.project_id.as_deref()
            )
            .await
    }
    
    async fn build_context(&self, session_id: &str, content: &str) -> Result<RecallContext> {
        // Use parallel_recall_context with proper parameters
        self.app_state.memory_service
            .parallel_recall_context(session_id, content, 5, 5)
            .await
    }
    
    /// Build context messages in simple format (no caching)
    /// 
    /// Context caching disabled because conversation history changes with each
    /// request (sliding window), resulting in cache misses while still paying
    /// the 25% write premium. Only system prompt + tools are cached (1h TTL).
    async fn build_context_messages(&self, context: &RecallContext) -> Result<Vec<Value>> {
        let mut messages = Vec::new();
        
        // Add recent messages in simple format (no cache_control)
        for memory in &context.recent {
            messages.push(json!({
                "role": if memory.role == "user" { "user" } else { "assistant" },
                "content": memory.content
            }));
        }
        
        Ok(messages)
    }
    
    /// Select persona based on metadata
    /// 
    /// Currently always returns Default as persona switching is not implemented.
    /// Infrastructure exists for switching based on metadata (see PersonaOverlay enum),
    /// but the actual switching logic hasn't been built yet.
    fn select_persona(&self, _metadata: &Option<MessageMetadata>) -> PersonaOverlay {
        // TODO: Implement persona switching based on metadata when feature is ready
        // Possible triggers: explicit user request, conversation context, mood detection
        PersonaOverlay::Default
    }
}
