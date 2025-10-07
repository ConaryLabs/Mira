// src/api/ws/chat/unified_handler.rs
// Unified chat handler with tool execution loop
// Delegates all tool logic to src/tools/* for clean separation
//
// PHASE 1.3 UPDATE: Enhanced build_context with summary retrieval and increased limits
// PHASE 3.1-3.2 UPDATE: Added efficiency tools (get_project_context, read_files, write_files)
// PHASE 3.3 UPDATE: Added session tool cache for expensive operations
// PHASE 5 UPDATE: Doubled iteration limit from 20 to 50

use std::sync::Arc;
use std::time::{Duration, Instant};
use std::collections::HashMap;
use anyhow::{Result, anyhow};
use serde_json::{json, Value};
use tracing::{info, warn, debug};

use crate::api::ws::message::MessageMetadata;
use crate::llm::structured::{CompleteResponse, claude_processor};
use crate::llm::structured::tool_schema::*;
use crate::llm::structured::types::{StructuredLLMResponse, MessageAnalysis};
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

// ===== PHASE 3.3: SESSION TOOL CACHE =====

/// Cached tool result with timestamp
#[derive(Clone)]
struct CachedToolResult {
    result: Value,
    cached_at: Instant,
}

/// Session-level cache for expensive tool operations
struct SessionToolCache {
    // Cache key: (project_id, tool_name)
    cache: HashMap<(String, String), CachedToolResult>,
    // TTL for different tool types
    project_context_ttl: Duration,
}

impl SessionToolCache {
    fn new() -> Self {
        Self {
            cache: HashMap::new(),
            project_context_ttl: Duration::from_secs(300), // 5 minutes
        }
    }
    
    /// Get cached result if still valid
    fn get(&self, project_id: &str, tool_name: &str, ttl: Duration) -> Option<Value> {
        let key = (project_id.to_string(), tool_name.to_string());
        
        if let Some(cached) = self.cache.get(&key) {
            if cached.cached_at.elapsed() < ttl {
                debug!("Cache HIT for {}:{} (age: {:?})", tool_name, project_id, cached.cached_at.elapsed());
                return Some(cached.result.clone());
            } else {
                debug!("Cache EXPIRED for {}:{} (age: {:?})", tool_name, project_id, cached.cached_at.elapsed());
            }
        } else {
            debug!("Cache MISS for {}:{}", tool_name, project_id);
        }
        
        None
    }
    
    /// Store result in cache
    fn set(&mut self, project_id: &str, tool_name: &str, result: Value) {
        let key = (project_id.to_string(), tool_name.to_string());
        
        self.cache.insert(key, CachedToolResult {
            result,
            cached_at: Instant::now(),
        });
        
        debug!("Cached result for {}:{}", tool_name, project_id);
    }
    
    /// Check if tool should be cached
    fn is_cacheable(&self, tool_name: &str) -> bool {
        matches!(tool_name, "get_project_context")
    }
    
    /// Get TTL for tool
    fn get_ttl(&self, tool_name: &str) -> Duration {
        match tool_name {
            "get_project_context" => self.project_context_ttl,
            _ => Duration::from_secs(0), // No cache
        }
    }
}

// ===== END PHASE 3.3 =====

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
        
        // Route to tool execution loop - LLM decides what tools to use
        self.handle_chat_with_tools(request).await
    }
    
    /// Main chat processing with tool execution loop
    /// Allows Claude to use tools iteratively until it responds with respond_to_user
    async fn handle_chat_with_tools(
        &self,
        request: ChatRequest,
    ) -> Result<CompleteResponse> {
        // Save user message and run through analysis pipeline
        let user_message_id = self.save_user_message(&request).await?;
        
        // Build context (recent messages + semantic search + summaries)
        let context = self.build_context(&request.session_id, &request.content).await?;
        let persona = self.select_persona(&request.metadata);
        
        let system_prompt = UnifiedPromptBuilder::build_system_prompt(
            &persona,
            &context,
            None,
            request.metadata.as_ref(),
            request.project_id.as_deref(),
        );
        
        // PHASE 3: Conditional tools - added efficiency tools
        let tools = if request.project_id.is_some() {
            vec![
                // Core response tool (always required)
                get_response_tool_schema(),
                
                // Existing single-operation tools
                get_read_file_tool_schema(),
                get_code_search_tool_schema(),
                get_list_files_tool_schema(),
                
                // PHASE 3: Efficiency tools - batch operations
                get_project_context_tool_schema(),  // Complete project overview in 1 call
                get_read_files_tool_schema(),       // Batch read multiple files
                get_write_files_tool_schema(),      // Batch write multiple files
            ]
        } else {
            // No project = only allow responding, no file/code operations
            vec![get_response_tool_schema()]
        };
        
        // Build initial message history from context
        let mut chat_messages = Vec::new();
        for entry in context.recent.iter().rev() {
            chat_messages.push(ChatMessage::text(
                if entry.role == "user" { "user" } else { "assistant" },
                entry.content.clone(),
            ));
        }
        // Add current user message
        chat_messages.push(ChatMessage::text("user", request.content.clone()));
        
        // PHASE 3.3: Initialize session tool cache
        let mut tool_cache = SessionToolCache::new();
        
        // PHASE 5: Tool execution loop - doubled from 20 to 50 iterations
        // With better tools and context, Claude should iterate less,
        // but give it room when needed for complex multi-step tasks
        for iteration in 0..50 {
            info!("Tool loop iteration {}", iteration);
            
            let raw_response = self.app_state.llm.chat_with_tools(
                chat_messages.clone(),
                system_prompt.clone(),
                tools.clone(),
                None,  // No forced tool choice - Claude decides
            ).await?;
            
            // Check stop reason
            let stop_reason = raw_response["stop_reason"].as_str().unwrap_or("");
            
            if stop_reason == "end_turn" {
                // Claude finished without calling a tool
                // Try to extract respond_to_user if present, otherwise provide fallback
                
                let structured = if claude_processor::has_tool_calls(&raw_response) {
                    // Normal case: has respond_to_user tool call
                    claude_processor::extract_claude_content_from_tool(&raw_response)?
                } else {
                    // Edge case: Claude thought but didn't respond
                    // Extract thinking content if available
                    let thinking_summary = if let Some(content) = raw_response["content"].as_array() {
                        content.iter()
                            .filter(|block| block["type"] == "thinking")
                            .filter_map(|block| block["thinking"].as_str())
                            .collect::<Vec<_>>()
                            .join("\n\n")
                    } else {
                        String::new()
                    };
                    
                    let fallback_message = if !thinking_summary.is_empty() {
                        format!("I processed your message but didn't generate a response. My thoughts were:\n\n{}", thinking_summary)
                    } else {
                        "I processed your message but didn't generate a response. Please try rephrasing your request.".to_string()
                    };
                    
                    info!("Claude ended turn without respond_to_user - using fallback response");
                    
                    // Create a minimal structured response for the fallback
                    StructuredLLMResponse {
                        output: fallback_message,
                        analysis: MessageAnalysis {
                            salience: 0.5,
                            topics: vec![],
                            contains_code: false,
                            routed_to_heads: vec![],
                            language: String::new(),
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
                        schema_name: Some("fallback_response".to_string()),
                        validation_status: Some("valid".to_string()),
                    }
                };
                
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
            
            // stop_reason == "tool_use" - Claude called tools
            let mut tool_results = Vec::new();
            
            if let Some(content) = raw_response["content"].as_array() {
                for block in content {
                    if block["type"] == "tool_use" {
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
                        
                        // PHASE 3.3: Check cache for expensive tools
                        let result = if let Some(project_id) = request.project_id.as_deref() {
                            if tool_cache.is_cacheable(tool_name) {
                                let ttl = tool_cache.get_ttl(tool_name);
                                
                                if let Some(cached_result) = tool_cache.get(project_id, tool_name, ttl) {
                                    Ok(cached_result)
                                } else {
                                    // Execute tool and cache result
                                    match self.execute_tool(tool_name, tool_input, &request).await {
                                        Ok(r) => {
                                            tool_cache.set(project_id, tool_name, r.clone());
                                            Ok(r)
                                        }
                                        Err(e) => Err(e)
                                    }
                                }
                            } else {
                                // Not cacheable - execute directly
                                self.execute_tool(tool_name, tool_input, &request).await
                            }
                        } else {
                            // No project_id - execute directly
                            self.execute_tool(tool_name, tool_input, &request).await
                        };
                        
                        // Handle result or error
                        let result = match result {
                            Ok(r) => r,
                            Err(e) => {
                                info!("Tool execution error (returned to LLM): {}", e);
                                json!({
                                    "error": e.to_string(),
                                    "status": "failed",
                                    "hint": "This operation failed. Try a different approach."
                                })
                            }
                        };
                        
                        tool_results.push(json!({
                            "type": "tool_result",
                            "tool_use_id": block["id"],
                            "content": result.to_string()
                        }));
                    }
                }
                
                // Add assistant message with tool calls (only if content exists)
                // This prevents "all messages must have non-empty content" error
                if let Some(content) = raw_response["content"].clone().as_array() {
                    if !content.is_empty() {
                        // Verify content blocks are valid
                        let has_valid_content = content.iter().any(|block| {
                            if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                                !text.trim().is_empty()
                            } else if block.get("type") == Some(&json!("tool_use")) {
                                true
                            } else if block.get("type") == Some(&json!("thinking")) {
                                true
                            } else {
                                false
                            }
                        });
                        
                        if has_valid_content {
                            chat_messages.push(ChatMessage::blocks("assistant", Value::Array(content.clone())));
                        } else {
                            warn!("Skipping assistant message with empty content blocks");
                        }
                    }
                }
                
                // Add user message with tool results (only if results exist)
                if !tool_results.is_empty() {
                    chat_messages.push(ChatMessage::blocks("user", Value::Array(tool_results)));
                } else {
                    // No tool results means tool loop failed - break to prevent infinite loop
                    warn!("No tool results after tool_use - breaking tool loop to prevent infinite iterations");
                    break;
                }
            }
        }
        
        Err(anyhow!("Tool loop exceeded max iterations"))
    }
    
    /// Execute tool via ToolExecutor
    /// All tool logic is delegated to src/tools/executor.rs
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
    
    /// Save user message through MemoryService
    /// Runs through analysis pipeline (sentiment, topics, salience, etc.)
    async fn save_user_message(&self, request: &ChatRequest) -> Result<i64> {
        self.app_state.memory_service
            .save_user_message(
                &request.session_id,
                &request.content,
                request.project_id.as_deref()
            )
            .await
    }
    
    // ===== PHASE 1.3: UPDATED BUILD_CONTEXT WITH SUMMARIES =====
    
    /// Build layered context for recall
    /// 
    /// PHASE 1.3 CHANGES:
    /// - Increased limits from 5/10 to 20/15
    /// - Added rolling summary retrieval (last 100 messages)
    /// - Added session summary retrieval (entire conversation)
    /// - Enhanced logging to show what context was built
    async fn build_context(
        &self,
        session_id: &str,
        user_message: &str,
    ) -> Result<RecallContext> {
        info!("Building layered context with summaries for session: {}", session_id);
        
        // Get recent + semantic (INCREASED from 5/10 to 20/15)
        let mut context = self.app_state.memory_service
            .parallel_recall_context(
                session_id,
                user_message,  // Embedded and used for vector search
                20,  // recent_count: INCREASED from 5
                15,  // semantic_count: INCREASED from 10
            )
            .await?;
        
        // Add rolling summary (last 100 messages, ~2,500 tokens) if exists
        context.rolling_summary = self.app_state.memory_service
            .get_rolling_summary(session_id)
            .await?;
        
        // Add session summary (entire conversation, ~3,000 tokens) if exists
        context.session_summary = self.app_state.memory_service
            .get_session_summary(session_id)
            .await?;
        
        info!(
            "Context built: {} recent, {} semantic, rolling={}, session={}",
            context.recent.len(),
            context.semantic.len(),
            context.rolling_summary.is_some(),
            context.session_summary.is_some()
        );
        
        Ok(context)
    }
    
    // ===== END PHASE 1.3 =====
    
    /// Select persona based on metadata
    /// Currently returns default persona, can be extended for context-aware personas
    fn select_persona(&self, _metadata: &Option<MessageMetadata>) -> PersonaOverlay {
        PersonaOverlay::Default
    }
}
