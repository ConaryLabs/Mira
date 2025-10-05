// src/api/ws/chat/unified_handler.rs
// Slim routing layer - all tool logic delegated to src/tools/*

use std::sync::Arc;
use anyhow::{Result, anyhow};
use serde_json::{json, Value};
use tracing::info;

use crate::api::ws::message::MessageMetadata;
use crate::llm::structured::{CompleteResponse, claude_processor};
use crate::llm::structured::tool_schema::*;
use crate::llm::provider::ChatMessage;
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
        info!("Processing message for session: {}", request.session_id);
        
        // Go straight to tool execution loop - let the LLM decide what to do
        self.handle_chat_with_tools(request).await
    }
    
    /// Process chat with tool execution loop
    async fn handle_chat_with_tools(
        &self,
        request: ChatRequest,
    ) -> Result<CompleteResponse> {
        // Save user message first - FIXED: Use proper MemoryService with analysis pipeline
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
        
        // FIXED: Conditional tools - only offer project-dependent tools when project exists
        let tools = if request.project_id.is_some() {
            vec![
                get_response_tool_schema(),
                get_read_file_tool_schema(),
                get_code_search_tool_schema(),
                get_list_files_tool_schema(),
            ]
        } else {
            // No project = only allow responding, no file/code operations
            vec![get_response_tool_schema()]
        };
        
        // Build initial message history with simple text content
        let mut chat_messages = Vec::new();
        for entry in context.recent.iter().rev() {
            chat_messages.push(ChatMessage::text(
                if entry.role == "user" { "user" } else { "assistant" },
                entry.content.clone(),
            ));
        }
        // Add current user message
        chat_messages.push(ChatMessage::text("user", request.content.clone()));
        
        // Tool execution loop - INCREASED: 10 -> 20 iterations
        for iteration in 0..20 {
            info!("Tool loop iteration {}", iteration);
            
            let raw_response = self.app_state.llm.chat_with_tools(
                chat_messages.clone(),
                system_prompt.clone(),
                tools.clone(),
                None,  // No forced tool choice
            ).await?;
            
            // Check stop_reason
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
                
                // Add assistant message with tool calls (preserve content blocks)
                if let Some(content) = raw_response["content"].clone().as_array() {
                    chat_messages.push(ChatMessage::blocks("assistant", Value::Array(content.clone())));
                }
                
                // Add user message with tool results
                chat_messages.push(ChatMessage::blocks("user", Value::Array(tool_results)));
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
    
    /// FIXED: Use proper MemoryService with analysis pipeline instead of raw SQL
    async fn save_user_message(&self, request: &ChatRequest) -> Result<i64> {
        self.app_state.memory_service
            .save_user_message(
                &request.session_id,
                &request.content,
                request.project_id.as_deref()
            )
            .await
    }
    
    /// FIXED: Use MemoryService instead of raw SQL for consistency
    async fn build_context(
        &self,
        session_id: &str,
        _user_message: &str,
    ) -> Result<RecallContext> {
        let recent = self.app_state.memory_service
            .get_recent_context(session_id, 5)
            .await?;
        
        Ok(RecallContext {
            recent,
            semantic: vec![],
        })
    }
    
    fn select_persona(&self, _metadata: &Option<MessageMetadata>) -> PersonaOverlay {
        PersonaOverlay::Default  // FIXED: Was PersonaOverload
    }
}
