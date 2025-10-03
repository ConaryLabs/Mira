// src/api/ws/chat/unified_handler.rs

use std::sync::Arc;
use std::path::Path;
use anyhow::Result;
use serde_json::{json, Value};
use tracing::{debug, info, warn};

use crate::api::ws::message::MessageMetadata;
use crate::llm::structured::{CompleteResponse, code_fix_processor, claude_processor};
use crate::llm::structured::code_fix_processor::ErrorContext;
use crate::memory::storage::sqlite::structured_ops::save_structured_response;
use crate::memory::features::recall_engine::RecallContext;
use crate::persona::PersonaOverlay;
use crate::prompt::unified_builder::UnifiedPromptBuilder;
use crate::state::AppState;
use crate::config::CONFIG;

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
                
                // Use two-phase approach with thinking
                return self.handle_error_fix_with_thinking(request, error_context).await;
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
    
    /// Handle error fix with TWO-PHASE approach:
    /// Phase 1: Deep analysis with thinking (uses tiered budget based on complexity)
    /// Phase 2: Structured fix with forced tool (no thinking, guaranteed structured output)
    async fn handle_error_fix_with_thinking(
        &self,
        request: ChatRequest,
        error_context: ErrorContext,
    ) -> Result<CompleteResponse> {
        info!("Handling error fix with two-phase approach: {}", error_context.file_path);
        
        // Load complete file
        let file_content = self.load_complete_file(
            &error_context.file_path,
            request.project_id.as_deref()
        ).await?;
        
        let file_lines = file_content.lines().count();
        info!("Loaded file with {} lines", file_lines);
        
        // Build context and persona
        let context = self.build_context(&request.session_id, &request.content).await?;
        let persona = self.select_persona(&request.metadata);
        
        // ============================================================
        // PHASE 1: DEEP ANALYSIS WITH THINKING
        // ============================================================
        
        // Use existing complexity analyzer to determine thinking budget
        // Note: We only use the thinking_budget, not the temperature
        // because thinking REQUIRES temperature=1.0 (Claude API requirement)
        let (thinking_budget, _) = claude_processor::analyze_message_complexity(&request.content);
        
        info!(
            "Phase 1 - Analysis with thinking: budget={}, temp=1.0 (required)",
            thinking_budget
        );
        
        // Build analysis prompt
        let analysis_prompt = format!(
            "You are analyzing a code error to understand it deeply before fixing.\n\n\
            Error Message:\n{}\n\n\
            File: {}\n\
            Lines: {}\n\n\
            File Content:\n```\n{}\n```\n\n\
            Think through:\n\
            1. What is the root cause of this error?\n\
            2. What parts of the code are affected?\n\
            3. What are potential side effects of different fixes?\n\
            4. Are there edge cases to consider?\n\
            5. What is the cleanest solution?\n\n\
            Provide your analysis and recommended approach.",
            error_context.error_message,
            error_context.file_path,
            file_lines,
            file_content
        );
        
        // Build Phase 1 request with thinking enabled
        // CRITICAL: Temperature MUST be 1.0 when thinking is enabled
        let analysis_request = json!({
            "model": CONFIG.anthropic_model,
            "max_tokens": 4000,
            "temperature": 1.0,
            "thinking": {
                "type": "enabled",
                "budget_tokens": thinking_budget
            },
            "system": UnifiedPromptBuilder::build_system_prompt(
                &persona,
                &context,
                None,
                request.metadata.as_ref(),
                request.project_id.as_deref(),
            ),
            "messages": [
                json!({
                    "role": "user",
                    "content": analysis_prompt
                })
            ]
        });
        
        // Get analysis response
        let analysis_response = self.app_state.llm_client
            .post_response_with_retry(analysis_request)
            .await?;
        
        // Extract thinking and analysis text
        let thinking_content = self.extract_thinking_blocks(&analysis_response);
        let analysis_text = self.extract_text_content(&analysis_response);
        
        info!(
            "Phase 1 complete - thinking blocks: {}, analysis length: {}",
            thinking_content.len(),
            analysis_text.len()
        );
        
        // ============================================================
        // PHASE 2: STRUCTURED FIX WITH FORCED TOOL
        // ============================================================
        
        info!("Phase 2 - Generating structured fix with tool");
        
        // Build system prompt for fix phase
        let fix_system_prompt = UnifiedPromptBuilder::build_code_fix_prompt(
            &persona,
            &context,
            &error_context,
            &file_content,
            request.metadata.as_ref(),
            request.project_id.as_deref(),
        );
        
        // Include analysis as context message
        let context_messages = vec![
            json!({
                "role": "user",
                "content": analysis_prompt
            }),
            json!({
                "role": "assistant",
                "content": format!(
                    "Analysis:\n{}\n\nNow I'll generate the complete fixed file.",
                    analysis_text
                )
            })
        ];
        
        // Build code fix request with forced tool (no thinking)
        let fix_request = code_fix_processor::build_code_fix_request(
            &error_context.error_message,
            &error_context.file_path,
            &file_content,
            fix_system_prompt,
            context_messages,
        )?;
        
        // Get fix response
        let fix_response = self.app_state.llm_client
            .post_response_with_retry(fix_request)
            .await?;
        
        // Extract and validate code fix
        let code_fix = code_fix_processor::extract_code_fix_response(&fix_response)?;
        
        // Validate line counts
        let warnings = code_fix.validate_line_counts(&ErrorContext {
            error_message: error_context.error_message.clone(),
            file_path: error_context.file_path.clone(),
            original_line_count: file_lines,
        });
        
        for warning in &warnings {
            warn!("{}", warning);
        }
        
        info!(
            "Phase 2 complete - generated {} file(s), confidence: {}",
            code_fix.files.len(),
            code_fix.confidence
        );
        
        // Extract metadata from Phase 2 (the actual fix)
        let metadata = crate::llm::structured::processor::extract_metadata(&fix_response, 0)?;
        
        // Convert to CompleteResponse with enhanced output
        let mut complete_response = code_fix.into_complete_response(metadata, fix_response);
        
        // Enhance output with thinking summary if substantial
        if !thinking_content.is_empty() {
            let thinking_summary = if thinking_content.len() > 500 {
                format!("{}...", &thinking_content[..500])
            } else {
                thinking_content.clone()
            };
            
            complete_response.structured.output = format!(
                "**Analysis Process:**\n{}\n\n**Fix:**\n{}",
                thinking_summary,
                complete_response.structured.output
            );
        }
        
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
    
    /// Extract thinking blocks from Claude response
    fn extract_thinking_blocks(&self, response: &Value) -> String {
        let mut thinking_parts = Vec::new();
        
        if let Some(content) = response["content"].as_array() {
            for block in content {
                if block["type"] == "thinking" {
                    if let Some(thought) = block["thinking"].as_str() {
                        thinking_parts.push(thought.to_string());
                    }
                }
            }
        }
        
        thinking_parts.join("\n\n")
    }
    
    /// Extract text content from Claude response
    fn extract_text_content(&self, response: &Value) -> String {
        if let Some(content) = response["content"].as_array() {
            for block in content {
                if block["type"] == "text" {
                    if let Some(text) = block["text"].as_str() {
                        return text.to_string();
                    }
                }
            }
        }
        
        String::new()
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
