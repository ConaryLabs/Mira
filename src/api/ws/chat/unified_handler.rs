// src/api/ws/chat/unified_handler.rs
// Unified chat handler with tool execution loop

use std::sync::Arc;
use std::time::{Duration, Instant};
use std::collections::HashMap;
use anyhow::{Result, anyhow};
use serde_json::{json, Value};
use tracing::{info, warn, debug};

use crate::api::ws::message::MessageMetadata;
use crate::llm::structured::{CompleteResponse, has_tool_calls, extract_claude_content_from_tool, extract_claude_metadata};
use crate::llm::structured::tool_schema::*;
use crate::llm::structured::types::{StructuredLLMResponse, MessageAnalysis};
use crate::llm::provider::Message;
use crate::memory::storage::sqlite::structured_ops::{save_structured_response, process_embeddings};
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

// ===== SESSION TOOL CACHE =====

#[derive(Clone)]
struct CachedToolResult {
    result: Value,
    cached_at: Instant,
}

struct SessionToolCache {
    cache: HashMap<(String, String), CachedToolResult>,
    project_context_ttl: Duration,
}

impl SessionToolCache {
    fn new() -> Self {
        Self {
            cache: HashMap::new(),
            project_context_ttl: Duration::from_secs(300),
        }
    }
    
    fn get(&self, project_id: &str, tool_name: &str, ttl: Duration) -> Option<Value> {
        let key = (project_id.to_string(), tool_name.to_string());
        
        if let Some(cached) = self.cache.get(&key) {
            if cached.cached_at.elapsed() < ttl {
                debug!("Cache HIT for {}:{}", tool_name, project_id);
                return Some(cached.result.clone());
            }
        }
        None
    }
    
    fn set(&mut self, project_id: &str, tool_name: &str, result: Value) {
        let key = (project_id.to_string(), tool_name.to_string());
        self.cache.insert(key, CachedToolResult {
            result,
            cached_at: Instant::now(),
        });
    }
    
    fn is_cacheable(&self, tool_name: &str) -> bool {
        matches!(tool_name, "get_project_context")
    }
    
    fn get_ttl(&self, tool_name: &str) -> Duration {
        match tool_name {
            "get_project_context" => self.project_context_ttl,
            _ => Duration::from_secs(0),
        }
    }
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
        self.handle_chat_with_tools(request).await
    }
    
    async fn handle_chat_with_tools(
        &self,
        request: ChatRequest,
    ) -> Result<CompleteResponse> {
        // Save user message
        let user_message_id = self.save_user_message(&request).await?;
        
        // Build context
        let context = self.build_context(&request.session_id, &request.content).await?;
        let persona = self.select_persona(&request.metadata);
        
        let system_prompt = UnifiedPromptBuilder::build_system_prompt(
            &persona,
            &context,
            None,
            request.metadata.as_ref(),
            request.project_id.as_deref(),
        );
        
        // Define tools
        let tools = if request.project_id.is_some() {
            vec![
                get_response_tool_schema(),
                get_create_artifact_tool_schema(),
                get_code_fix_tool_schema(),
                get_read_file_tool_schema(),
                get_code_search_tool_schema(),
                get_list_files_tool_schema(),
                get_project_context_tool_schema(),
                get_read_files_tool_schema(),
                get_write_files_tool_schema(),
            ]
        } else {
            vec![get_response_tool_schema()]
        };
        
        // Build message history - FIXED: Message is just a struct
        let mut chat_messages = Vec::new();
        for entry in context.recent.iter().rev() {
            chat_messages.push(Message {
                role: if entry.role == "user" { "user".to_string() } else { "assistant".to_string() },
                content: entry.content.clone(),
            });
        }
        chat_messages.push(Message {
            role: "user".to_string(),
            content: request.content.clone(),
        });
        
        // Initialize cache and artifacts
        let mut tool_cache = SessionToolCache::new();
        let mut collected_artifacts: Vec<Value> = Vec::new();
        
        // Tool execution loop (50 iterations max)
        for iteration in 0..50 {
            info!("Tool loop iteration {}", iteration);
            
            let raw_response = self.app_state.llm.chat_with_tools(
                chat_messages.clone(),
                system_prompt.clone(),
                tools.clone(),
                None,
            ).await?;
            
            // Check if response has tool calls
            if !has_tool_calls(&raw_response) {
                // No tool calls - model finished
                warn!("Model ended without tool calls on iteration {}", iteration);
                
                if iteration == 0 {
                    // Force continuation on first iteration
                    let reminder = "⚠️ You MUST call the respond_to_user tool. Please respond now.";
                    chat_messages.push(Message {
                        role: "user".to_string(),
                        content: reminder.to_string(),
                    });
                    continue;
                }
                
                // Create fallback response
                let structured = StructuredLLMResponse {
                    output: "I processed your message but didn't generate a response.".to_string(),
                    analysis: MessageAnalysis {
                        salience: 0.5,
                        topics: vec![],
                        contains_code: false,
                        routed_to_heads: vec![],
                        language: "en".to_string(),
                        mood: None,
                        intensity: None,
                        intent: Some("clarification_needed".to_string()),
                        summary: None,
                        relationship_impact: None,
                        programming_lang: None,
                        contains_error: false,
                        error_file: None,
                        error_severity: None,
                        error_type: None,
                    },
                    reasoning: None,
                    schema_name: Some("fallback".to_string()),
                    validation_status: Some("valid".to_string()),
                };
                
                let metadata = extract_claude_metadata(&raw_response, 0)?;
                
                return Ok(CompleteResponse {
                    structured,
                    metadata,
                    raw_response: raw_response.raw_response.clone(),
                    artifacts: if collected_artifacts.is_empty() { None } else { Some(collected_artifacts) },
                });
            }
            
            // Process tool calls
            let mut tool_results = Vec::new();
            let mut found_respond = false;
            
            for func_call in &raw_response.function_calls {
                let tool_name = &func_call.name;
                let tool_input = &func_call.arguments;
                
                info!("Executing tool: {}", tool_name);
                
                // Check for respond_to_user (final response)
                if tool_name == "respond_to_user" {
                    found_respond = true;
                    
                    let structured = extract_claude_content_from_tool(&raw_response)?;
                    let metadata = extract_claude_metadata(&raw_response, 0)?;
                    
                    let complete_response = CompleteResponse {
                        structured,
                        metadata,
                        raw_response: raw_response.raw_response.clone(),
                        artifacts: if collected_artifacts.is_empty() { None } else { Some(collected_artifacts) },
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
                    
                    return Ok(complete_response);
                }
                
                // Execute tool with caching
                let result = if let Some(project_id) = request.project_id.as_deref() {
                    if tool_cache.is_cacheable(tool_name) {
                        let ttl = tool_cache.get_ttl(tool_name);
                        
                        if let Some(cached) = tool_cache.get(project_id, tool_name, ttl) {
                            Ok(cached)
                        } else {
                            match self.execute_tool(tool_name, tool_input, &request).await {
                                Ok(r) => {
                                    tool_cache.set(project_id, tool_name, r.clone());
                                    Ok(r)
                                }
                                Err(e) => Err(e)
                            }
                        }
                    } else {
                        self.execute_tool(tool_name, tool_input, &request).await
                    }
                } else {
                    self.execute_tool(tool_name, tool_input, &request).await
                };
                
                // Handle result
                let result_value = match result {
                    Ok(r) => {
                        // Collect artifacts
                        if tool_name == "create_artifact" {
                            if let Some(artifact) = r.get("artifact") {
                                collected_artifacts.push(artifact.clone());
                            }
                        } else if tool_name == "provide_code_fix" {
                            if let Some(artifacts_array) = r.get("artifacts").and_then(|a| a.as_array()) {
                                collected_artifacts.extend(artifacts_array.iter().cloned());
                            }
                        }
                        r
                    }
                    Err(e) => {
                        info!("Tool error: {}", e);
                        json!({
                            "error": e.to_string(),
                            "status": "failed"
                        })
                    }
                };
                
                tool_results.push(json!({
                    "type": "tool_result",
                    "tool_use_id": func_call.id,
                    "content": result_value.to_string()
                }));
            }
            
            // If we found respond_to_user, we already returned above
            if found_respond {
                continue;
            }
            
            // Add assistant message (as text for simplicity) - FIXED
            chat_messages.push(Message {
                role: "assistant".to_string(),
                content: raw_response.text_output.clone(),
            });
            
            // Add tool results - FIXED
            if !tool_results.is_empty() {
                let results_text = serde_json::to_string(&tool_results)?;
                chat_messages.push(Message {
                    role: "user".to_string(),
                    content: results_text,
                });
            } else {
                warn!("No tool results - breaking loop");
                break;
            }
        }
        
        Err(anyhow!("Tool loop exceeded max iterations"))
    }
    
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
