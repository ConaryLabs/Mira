// src/api/ws/chat/unified_handler.rs
// Slim routing layer - all tool logic delegated to src/tools/*

use std::sync::Arc;
use anyhow::{Result, anyhow};
use serde_json::{json, Value};
use tracing::{info, warn};
use chrono::Utc;

use crate::api::ws::message::MessageMetadata;
use crate::llm::structured::{CompleteResponse, code_fix_processor, claude_processor};
use crate::llm::structured::code_fix_processor::ErrorContext;
use crate::llm::structured::tool_schema::*;
use crate::llm::provider::ChatMessage;  // NEW: Import provider types
use crate::memory::storage::sqlite::structured_ops::save_structured_response;
use crate::memory::features::recall_engine::RecallContext;
use crate::memory::core::types::MemoryEntry;
use crate::persona::PersonaOverlay;
use crate::prompt::unified_builder::UnifiedPromptBuilder;
use crate::state::AppState;
use crate::tools::{CodeFixHandler, file_ops};

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
        
        // Check for error patterns first
        if let Some(mut error_context) = code_fix_processor::detect_error_context(&request.content) {
            if let Some(project_id) = &request.project_id {
                info!("Detected error in file: {}", error_context.file_path);
                
                // Load file and update line count
                if let Ok(content) = file_ops::load_complete_file(
                    &self.app_state.sqlite_pool,
                    &error_context.file_path,
                    project_id
                ).await {
                    error_context.original_line_count = content.lines().count();
                }
                
                return self.handle_error_fix_with_handler(request, error_context).await;
            } else {
                warn!("Error detected but no project context available");
            }
        }
        
        // Use tool execution loop for regular chat
        self.handle_chat_with_tools(request).await
    }
    
    /// Delegate error fixing to CodeFixHandler
    async fn handle_error_fix_with_handler(
        &self,
        request: ChatRequest,
        error_context: ErrorContext,
    ) -> Result<CompleteResponse> {
        info!("Delegating to CodeFixHandler");

        // Load complete file
        let file_content = file_ops::load_complete_file(
            &self.app_state.sqlite_pool,
            &error_context.file_path,
            request.project_id.as_deref().unwrap()
        ).await?;

        // Build context and persona
        let context = self.build_context(&request.session_id, &request.content).await?;
        let persona = self.select_persona(&request.metadata);

        // UPDATED: Use llm instead of llm_client
        let handler = CodeFixHandler::new(
            self.app_state.llm.clone(),
            self.app_state.code_intelligence.clone(),
            self.app_state.sqlite_pool.clone(),
        );

        handler.handle_error_fix(
            &error_context,
            &file_content,
            &context,
            &persona,
            request.project_id.as_deref().unwrap(),
            request.metadata.as_ref(),
        ).await
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
        
        let tools = vec![
            get_response_tool_schema(),
            get_read_file_tool_schema(),
            get_code_search_tool_schema(),
            get_list_files_tool_schema(),
        ];
        
        let (_thinking_budget, _temperature) = claude_processor::analyze_message_complexity(&request.content);
        
        let mut context_messages = self.build_context_messages(&context).await?;
        context_messages.push(json!({
            "role": "user",
            "content": request.content
        }));
        
        // Tool execution loop (max 10 iterations)
        for iteration in 0..10 {
            info!("Tool loop iteration {}", iteration);
            
            // UPDATED: Convert to provider format
            let provider_messages: Vec<ChatMessage> = context_messages
                .iter()
                .filter_map(|m| {
                    Some(ChatMessage {
                        role: m["role"].as_str()?.to_string(),
                        content: if let Some(text) = m["content"].as_str() {
                            text.to_string()
                        } else {
                            // Handle array content (tool results)
                            serde_json::to_string(&m["content"]).ok()?
                        },
                    })
                })
                .collect();
            
            // UPDATED: Call provider with tools
            let raw_response = self.app_state.llm
                .chat_with_tools(
                    provider_messages,
                    system_prompt.clone(),
                    tools.clone(),
                )
                .await?;
            
            // Response is already in Claude format from conversion layer
            let stop_reason = raw_response["stop_reason"].as_str().unwrap_or("");
            
            if stop_reason == "end_turn" {
                // No more tools, extract response
                let structured = claude_processor::extract_claude_content_from_tool(&raw_response)?;
                let metadata = claude_processor::extract_claude_metadata(&raw_response, 0)?;
                
                let complete_response = CompleteResponse {
                    structured,
                    metadata,
                    raw_response,
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
            
            if stop_reason == "tool_use" {
                // Extract and execute tools
                let mut tool_results = Vec::new();
                
                if let Some(content) = raw_response["content"].as_array() {
                    for block in content {
                        if block["type"] != "tool_use" {
                            continue;
                        }
                        
                        let tool_name = block["name"].as_str().unwrap_or("");
                        let tool_input = &block["input"];
                        
                        info!("Executing tool: {}", tool_name);
                        
                        // Check for respond_to_user (final response)
                        if tool_name == "respond_to_user" {
                            let structured = claude_processor::extract_claude_content_from_tool(&raw_response)?;
                            let metadata = claude_processor::extract_claude_metadata(&raw_response, 0)?;
                            
                            let complete_response = CompleteResponse {
                                structured,
                                metadata,
                                raw_response,
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
                        
                        // Execute tool via ToolExecutor
                        let result = self.execute_tool(tool_name, tool_input, &request).await?;
                        
                        tool_results.push(json!({
                            "type": "tool_result",
                            "tool_use_id": block["id"],
                            "content": result.to_string()
                        }));
                    }
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
        
        Err(anyhow!("Tool loop exceeded max iterations"))
    }
    
    /// Execute tool via ToolExecutor (delegated to src/tools/executor.rs)
    async fn execute_tool(
        &self,
        tool_name: &str,
        input: &Value,
        request: &ChatRequest,
    ) -> Result<Value> {
        let executor = crate::tools::ToolExecutor::new(
            self.app_state.code_intelligence.clone(),
            self.app_state.sqlite_pool.clone(),
        );

        let project_id = request.project_id.as_deref()
            .ok_or_else(|| anyhow!("No project context for {}", tool_name))?;

        executor.execute_tool(tool_name, input, project_id).await
    }
    
    async fn save_user_message(&self, request: &ChatRequest) -> Result<i64> {
        let user_id = sqlx::query_scalar::<_, i64>(
            "INSERT INTO memory_entries (session_id, role, content, timestamp) 
             VALUES (?, 'user', ?, ?) 
             RETURNING id"
        )
        .bind(&request.session_id)
        .bind(&request.content)
        .bind(Utc::now().timestamp())
        .fetch_one(&self.app_state.sqlite_pool)
        .await?;
        
        Ok(user_id)
    }
    
    async fn build_context(
        &self,
        session_id: &str,
        _user_message: &str,
    ) -> Result<RecallContext> {
        // Fetch recent messages from database
        let recent = sqlx::query!(
            r#"
            SELECT id, role, content, timestamp
            FROM memory_entries
            WHERE session_id = ?
            ORDER BY timestamp DESC
            LIMIT 5
            "#,
            session_id
        )
        .fetch_all(&self.app_state.sqlite_pool)
        .await?;
        
        let recent_entries = recent.into_iter().map(|row| {
            MemoryEntry {
                id: row.id,
                session_id: session_id.to_string(),
                response_id: None,
                parent_id: None,
                role: row.role,
                content: row.content,
                timestamp: chrono::DateTime::from_timestamp(row.timestamp, 0).unwrap_or(Utc::now()),
                tags: None,
                mood: None,
                intensity: None,
                salience: None,
                original_salience: None,
                intent: None,
                topics: None,
                summary: None,
                relationship_impact: None,
                contains_code: None,
                language: None,
                programming_lang: None,
                analyzed_at: None,
                analysis_version: None,
                routed_to_heads: None,
                last_recalled: None,
                recall_count: None,
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
                embedding: None,
                embedding_heads: None,
                qdrant_point_ids: None,
            }
        }).collect();
        
        Ok(RecallContext {
            recent: recent_entries,
            semantic: vec![],
        })
    }
    
    async fn build_context_messages(&self, context: &RecallContext) -> Result<Vec<Value>> {
        let mut messages = Vec::new();
        
        for entry in context.recent.iter().rev() {
            messages.push(json!({
                "role": if entry.role == "user" { "user" } else { "assistant" },
                "content": entry.content
            }));
        }
        
        Ok(messages)
    }
    
    fn select_persona(&self, _metadata: &Option<MessageMetadata>) -> PersonaOverlay {
        PersonaOverlay::Default
    }
}
