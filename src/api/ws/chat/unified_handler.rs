// src/api/ws/chat/unified_handler.rs

use std::sync::Arc;
use std::path::Path;
use anyhow::{Result, anyhow};
use serde_json::{json, Value};
use tracing::{debug, info, warn};

use crate::api::ws::message::MessageMetadata;
use crate::llm::structured::{CompleteResponse, code_fix_processor, claude_processor};
use crate::llm::structured::code_fix_processor::ErrorContext;
use crate::llm::structured::types::{StructuredLLMResponse, LLMMetadata};
use crate::llm::structured::tool_schema::*;
use crate::memory::storage::sqlite::structured_ops::save_structured_response;
use crate::memory::features::recall_engine::RecallContext;
use crate::memory::features::code_intelligence::types::FileContext;
use crate::persona::PersonaOverlay;
use crate::prompt::unified_builder::{UnifiedPromptBuilder, CodeElement, QualityIssue};
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
                
                return self.handle_error_fix_with_thinking(request, error_context).await;
            } else {
                warn!("Error detected but no project context available");
            }
        }
        
        // Use tool execution loop for regular chat
        self.handle_chat_with_tools(request).await
    }
    
    /// Process chat with tool execution loop
    async fn handle_chat_with_tools(
        &self,
        request: ChatRequest,
    ) -> Result<CompleteResponse> {
        // Save user message first
        let user_message_id = self.save_user_message(&request).await?;
        
        // Build initial context
        let context = self.build_context(&request.session_id, &request.content).await?;
        let persona = self.select_persona(&request.metadata);
        
        let system_prompt = UnifiedPromptBuilder::build_system_prompt(
            &persona,
            &context,
            None,
            request.metadata.as_ref(),
            request.project_id.as_deref(),
        );
        
        // Define available tools
        let tools = vec![
            get_response_tool_schema(),
            get_read_file_tool_schema(),
            get_code_search_tool_schema(),
            get_list_files_tool_schema(),
        ];
        
        let mut context_messages = self.build_context_messages(&context).await?;
        
        // Tool execution loop - 20 iterations allows complex research without spiraling
        let max_iterations = 20;
        let mut iteration = 0;
        
        loop {
            iteration += 1;
            
            if iteration > max_iterations {
                warn!("Max tool iterations ({}) reached, forcing final response with gathered context", max_iterations);
                
                // Force a final response with everything learned so far
                let final_request = claude_processor::build_claude_request_with_tool(
                    &request.content,
                    system_prompt.clone(),
                    context_messages.clone(),
                )?;
                
                let final_response = self.app_state.llm_client
                    .post_response_with_retry(final_request)
                    .await?;
                
                let structured = claude_processor::extract_claude_content_from_tool(&final_response)?;
                let metadata = claude_processor::extract_claude_metadata(&final_response, 0)?;
                
                let complete_response = CompleteResponse {
                    structured,
                    metadata,
                    raw_response: final_response,
                    artifacts: None,
                };
                
                save_structured_response(
                    &self.app_state.sqlite_pool,
                    &request.session_id,
                    &complete_response,
                    Some(user_message_id),
                ).await?;
                
                return Ok(complete_response);
            }
            
            // Build request with custom tools
            let llm_request = claude_processor::build_claude_request_with_custom_tools(
                &request.content,
                system_prompt.clone(),
                context_messages.clone(),
                tools.clone(),
            )?;
            
            // Get Claude's response
            let raw_response = self.app_state.llm_client
                .post_response_with_retry(llm_request)
                .await?;
            
            // Check if response has tool calls
            if !claude_processor::has_tool_calls(&raw_response) {
                // No tools - extract final response
                let structured = claude_processor::extract_claude_content_from_tool(&raw_response)?;
                let metadata = claude_processor::extract_claude_metadata(&raw_response, 0)?;
                
                let complete_response = CompleteResponse {
                    structured,
                    metadata,
                    raw_response,
                    artifacts: None,
                };
                
                // Save to database
                save_structured_response(
                    &self.app_state.sqlite_pool,
                    &request.session_id,
                    &complete_response,
                    Some(user_message_id),
                ).await?;
                
                return Ok(complete_response);
            }
            
            // Extract tool calls
            let tool_calls = claude_processor::extract_tool_calls(&raw_response)?;
            
            info!("Processing {} tool call(s)", tool_calls.len());
            
            // Execute each tool
            let mut tool_results = Vec::new();
            
            for tool_call in &tool_calls {
                let tool_name = tool_call["name"].as_str().unwrap_or("unknown");
                let tool_input = &tool_call["input"];
                
                info!("Executing tool: {}", tool_name);
                
                let result = match tool_name {
                    "read_file" => {
                        self.execute_read_file(tool_input, &request).await?
                    }
                    "search_code" => {
                        self.execute_search_code(tool_input, &request).await?
                    }
                    "list_files" => {
                        self.execute_list_files(tool_input, &request).await?
                    }
                    "respond_to_user" => {
                        // Final response tool - extract and return
                        let structured: StructuredLLMResponse = serde_json::from_value(tool_input.clone())?;
                        let metadata = claude_processor::extract_claude_metadata(&raw_response, 0)?;
                        
                        let complete_response = CompleteResponse {
                            structured,
                            metadata,
                            raw_response,
                            artifacts: None,
                        };
                        
                        // Save to database
                        save_structured_response(
                            &self.app_state.sqlite_pool,
                            &request.session_id,
                            &complete_response,
                            Some(user_message_id),
                        ).await?;
                        
                        return Ok(complete_response);
                    }
                    _ => {
                        json!({ "error": format!("Unknown tool: {}", tool_name) })
                    }
                };
                
                tool_results.push(json!({
                    "type": "tool_result",
                    "tool_use_id": tool_call["id"],
                    "content": result.to_string()
                }));
            }
            
            // Add assistant message with tool calls
            context_messages.push(json!({
                "role": "assistant",
                "content": raw_response["content"]
            }));
            
            // Add tool results as user messages
            for result in tool_results {
                context_messages.push(json!({
                    "role": "user",
                    "content": vec![result]
                }));
            }
        }
    }
    
    /// Execute read_file tool
    async fn execute_read_file(
        &self,
        input: &Value,
        request: &ChatRequest,
    ) -> Result<Value> {
        let path = input["path"].as_str()
            .ok_or_else(|| anyhow!("Missing 'path' in read_file input"))?;
        
        info!("Reading file: {}", path);
        
        // Use existing load_complete_file method
        let content = self.load_complete_file(path, request.project_id.as_deref()).await?;
        
        Ok(json!({
            "path": path,
            "content": content,
            "lines": content.lines().count()
        }))
    }
    
    /// Execute search_code tool
    async fn execute_search_code(
        &self,
        input: &Value,
        _request: &ChatRequest,
    ) -> Result<Value> {
        let query = input["query"].as_str()
            .ok_or_else(|| anyhow!("Missing 'query' in search_code input"))?;
        
        let _element_type = input["element_type"].as_str();
        
        info!("Searching code: {}", query);
        
        // Search using code intelligence service (takes pattern and limit only)
        let results = self.app_state.code_intelligence
            .search_elements(query, Some(20))
            .await?;
        
        Ok(json!({
            "query": query,
            "results": results,
            "count": results.len()
        }))
    }
    
    /// Execute list_files tool
    async fn execute_list_files(
        &self,
        input: &Value,
        request: &ChatRequest,
    ) -> Result<Value> {
        let path = input["path"].as_str().unwrap_or("");
        
        info!("Listing files in: {}", if path.is_empty() { "root" } else { path });
        
        // Get git attachment for project
        let project_id = request.project_id.as_deref()
            .ok_or_else(|| anyhow!("No project context for list_files"))?;
        
        let attachment = sqlx::query!(
            r#"SELECT local_path FROM git_repo_attachments WHERE project_id = ? LIMIT 1"#,
            project_id
        )
        .fetch_optional(&self.app_state.sqlite_pool)
        .await?
        .ok_or_else(|| anyhow!("No git repository attached to project"))?;
        
        let base_path = Path::new(&attachment.local_path).join(path);
        
        // Read directory
        let mut entries = Vec::new();
        let mut dir = tokio::fs::read_dir(&base_path).await?;
        
        while let Some(entry) = dir.next_entry().await? {
            let metadata = entry.metadata().await?;
            let name = entry.file_name().to_string_lossy().to_string();
            
            entries.push(json!({
                "name": name,
                "is_file": metadata.is_file(),
                "is_dir": metadata.is_dir(),
                "size": metadata.len()
            }));
        }
        
        Ok(json!({
            "path": path,
            "entries": entries
        }))
    }
    
    /// Two-phase error fix: analyze with thinking → generate structured fix
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
        
        // Get code intelligence context
        let code_intel = self.get_code_intelligence_for_file(
            &error_context.file_path,
            request.project_id.as_deref()
        ).await?;
        
        if code_intel.is_some() {
            info!("Retrieved code intelligence analysis for {}", error_context.file_path);
        }
        
        // Build context and persona
        let context = self.build_context(&request.session_id, &request.content).await?;
        let persona = self.select_persona(&request.metadata);
        
        // PHASE 1: Deep analysis with thinking
        info!("Phase 1 - Analyzing error with extended thinking");
        
        let analysis_prompt = format!(
            "Analyze this error and plan how to fix it:\n\n\
             Error: {}\n\
             File: {}\n\
             Lines: {}\n\n\
             What's the root cause and what changes are needed?",
            error_context.error_message,
            error_context.file_path,
            file_lines
        );
        
        let (thinking_budget, _) = claude_processor::analyze_message_complexity(&request.content);
        
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
        
        let analysis_response = self.app_state.llm_client
            .post_response_with_retry(analysis_request)
            .await?;
        
        let thinking_content = self.extract_thinking_blocks(&analysis_response);
        let analysis_text = self.extract_text_content(&analysis_response);
        
        info!(
            "Phase 1 complete - thinking: {} chars, analysis: {} chars",
            thinking_content.len(),
            analysis_text.len()
        );
        
        // PHASE 2: Structured fix with code intelligence
        info!("Phase 2 - Generating structured fix");
        
        // Convert FileContext to prompt builder types
        let (code_elements, quality_issues) = if let Some(intel) = code_intel {
            (
                Some(Self::convert_to_code_elements(&intel)),
                Some(Self::convert_to_quality_issues(&intel)),
            )
        } else {
            (None, None)
        };
        
        let fix_system_prompt = UnifiedPromptBuilder::build_code_fix_prompt(
            &persona,
            &context,
            &error_context,
            &file_content,
            request.metadata.as_ref(),
            request.project_id.as_deref(),
            code_elements,
            quality_issues,
        );
        
        // Include analysis as context
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
        
        let fix_request = code_fix_processor::build_code_fix_request(
            &error_context.error_message,
            &error_context.file_path,
            &file_content,
            fix_system_prompt,
            context_messages,
        )?;
        
        let fix_response = self.app_state.llm_client
            .post_response_with_retry(fix_request)
            .await?;
        
        // Extract and validate code fix
        let code_fix = code_fix_processor::extract_code_fix_response(&fix_response)?;
        
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
        
        // Extract metadata and build complete response
        let metadata = crate::llm::structured::processor::extract_metadata(&fix_response, 0)?;
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
        
        info!("Error fix complete and saved for session: {}", request.session_id);
        Ok(complete_response)
    }
    
    /// Get code intelligence context for a file
    async fn get_code_intelligence_for_file(
        &self,
        file_path: &str,
        project_id: Option<&str>,
    ) -> Result<Option<FileContext>> {
        let project_id = match project_id {
            Some(id) => id,
            None => return Ok(None),
        };
        
        // Get git attachment for this project
        let attachment = sqlx::query!(
            r#"SELECT id FROM git_repo_attachments WHERE project_id = ? LIMIT 1"#,
            project_id
        )
        .fetch_optional(&self.app_state.sqlite_pool)
        .await?;
        
        let attachment = match attachment {
            Some(a) => a,
            None => return Ok(None),
        };
        
        // Get file_id from repository_files
        let file_record = sqlx::query!(
            r#"SELECT id FROM repository_files WHERE attachment_id = ? AND file_path = ? LIMIT 1"#,
            attachment.id,
            file_path
        )
        .fetch_optional(&self.app_state.sqlite_pool)
        .await?;
        
        let Some(f) = file_record else {
            debug!("File {} not in repository_files, skipping code intelligence", file_path);
            return Ok(None);
        };
        
        let Some(file_id) = f.id else {
            debug!("File record has null id");
            return Ok(None);
        };
        
        // Get analysis from code intelligence service
        self.app_state.code_intelligence
            .get_file_analysis(file_id)
            .await
    }
    
    /// Convert FileContext to CodeElement vector for prompt builder
    fn convert_to_code_elements(file_context: &FileContext) -> Vec<CodeElement> {
        file_context.elements.iter().map(|elem| {
            CodeElement {
                element_type: elem.element_type.clone(),
                name: elem.name.clone(),
                start_line: elem.start_line as i32,
                end_line: elem.end_line as i32,
                complexity: Some(elem.complexity_score as i32),
                is_async: Some(elem.is_async),
                is_public: Some(elem.visibility == "public"),
                documentation: elem.documentation.clone(),
            }
        }).collect()
    }
    
    /// Convert FileContext to QualityIssue vector for prompt builder
    fn convert_to_quality_issues(file_context: &FileContext) -> Vec<QualityIssue> {
        file_context.quality_issues.iter().map(|issue| {
            QualityIssue {
                severity: issue.severity.clone(),
                category: issue.issue_type.clone(),
                description: issue.description.clone(),
                element_name: Some(issue.title.clone()),
                suggestion: issue.suggested_fix.clone(),
            }
        }).collect()
    }
    
    /// Load complete file from project repository
    async fn load_complete_file(
        &self,
        file_path: &str,
        project_id: Option<&str>,
    ) -> Result<String> {
        // Try to load from project context first
        if let Some(proj_id) = project_id {
            if let Ok(Some(attachment)) = sqlx::query!(
                r#"SELECT local_path FROM git_repo_attachments WHERE project_id = ? LIMIT 1"#,
                proj_id
            )
            .fetch_optional(&self.app_state.sqlite_pool)
            .await
            {
                let local_path = attachment.local_path;
                let full_path = Path::new(&local_path).join(file_path);
                
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
        
        // Fallback: try direct path
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
        self.app_state.memory_service
            .parallel_recall_context(session_id, content, 5, 5)
            .await
    }
    
    async fn build_context_messages(&self, context: &RecallContext) -> Result<Vec<Value>> {
        let mut messages = Vec::new();
        
        for memory in &context.recent {
            messages.push(json!({
                "role": if memory.role == "user" { "user" } else { "assistant" },
                "content": memory.content
            }));
        }
        
        Ok(messages)
    }
    
    fn select_persona(&self, _metadata: &Option<MessageMetadata>) -> PersonaOverlay {
        PersonaOverlay::Default
    }
    
    fn extract_thinking_blocks(&self, response: &Value) -> String {
        let mut thinking = String::new();
        
        if let Some(content) = response["content"].as_array() {
            for block in content {
                if block["type"] == "thinking" {
                    if let Some(text) = block["thinking"].as_str() {
                        if !thinking.is_empty() {
                            thinking.push_str("\n\n");
                        }
                        thinking.push_str(text);
                    }
                }
            }
        }
        
        thinking
    }
    
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
}
