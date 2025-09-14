// src/api/ws/chat/unified_handler.rs

use std::sync::Arc;
use anyhow::Result;
use futures::{Stream, StreamExt};
use serde::Serialize;
use serde_json::{json, Value};
use tracing::{debug, info, warn, error};
use tokio::time::{timeout, Duration};

use crate::api::ws::message::MessageMetadata;
use crate::config::CONFIG;
use crate::llm::responses::thread::ThreadManager;
use crate::memory::recall::RecallContext;
use crate::persona::PersonaOverlay;
use crate::prompt::unified_builder::UnifiedPromptBuilder;
use crate::state::AppState;
use crate::tools::executor::ToolExecutor;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum ChatEvent {
    Content { text: String },
    ToolExecution { 
        tool_name: String, 
        status: String 
    },
    ToolResult {
        tool_name: String,
        result: Value,
    },
    Complete {
        mood: Option<String>,
        salience: Option<f32>,
        tags: Option<Vec<String>>,
    },
    Done,
    Error { message: String },
}

#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub content: String,
    pub project_id: Option<String>,
    pub metadata: Option<MessageMetadata>,
    pub session_id: String,
    pub require_json: bool,
}

pub struct UnifiedChatHandler {
    app_state: Arc<AppState>,
    tool_executor: ToolExecutor,
    thread_manager: Arc<ThreadManager>,
}

impl UnifiedChatHandler {
    pub fn new(app_state: Arc<AppState>) -> Self {
        let thread_manager = Arc::new(ThreadManager::new(
            CONFIG.history_message_cap,
            CONFIG.history_token_limit,
        ));
        
        Self {
            app_state,
            tool_executor: ToolExecutor::new(),
            thread_manager,
        }
    }
    
    pub async fn handle_message(
        &self,
        request: ChatRequest,
    ) -> Result<impl Stream<Item = Result<ChatEvent>> + Send> {
        let use_tools = self.tool_executor.should_use_tools(&request.metadata);
        
        if use_tools {
            info!("Processing tool-enabled chat for session: {}", request.session_id);
        } else {
            info!("Processing simple chat message: {}", 
                request.content.chars().take(80).collect::<String>());
        }
        
        let context = self.build_context(&request.session_id, &request.content).await?;
        debug!("Context built: {} recent, {} semantic", 
            context.recent.len(), 
            context.semantic.len()
        );
        
        let tools = if use_tools {
            Some(crate::tools::definitions::get_enabled_tools())
        } else {
            None
        };
        debug!("Tools enabled: {} (found {} tools)", 
            use_tools, 
            tools.as_ref().map_or(0, |t| t.len())
        );
        
        let persona = self.select_persona(&request.metadata);
        debug!("Selected persona: {}", persona);
        
        let system_prompt = UnifiedPromptBuilder::build_system_prompt(
            &persona,
            &context,
            tools.as_deref(),
            request.metadata.as_ref(),
            request.project_id.as_deref(),
            request.require_json,
        );
        debug!("System prompt built: {} chars", system_prompt.len());
        
        let input = vec![
            json!({
                "role": "system",
                "content": [{
                    "type": "input_text",
                    "text": system_prompt
                }]
            }),
            json!({
                "role": "user",
                "content": [{
                    "type": "input_text",
                    "text": request.content.clone()
                }]
            })
        ];
        
        let mut request_body = json!({
            "model": CONFIG.gpt5_model,
            "input": input,
            "stream": true,
            "instructions": "Respond helpfully using available tools when appropriate.",
            "max_output_tokens": CONFIG.max_output_tokens,
            "text": {
                "verbosity": CONFIG.verbosity
            },
            "reasoning": {
                "effort": CONFIG.reasoning_effort
            }
        });
        
        // Format tools according to GPT-5 Responses API spec (09/14/25)
        // FIXED: Only add valid, non-empty tools
        if use_tools {
            let mut valid_tools = Vec::new();
            
            // Only add web_search if it's actually enabled
            // GPT-5 wants just {"type": "web_search"} - no nested object
            if CONFIG.enable_web_search {
                valid_tools.push(json!({
                    "type": "web_search"
                }));
                info!("Added web_search tool to request");
            }
            
            // Add other built-in tools that don't require special setup
            // Note: code_interpreter requires container management, so we skip it
            
            // Only set tools if we have at least one valid tool
            if !valid_tools.is_empty() {
                request_body["tools"] = json!(valid_tools);
                request_body["tool_choice"] = json!("auto");
                info!("Sending {} tools with request", valid_tools.len());
            } else {
                // Don't send tools field at all if empty
                info!("No valid tools to send");
            }
        }
        
        if let Err(e) = self.app_state.memory_service.save_user_message(
            &request.session_id,
            &request.content,
            request.project_id.as_deref()
        ).await {
            warn!("Failed to save user message to memory: {}", e);
        }
        
        let previous_response_id = self.thread_manager
            .get_previous_response_id(&request.session_id)
            .await;
        
        if let Some(prev_id) = previous_response_id {
            request_body["previous_response_id"] = json!(prev_id);
            debug!("Using previous_response_id: {}", prev_id);
        }
        
        info!("Creating response stream for session: {}", request.session_id);
        let stream = self.app_state.llm_client
            .post_response_stream(request_body)
            .await?;
        info!("Response stream created successfully");
        
        info!("Processing stream events...");
        let event_stream = self.process_stream(
            stream, 
            use_tools, 
            request.session_id,
            request.project_id
        );
        
        Ok(Box::pin(event_stream))
    }
    
    async fn build_context(&self, session_id: &str, content: &str) -> Result<RecallContext> {
        self.app_state.memory_service.parallel_recall_context(
            session_id,
            content,
            CONFIG.context_recent_messages,
            CONFIG.context_semantic_matches,
        ).await
    }
    
    fn select_persona(&self, _metadata: &Option<MessageMetadata>) -> PersonaOverlay {
        PersonaOverlay::Default
    }
    
    fn process_stream(
        &self,
        mut stream: impl Stream<Item = Result<Value>> + Send + Unpin + 'static,
        has_tools: bool,
        session_id: String,
        project_id: Option<String>,
    ) -> impl Stream<Item = Result<ChatEvent>> + Send {
        let app_state = self.app_state.clone();
        let _tool_executor = self.tool_executor.clone();
        
        let buffer = Arc::new(std::sync::Mutex::new(String::new()));
        let tool_calls = Arc::new(std::sync::Mutex::new(Vec::new()));
        let completion_sent = Arc::new(std::sync::Mutex::new(false));
        let chunk_count = Arc::new(std::sync::Mutex::new(0));
        
        // Create a channel for sending events
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        
        // Spawn a task to process the stream
        tokio::spawn(async move {
            loop {
                match timeout(Duration::from_secs(120), stream.next()).await {
                    Ok(Some(chunk_result)) => {
                        match chunk_result {
                            Ok(chunk) => {
                                // Get count and immediately drop the lock
                                let count = {
                                    let mut guard = chunk_count.lock().unwrap();
                                    *guard += 1;
                                    *guard
                                };
                                
                                // Log first 5 chunks in detail for debugging
                                if count <= 5 {
                                    info!("RAW CHUNK #{}: {}", count, serde_json::to_string(&chunk).unwrap_or_default());
                                }
                                
                                if let Some(event_type) = chunk.get("type").and_then(|t| t.as_str()) {
                                    debug!("Processing event #{} type: {}", count, event_type);
                                    
                                    match event_type {
                                        // GPT-5 text streaming event - this is what we care about!
                                        "response.output_text.delta" => {
                                            if let Some(delta) = chunk.get("delta").and_then(|d| d.as_str()) {
                                                info!("Got text delta: {} chars", delta.len());
                                                // Update buffer
                                                {
                                                    let mut buf = buffer.lock().unwrap();
                                                    buf.push_str(delta);
                                                }
                                                let _ = tx.send(Ok(ChatEvent::Content { text: delta.to_string() }));
                                            }
                                        }
                                        
                                        // GPT-5 text completion - marks end of text streaming
                                        "response.output_text.done" => {
                                            // Stream is complete - get buffer content
                                            let final_text = {
                                                let buf = buffer.lock().unwrap();
                                                buf.clone()
                                            };
                                            
                                            info!("Text streaming complete - Final buffer: {} chars", final_text.len());
                                            
                                            if !final_text.is_empty() {
                                                if let Err(e) = Self::save_assistant_to_memory(
                                                    &app_state,
                                                    &session_id,
                                                    &final_text,
                                                    project_id.as_deref(),
                                                ).await {
                                                    warn!("Failed to save assistant response: {}", e);
                                                }
                                            } else {
                                                warn!("Stream completed but buffer is empty!");
                                            }
                                            
                                            // Set completion flag
                                            {
                                                let mut sent = completion_sent.lock().unwrap();
                                                *sent = true;
                                            }
                                            
                                            let _ = tx.send(Ok(ChatEvent::Done));
                                            break; // Exit the loop after completion
                                        }
                                        
                                        // These are informational events we can safely ignore
                                        "response.created" | "response.in_progress" | 
                                        "response.output_item.added" | "response.output_item.done" => {
                                            debug!("Ignoring informational event: {}", event_type);
                                        }
                                        
                                        // Tool events (if they come through)
                                        "tool_call" if has_tools => {
                                            {
                                                let mut calls = tool_calls.lock().unwrap();
                                                calls.push(chunk.clone());
                                            }
                                            
                                            let tool_name = chunk.get("name")
                                                .and_then(|n| n.as_str())
                                                .unwrap_or("unknown");
                                            
                                            let _ = tx.send(Ok(ChatEvent::ToolExecution {
                                                tool_name: tool_name.to_string(),
                                                status: "started".to_string(),
                                            }));
                                        }
                                        
                                        // Error events
                                        "error" => {
                                            let error_msg = chunk.get("error")
                                                .and_then(|e| e.get("message"))
                                                .and_then(|m| m.as_str())
                                                .unwrap_or("Unknown error");
                                            error!("Stream error: {}", error_msg);
                                            let _ = tx.send(Ok(ChatEvent::Error { message: error_msg.to_string() }));
                                            break;
                                        }
                                        
                                        // Rate limit or other metadata
                                        "rate_limit" | "ping" => {
                                            debug!("Metadata event: {}", event_type);
                                        }
                                        
                                        // Legacy format fallback (shouldn't happen with GPT-5)
                                        "text_delta" => {
                                            warn!("Got legacy text_delta event - API mismatch?");
                                            if let Some(delta) = chunk.get("delta").and_then(|d| d.as_str()) {
                                                {
                                                    let mut buf = buffer.lock().unwrap();
                                                    buf.push_str(delta);
                                                }
                                                let _ = tx.send(Ok(ChatEvent::Content { text: delta.to_string() }));
                                            }
                                        }
                                        
                                        _ => {
                                            // Only warn about truly unexpected events
                                            if !event_type.starts_with("response.") {
                                                warn!("Unhandled event type: {}", event_type);
                                            }
                                        }
                                    }
                                } else {
                                    // No type field - shouldn't happen with GPT-5
                                    debug!("Chunk #{} without 'type' field", count);
                                }
                            }
                            Err(e) => {
                                error!("Stream error: {}", e);
                                let _ = tx.send(Ok(ChatEvent::Error { 
                                    message: format!("Stream error: {}", e) 
                                }));
                                break;
                            }
                        }
                    }
                    Ok(None) => {
                        // Stream ended naturally
                        info!("Stream ended naturally");
                        break;
                    }
                    Err(_) => {
                        // Timeout
                        warn!("Stream timeout after 120 seconds - forcing completion");
                        let _ = tx.send(Ok(ChatEvent::Error { 
                            message: "Stream timeout - response may be incomplete".to_string() 
                        }));
                        let _ = tx.send(Ok(ChatEvent::Done));
                        break;
                    }
                }
            }
            
            // Check if we sent completion
            let completed = {
                let sent = completion_sent.lock().unwrap();
                *sent
            };
            
            if !completed {
                info!("Sending final Done event");
                let _ = tx.send(Ok(ChatEvent::Done));
            }
        });
        
        // Convert the receiver into a Stream
        tokio_stream::wrappers::UnboundedReceiverStream::new(rx)
    }
    
    fn extract_text_from_chunk(chunk: &Value) -> Option<String> {
        // GPT-5 Responses API: delta field directly contains text
        if let Some(delta) = chunk.get("delta").and_then(|d| d.as_str()) {
            if !delta.is_empty() {
                return Some(delta.to_string());
            }
        }
        
        // Direct text field
        if let Some(text) = chunk.get("text").and_then(|t| t.as_str()) {
            if !text.is_empty() {
                return Some(text.to_string());
            }
        }
        
        // Direct content field
        if let Some(content) = chunk.get("content").and_then(|c| c.as_str()) {
            if !content.is_empty() {
                return Some(content.to_string());
            }
        }
        
        None
    }
    
    fn is_completion_chunk(chunk: &Value) -> bool {
        // Check for GPT-5 response completion events
        if let Some(event_type) = chunk.get("type").and_then(|t| t.as_str()) {
            // GPT-5 uses "response.output_text.done" for text completion
            if event_type == "response.output_text.done" || 
               event_type == "response.done" ||
               event_type == "message_stop" {
                return true;
            }
        }
        
        // Check for done field
        if chunk.get("done").is_some() {
            return true;
        }
        
        false
    }
    
    async fn save_assistant_to_memory(
        app_state: &Arc<AppState>,
        session_id: &str,
        content: &str,
        project_id: Option<&str>,
    ) -> Result<()> {
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
        
        if let Some(proj_id) = project_id {
            debug!("Assistant response saved with project context: {}", proj_id);
        }
        
        Ok(())
    }
}

impl Clone for ToolExecutor {
    fn clone(&self) -> Self {
        Self::new()
    }
}
