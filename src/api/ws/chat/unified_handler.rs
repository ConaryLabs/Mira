// src/api/ws/chat/unified_handler.rs
// Unified chat handler that consolidates all chat processing logic into a single pipeline.
// Handles both tool-enabled and simple chats through the same flow.

use std::sync::Arc;
use anyhow::Result;
use futures::{Stream, StreamExt};
use serde::Serialize;
use serde_json::{json, Value};
use tracing::{debug, info, warn, error};

use crate::api::ws::message::MessageMetadata;
use crate::config::CONFIG;
use crate::llm::responses::{ResponsesManager, types::Message};
use crate::llm::responses::thread::ThreadManager;
use crate::memory::recall::RecallContext;
use crate::persona::PersonaOverlay;
use crate::prompt::unified_builder::UnifiedPromptBuilder;
use crate::state::AppState;
use crate::tools::executor::ToolExecutor;
use crate::tools::ToolExecutorExt;

/// Events emitted during chat processing
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum ChatEvent {
    /// Text content chunk from the LLM
    Content { text: String },
    
    /// Tool execution notification
    ToolExecution { 
        tool_name: String, 
        status: String 
    },
    
    /// Tool result
    ToolResult {
        tool_name: String,
        result: Value,
    },
    
    /// Completion metadata
    Complete {
        mood: Option<String>,
        salience: Option<f32>,
        tags: Option<Vec<String>>,
    },
    
    /// Stream finished
    Done,
    
    /// Error occurred
    Error { message: String },
}

/// Configuration for a chat request
#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub content: String,
    pub project_id: Option<String>,
    pub metadata: Option<MessageMetadata>,
    pub session_id: String,
    pub require_json: bool,
}

/// Unified chat handler that processes all chat messages through a single pipeline
pub struct UnifiedChatHandler {
    app_state: Arc<AppState>,
    tool_executor: ToolExecutor,
    response_manager: Arc<ResponsesManager>,
    thread_manager: Arc<ThreadManager>,
}

impl UnifiedChatHandler {
    pub fn new(app_state: Arc<AppState>) -> Self {
        // Create ThreadManager for response_id tracking
        let thread_manager = Arc::new(ThreadManager::new(
            CONFIG.history_message_cap,
            CONFIG.history_token_limit,
        ));
        
        // Create ResponsesManager WITH ThreadManager for proper response_id tracking
        let response_manager = Arc::new(
            ResponsesManager::with_thread_manager(
                app_state.llm_client.clone(),
                thread_manager.clone(),
            )
        );
        
        Self {
            app_state,
            tool_executor: ToolExecutor::new(),
            response_manager,
            thread_manager,
        }
    }
    
    /// Main entry point for all chat messages
    pub async fn handle_message(
        &self,
        request: ChatRequest,
    ) -> Result<impl Stream<Item = Result<ChatEvent>> + Send> {
        // 1. Determine if tools should be used
        let use_tools = self.tool_executor.should_use_tools(&request.metadata);
        
        // Log appropriately based on path
        if use_tools {
            info!("Processing tool-enabled chat for session: {}", request.session_id);
        } else {
            info!("Processing simple chat message: {}", 
                request.content.chars().take(80).collect::<String>());
        }
        
        // 2. Build REAL context using memory service
        let context = self.build_context(&request.session_id, &request.content).await?;
        debug!("Context built: {} recent, {} semantic", 
            context.recent.len(), 
            context.semantic.len()
        );
        
        // 3. Get tools if needed
        let tools = if use_tools {
            Some(crate::tools::definitions::get_enabled_tools())
        } else {
            None
        };
        debug!("Tools enabled: {} (found {} tools)", 
            use_tools, 
            tools.as_ref().map_or(0, |t| t.len())
        );
        
        // 4. Select persona (currently always Default, but extensible)
        let persona = self.select_persona(&request.metadata);
        debug!("Selected persona: {}", persona);
        
        // 5. Build the unified system prompt
        let system_prompt = UnifiedPromptBuilder::build_system_prompt(
            &persona,
            &context,
            tools.as_deref(),
            request.metadata.as_ref(),
            request.project_id.as_deref(),
            request.require_json,
        );
        debug!("System prompt built: {} chars", system_prompt.len());
        
        // 6. Create messages array (same structure for both paths)
        let messages = vec![
            Message {
                role: "system".to_string(),
                content: Some(system_prompt),
                ..Default::default()
            },
            Message {
                role: "user".to_string(),
                content: Some(request.content.clone()),
                ..Default::default()
            }
        ];
        
        // 7. Build parameters (GPT-5 best practices - flat structure)
        let mut parameters = json!({
            "verbosity": CONFIG.verbosity,
            "reasoning_effort": CONFIG.reasoning_effort,
            "max_output_tokens": CONFIG.max_output_tokens,
        });
        
        if let Some(tool_list) = tools.as_deref() {
            let tool_values: Vec<Value> = tool_list.iter()
                .map(|t| serde_json::to_value(t).unwrap_or(json!({})))
                .collect();
            parameters["tools"] = json!(tool_values);
            parameters["tool_choice"] = json!("auto");
        }
        
        // 8. Save user message to memory BEFORE processing
        if let Err(e) = self.app_state.memory_service.save_user_message(
            &request.session_id,
            &request.content,
            request.project_id.as_deref()
        ).await {
            warn!("Failed to save user message to memory: {}", e);
        }
        
        // 9. Get previous_response_id if we have thread manager
        let previous_response_id = self.thread_manager
            .get_previous_response_id(&request.session_id)
            .await;
        
        // Build request body with all parameters
        let mut request_body = json!({
            "model": CONFIG.gpt5_model,
            "input": messages,
            "stream": true,
            "instructions": "Respond helpfully using available tools when appropriate.",
        });
        
        // Add previous_response_id if available
        if let Some(prev_id) = previous_response_id {
            request_body["previous_response_id"] = json!(prev_id);
            debug!("Using previous_response_id: {}", prev_id);
        }
        
        // Merge parameters at top level
        if let Some(obj) = parameters.as_object() {
            for (key, value) in obj {
                request_body[key] = value.clone();
            }
        }
        
        // Create stream directly using the client
        let stream = self.app_state.llm_client
            .post_response_stream(request_body)
            .await?;
        
        // 10. Process the stream into ChatEvents
        let event_stream = self.process_stream(
            stream, 
            use_tools, 
            request.session_id,
            request.project_id
        );
        
        // Box::pin the stream to ensure it implements Unpin
        Ok(Box::pin(event_stream))
    }
    
    /// Build context using memory service
    async fn build_context(&self, session_id: &str, content: &str) -> Result<RecallContext> {
        // Use memory service directly for parallel recall
        self.app_state.memory_service.parallel_recall_context(
            session_id,
            content,
            CONFIG.context_recent_messages,
            CONFIG.context_semantic_matches,
        ).await
    }
    
    /// Select the appropriate persona
    fn select_persona(&self, _metadata: &Option<MessageMetadata>) -> PersonaOverlay {
        // For now, always use Default
        // Could be extended to check metadata for persona hints
        PersonaOverlay::Default
    }
    
    /// Process the LLM stream into ChatEvents
    fn process_stream(
        &self,
        stream: impl Stream<Item = Result<Value>> + Send + 'static,
        has_tools: bool,
        session_id: String,
        project_id: Option<String>,
    ) -> impl Stream<Item = Result<ChatEvent>> + Send {
        let app_state = self.app_state.clone();
        let tool_executor = self.tool_executor.clone();
        
        // Track state across chunks
        let buffer = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let tool_calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let completion_sent = std::sync::Arc::new(std::sync::Mutex::new(false));
        
        stream.then(move |result| {
            let buffer = buffer.clone();
            let tool_calls = tool_calls.clone();
            let completion_sent = completion_sent.clone();
            let app_state = app_state.clone();
            let tool_executor = tool_executor.clone();
            let session_id = session_id.clone();
            let project_id = project_id.clone();
            
            async move {
                match result {
                    Ok(chunk) => {
                        // Check for event type
                        if let Some(event_type) = chunk.get("type").and_then(|t| t.as_str()) {
                            match event_type {
                                "text_delta" => {
                                    // Handle text delta
                                    if let Some(delta) = chunk.get("delta").and_then(|d| d.as_str()) {
                                        buffer.lock().unwrap().push_str(delta);
                                        Ok(ChatEvent::Content { text: delta.to_string() })
                                    } else {
                                        Ok(ChatEvent::Content { text: String::new() })
                                    }
                                }
                                "tool_call" if has_tools => {
                                    // Collect tool call for later execution
                                    tool_calls.lock().unwrap().push(chunk.clone());
                                    
                                    let tool_name = chunk.get("name")
                                        .and_then(|n| n.as_str())
                                        .unwrap_or("unknown");
                                    
                                    Ok(ChatEvent::ToolExecution {
                                        tool_name: tool_name.to_string(),
                                        status: "started".to_string(),
                                    })
                                }
                                "response.done" | "response_done" => {
                                    // Stream complete - execute tools and save to memory
                                    
                                    // Execute any pending tool calls
                                    let calls = tool_calls.lock().unwrap().clone();
                                    if has_tools && !calls.is_empty() {
                                        info!("Executing {} tool calls", calls.len());
                                        for tool_call in &calls {
                                            match tool_executor.handle_tool_call(&tool_call, &app_state).await {
                                                Ok(_result) => {
                                                    let tool_name = tool_call.get("name")
                                                        .and_then(|n| n.as_str())
                                                        .unwrap_or("unknown");
                                                    debug!("Tool {} completed", tool_name);
                                                }
                                                Err(e) => {
                                                    warn!("Tool execution failed: {}", e);
                                                }
                                            }
                                        }
                                    }
                                    
                                    // Save assistant response to memory
                                    let content = buffer.lock().unwrap().clone();
                                    if !content.is_empty() {
                                        if let Err(e) = Self::save_assistant_to_memory(
                                            &app_state,
                                            &session_id,
                                            &content,
                                            project_id.as_deref(),
                                        ).await {
                                            warn!("Failed to save assistant response to memory: {}", e);
                                        }
                                    }
                                    
                                    *completion_sent.lock().unwrap() = true;
                                    Ok(ChatEvent::Done)
                                }
                                _ => {
                                    debug!("Unhandled event type: {}", event_type);
                                    Ok(ChatEvent::Content { text: String::new() })
                                }
                            }
                        } else {
                            // Try to extract text from various formats
                            let text = Self::extract_text_from_chunk(&chunk);
                            if let Some(content) = text {
                                buffer.lock().unwrap().push_str(&content);
                                Ok(ChatEvent::Content { text: content })
                            } else if Self::is_completion_chunk(&chunk) && !*completion_sent.lock().unwrap() {
                                // Completion detected
                                *completion_sent.lock().unwrap() = true;
                                
                                // Save to memory if we have content
                                let content = buffer.lock().unwrap().clone();
                                if !content.is_empty() {
                                    if let Err(e) = Self::save_assistant_to_memory(
                                        &app_state,
                                        &session_id,
                                        &content,
                                        project_id.as_deref(),
                                    ).await {
                                        warn!("Failed to save assistant response: {}", e);
                                    }
                                }
                                
                                Ok(ChatEvent::Done)
                            } else {
                                Ok(ChatEvent::Content { text: String::new() })
                            }
                        }
                    }
                    Err(e) => {
                        error!("Stream error: {}", e);
                        Ok(ChatEvent::Error { 
                            message: format!("Stream error: {}", e) 
                        })
                    }
                }
            }
        })
    }
    
    /// Extract text content from various chunk formats
    fn extract_text_from_chunk(chunk: &Value) -> Option<String> {
        // Try multiple extraction paths as seen in existing handlers
        
        // 1. Direct content field
        if let Some(content) = chunk.get("content").and_then(|c| c.as_str()) {
            if !content.is_empty() {
                return Some(content.to_string());
            }
        }
        
        // 2. Delta content in choices array
        if let Some(content) = chunk.pointer("/choices/0/delta/content").and_then(|c| c.as_str()) {
            if !content.is_empty() {
                return Some(content.to_string());
            }
        }
        
        // 3. Text field
        if let Some(content) = chunk.get("text").and_then(|c| c.as_str()) {
            if !content.is_empty() {
                return Some(content.to_string());
            }
        }
        
        // 4. Delta field directly
        if let Some(delta) = chunk.get("delta").and_then(|d| d.as_str()) {
            if !delta.is_empty() {
                return Some(delta.to_string());
            }
        }
        
        None
    }
    
    /// Check if chunk indicates completion
    fn is_completion_chunk(chunk: &Value) -> bool {
        // Check for finish_reason
        if let Some(finish_reason) = chunk.pointer("/choices/0/finish_reason") {
            if !finish_reason.is_null() {
                return true;
            }
        }
        
        // Check for done field
        if chunk.get("done").is_some() {
            return true;
        }
        
        // Check for response.done event type
        if let Some(event_type) = chunk.get("type").and_then(|t| t.as_str()) {
            if event_type == "response.done" || event_type == "response_done" {
                return true;
            }
        }
        
        false
    }
    
    /// Save assistant response to memory
    async fn save_assistant_to_memory(
        app_state: &Arc<AppState>,
        session_id: &str,
        content: &str,
        project_id: Option<&str>,
    ) -> Result<()> {
        // Create a minimal ChatResponse for memory storage
        let response = crate::llm::chat_service::ChatResponse {
            output: content.to_string(),
            persona: "mira".to_string(),
            mood: "helpful".to_string(),
            salience: 5,
            summary: if content.len() > 100 {
                format!("{}...", &content[..100])
            } else {
                content.to_string()
            },
            memory_type: "Response".to_string(),
            tags: vec!["chat".to_string()],
            intent: None,
            monologue: None,
            reasoning_summary: None,
        };
        
        app_state.memory_service.save_assistant_response(session_id, &response).await?;
        
        // Also save project association if present
        if let Some(proj_id) = project_id {
            debug!("Assistant response saved with project context: {}", proj_id);
        }
        
        Ok(())
    }
}

/// Extension trait for ToolExecutor cloning
impl Clone for ToolExecutor {
    fn clone(&self) -> Self {
        Self::new()
    }
}
