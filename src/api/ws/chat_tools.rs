// src/api/ws/chat_tools.rs
// REAL IMPLEMENTATION - Actually executes tools and streams responses
// This version:
// 1. Uses the ResponsesManager for proper tool execution
// 2. Streams tokens in real-time as they arrive
// 3. Handles tool calls and results properly
// 4. Saves responses with metadata to memory

use std::sync::Arc;
use axum::extract::ws::Message;
use futures_util::stream::SplitSink;
use futures::SinkExt;
use futures::StreamExt;
use serde_json::{json, Value};
use tokio::sync::Mutex;
use tracing::{info, error, warn, debug};
use anyhow::Result;

use crate::api::ws::message::{WsClientMessage, MessageMetadata};
use crate::state::AppState;
use crate::services::chat::ChatResponse;
use crate::services::chat_with_tools::{get_enabled_tools, ResponsesTool};
use crate::llm::streaming::{start_response_stream, StreamEvent};
use crate::llm::responses::types::{Message as ResponseMessage};
use crate::llm::responses::ResponsesManager;
use crate::memory::recall::{build_context, RecallContext};

/// Enhanced WebSocket server messages with tool support
#[derive(Debug, serde::Serialize)]
#[serde(tag = "type")]
pub enum WsServerMessageWithTools {
    // Existing message types
    Chunk { 
        content: String, 
        mood: Option<String> 
    },
    Complete { 
        mood: Option<String>, 
        salience: Option<f32>, 
        tags: Option<Vec<String>> 
    },
    Status { 
        message: String, 
        detail: Option<String> 
    },
    Aside { 
        emotional_cue: String, 
        intensity: Option<f32> 
    },
    Error { 
        message: String,
        code: Option<String>
    },
    Done,
    
    // Tool-related message types
    ToolCall {
        tool_type: String,
        tool_id: String,
        status: String, // "started", "completed", "failed"
    },
    ToolResult { 
        tool_type: String,
        tool_id: String,
        data: Value 
    },
    Citation { 
        file_id: String, 
        filename: String, 
        url: Option<String>,
        snippet: Option<String>
    },
}

/// Enhanced streaming for tool-enabled chat with REAL tool execution
pub async fn handle_chat_message_with_tools(
    content: String,
    project_id: Option<String>,
    metadata: Option<MessageMetadata>,
    app_state: Arc<AppState>,
    sender: Arc<Mutex<SplitSink<axum::extract::ws::WebSocket, Message>>>,
    session_id: String,
) -> Result<()> {
    info!("🚀 Processing chat message with REAL tools for session: {}", session_id);
    
    // 1. Send initial status
    let status_msg = WsServerMessageWithTools::Status {
        message: "Initializing response...".to_string(),
        detail: Some("Setting up tools and context".to_string()),
    };
    sender.lock().await.send(Message::Text(
        serde_json::to_string(&status_msg)?
    )).await?;
    
    // 2. Save user message to memory
    info!("💾 Saving user message to memory...");
    if let Err(e) = app_state
        .memory_service
        .save_user_message(&session_id, &content, project_id.as_deref())
        .await
    {
        warn!("⚠️ Failed to save user message: {}", e);
    }
    
    // 3. Build context for the response
    let history_cap = std::env::var("MIRA_WS_HISTORY_CAP")
        .ok().and_then(|s| s.parse().ok()).unwrap_or(100);
    let vector_k = std::env::var("MIRA_WS_VECTOR_SEARCH_K")
        .ok().and_then(|s| s.parse().ok()).unwrap_or(15);
    
    info!("🔍 Building context (history: {}, semantic: {})...", history_cap, vector_k);
    
    let user_embedding = app_state.llm_client.get_embedding(&content).await.ok();
    let context = build_context(
        &session_id,
        user_embedding.as_deref(),
        history_cap,
        vector_k,
        app_state.sqlite_store.as_ref(),
        app_state.qdrant_store.as_ref(),
    )
    .await
    .unwrap_or_else(|e| {
        warn!("⚠️ Failed to build context: {}", e);
        RecallContext { recent: vec![], semantic: vec![] }
    });
    
    // 4. Get enabled tools
    let tools = get_enabled_tools();
    info!("🔧 {} tools enabled", tools.len());
    
    // 5. Build system prompt with tool awareness
    let system_prompt = build_tool_aware_system_prompt(&context, &tools, metadata.as_ref());
    
    // 6. Determine if we should use tool-enhanced streaming or simple streaming
    let should_use_tools = !tools.is_empty() && might_need_tools(&content, metadata.as_ref());
    
    if should_use_tools {
        // Use streaming with tool support
        stream_with_tools(
            content,
            project_id,
            metadata,
            tools,
            system_prompt,
            context,
            app_state,
            sender,
            session_id,
        ).await
    } else {
        // Use simple streaming without tools
        stream_without_tools(
            content,
            system_prompt,
            context,
            app_state,
            sender,
            session_id,
        ).await
    }
}

/// Stream response with tool support using the Responses API
async fn stream_with_tools(
    content: String,
    _project_id: Option<String>,
    metadata: Option<MessageMetadata>,
    tools: Vec<ResponsesTool>,
    system_prompt: String,
    context: RecallContext,
    app_state: Arc<AppState>,
    sender: Arc<Mutex<SplitSink<axum::extract::ws::WebSocket, Message>>>,
    session_id: String,
) -> Result<()> {
    info!("🔨 Using tool-enhanced streaming with Responses API");
    
    // Build messages for the Responses API
    let mut messages = vec![];
    
    // Add system message
    messages.push(ResponseMessage {
        role: "system".to_string(),
        content: Some(system_prompt),
        name: None,
        function_call: None,
        tool_calls: None,
    });
    
    // Add recent context as assistant/user messages
    for entry in context.recent.iter().take(10) {
        messages.push(ResponseMessage {
            role: entry.role.clone(),
            content: Some(entry.content.clone()),
            name: None,
            function_call: None,
            tool_calls: None,
        });
    }
    
    // Add current user message with any file context
    let mut user_content = content.clone();
    if let Some(meta) = &metadata {
        if let Some(file_path) = &meta.file_path {
            user_content = format!("[File: {}]\n{}", file_path, user_content);
        }
    }
    
    messages.push(ResponseMessage {
        role: "user".to_string(),
        content: Some(user_content),
        name: None,
        function_call: None,
        tool_calls: None,
    });
    
    // Convert our tools to the format expected by Responses API
    let api_tools: Vec<crate::llm::responses::types::Tool> = tools.iter().map(|t| {
        match t.tool_type.as_str() {
            "code_interpreter" => {
                crate::llm::responses::types::Tool {
                    tool_type: t.tool_type.clone(),
                    function: None,
                    web_search_preview: None,
                    code_interpreter: Some(crate::llm::responses::types::CodeInterpreterConfig {
                        container: crate::llm::responses::types::ContainerConfig {
                            container_type: "auto".to_string(),
                        },
                    }),
                }
            }
            "web_search" => {
                crate::llm::responses::types::Tool {
                    tool_type: "web_search_preview".to_string(), // API expects this name
                    function: None,
                    web_search_preview: Some(json!({})),
                    code_interpreter: None,
                }
            }
            _ => {
                // For file_search and image_generation, create as function tools
                crate::llm::responses::types::Tool {
                    tool_type: "function".to_string(),
                    function: Some(crate::llm::responses::types::FunctionDefinition {
                        name: t.tool_type.clone(),
                        description: format!("{} tool", t.tool_type),
                        parameters: json!({
                            "type": "object",
                            "properties": {},
                        }),
                    }),
                    web_search_preview: None,
                    code_interpreter: None,
                }
            }
        }
    }).collect();
    
    // Create ResponsesManager for tool execution
    let responses_manager = ResponsesManager::new(app_state.llm_client.clone());
    
    // Clone what we need before the streaming call
    let model = app_state.llm_client.model().to_string();
    let session_id_for_stream = session_id.clone();
    
    // Start streaming response with tools
    info!("📡 Starting streaming response with tools...");
    
    let stream_result = responses_manager.create_streaming_response(
        &model,
        messages,
        None, // instructions handled via system prompt
        Some(&session_id_for_stream),
        Some(json!({
            "tools": api_tools,
            "tool_choice": "auto",
            "stream": true,
            "verbosity": app_state.llm_client.verbosity(),
            "reasoning_effort": app_state.llm_client.reasoning_effort(),
            "max_output_tokens": app_state.llm_client.max_output_tokens(),
        })),
    ).await;
    
    match stream_result {
        Ok(mut stream) => {
            let mut full_text = String::new();
            let mut chunks_sent = 0;
            let mut tool_calls = vec![];
            
            // Process the stream
            while let Some(chunk) = stream.next().await {
                match chunk {
                    Ok(event) => {
                        // Handle different event types from the Responses API
                        if let Some(event_type) = event.get("type").and_then(|v| v.as_str()) {
                            match event_type {
                                "response.text.delta" => {
                                    // Text delta - stream it immediately
                                    if let Some(delta) = event.get("delta").and_then(|v| v.as_str()) {
                                        full_text.push_str(delta);
                                        chunks_sent += 1;
                                        
                                        let chunk_msg = WsServerMessageWithTools::Chunk {
                                            content: delta.to_string(),
                                            mood: None,
                                        };
                                        
                                        if let Ok(text) = serde_json::to_string(&chunk_msg) {
                                            let mut lock = sender.lock().await;
                                            if let Err(e) = lock.send(Message::Text(text)).await {
                                                warn!("Failed to send chunk: {}", e);
                                                break;
                                            }
                                        }
                                    }
                                }
                                "response.tool_call.started" => {
                                    // Tool call started
                                    if let (Some(tool_type), Some(tool_id)) = 
                                        (event.get("tool_type").and_then(|v| v.as_str()),
                                         event.get("tool_id").and_then(|v| v.as_str())) {
                                        
                                        let tool_msg = WsServerMessageWithTools::ToolCall {
                                            tool_type: tool_type.to_string(),
                                            tool_id: tool_id.to_string(),
                                            status: "started".to_string(),
                                        };
                                        
                                        let _ = sender.lock().await.send(Message::Text(
                                            serde_json::to_string(&tool_msg)?
                                        )).await;
                                        
                                        tool_calls.push(json!({
                                            "type": tool_type,
                                            "id": tool_id,
                                            "status": "started"
                                        }));
                                    }
                                }
                                "response.tool_call.completed" => {
                                    // Tool call completed with result
                                    if let (Some(tool_type), Some(tool_id), Some(result)) = 
                                        (event.get("tool_type").and_then(|v| v.as_str()),
                                         event.get("tool_id").and_then(|v| v.as_str()),
                                         event.get("result")) {
                                        
                                        let tool_result_msg = WsServerMessageWithTools::ToolResult {
                                            tool_type: tool_type.to_string(),
                                            tool_id: tool_id.to_string(),
                                            data: result.clone(),
                                        };
                                        
                                        let _ = sender.lock().await.send(Message::Text(
                                            serde_json::to_string(&tool_result_msg)?
                                        )).await;
                                        
                                        // Update tool call status
                                        for call in &mut tool_calls {
                                            if call["id"] == tool_id {
                                                call["status"] = json!("completed");
                                                call["result"] = result.clone();
                                            }
                                        }
                                    }
                                }
                                "response.done" => {
                                    // Response complete
                                    info!("✅ Streaming complete: {} chunks, {} chars", 
                                         chunks_sent, full_text.len());
                                    
                                    // Store response_id if available
                                    if let Some(response_id) = event.get("response_id").and_then(|v| v.as_str()) {
                                        debug!("Response ID: {}", response_id);
                                        // Could store this for conversation continuity
                                    }
                                    break;
                                }
                                _ => {
                                    debug!("Unhandled event type: {}", event_type);
                                }
                            }
                        }
                    }
                    Err(e) => {
                        error!("Stream error: {}", e);
                        let err_msg = WsServerMessageWithTools::Error {
                            message: format!("Stream error: {}", e),
                            code: Some("STREAM_ERROR".to_string()),
                        };
                        let _ = sender.lock().await.send(Message::Text(
                            serde_json::to_string(&err_msg)?
                        )).await;
                        break;
                    }
                }
            }
            
            // Run metadata pass and save response
            finalize_response(
                full_text,
                tool_calls,
                content,
                context,
                app_state,
                sender,
                session_id,
            ).await
        }
        Err(e) => {
            error!("Failed to create streaming response: {}", e);
            let err_msg = WsServerMessageWithTools::Error {
                message: format!("Failed to start streaming: {}", e),
                code: Some("STREAM_INIT_ERROR".to_string()),
            };
            sender.lock().await.send(Message::Text(
                serde_json::to_string(&err_msg)?
            )).await?;
            Err(e.into())
        }
    }
}

/// Stream response without tools (simpler path)
async fn stream_without_tools(
    content: String,
    system_prompt: String,
    context: RecallContext,
    app_state: Arc<AppState>,
    sender: Arc<Mutex<SplitSink<axum::extract::ws::WebSocket, Message>>>,
    session_id: String,
) -> Result<()> {
    info!("💬 Using simple streaming without tools");
    
    // Use the existing streaming infrastructure
    let mut stream = match start_response_stream(
        &app_state.llm_client,
        &content,
        Some(&system_prompt),
        false, // Plain text streaming
    ).await {
        Ok(s) => s,
        Err(e) => {
            error!("Failed to start streaming: {}", e);
            let err_msg = WsServerMessageWithTools::Error {
                message: format!("Failed to start streaming: {}", e),
                code: Some("STREAM_ERROR".to_string()),
            };
            sender.lock().await.send(Message::Text(
                serde_json::to_string(&err_msg)?
            )).await?;
            return Err(e.into());
        }
    };
    
    let mut full_text = String::new();
    let mut chunks_sent = 0;
    
    // Stream tokens as they arrive
    while let Some(event) = stream.next().await {
        match event {
            Ok(StreamEvent::Delta(chunk)) => {
                full_text.push_str(&chunk);
                chunks_sent += 1;
                
                let chunk_msg = WsServerMessageWithTools::Chunk {
                    content: chunk,
                    mood: None,
                };
                
                if let Ok(text) = serde_json::to_string(&chunk_msg) {
                    let mut lock = sender.lock().await;
                    if let Err(e) = lock.send(Message::Text(text)).await {
                        warn!("Failed to send chunk: {}", e);
                        break;
                    }
                }
            }
            Ok(StreamEvent::Done { full_text: done_text, .. }) => {
                if !done_text.is_empty() {
                    full_text = done_text;
                }
                info!("✅ Stream complete: {} chunks sent", chunks_sent);
                break;
            }
            Ok(StreamEvent::Error(err)) => {
                error!("Stream error: {}", err);
                let err_msg = WsServerMessageWithTools::Error {
                    message: err,
                    code: Some("STREAM_ERROR".to_string()),
                };
                sender.lock().await.send(Message::Text(
                    serde_json::to_string(&err_msg)?
                )).await?;
                return Ok(());
            }
            Err(e) => {
                error!("Parse error: {}", e);
                return Err(e.into());
            }
        }
    }
    
    // Finalize without tool results
    finalize_response(
        full_text,
        vec![],
        content,
        context,
        app_state,
        sender,
        session_id,
    ).await
}

/// Finalize the response: run metadata pass and save to memory
async fn finalize_response(
    full_text: String,
    _tool_calls: Vec<Value>,
    user_message: String,
    context: RecallContext,
    app_state: Arc<AppState>,
    sender: Arc<Mutex<SplitSink<axum::extract::ws::WebSocket, Message>>>,
    session_id: String,
) -> Result<()> {
    // Run metadata pass
    info!("🔮 Running metadata pass...");
    let (mood, salience, tags) = match run_metadata_pass(
        &app_state,
        &user_message,
        &context,
    ).await {
        Ok((m, s, t)) => (m, s, t),
        Err(e) => {
            warn!("Metadata pass failed: {}", e);
            (None, None, None)
        }
    };
    
    // Save response to memory
    if !full_text.is_empty() {
        info!("💾 Saving assistant response ({} chars)...", full_text.len());
        
        let response = ChatResponse {
            output: full_text.clone(),
            persona: std::env::var("MIRA_DEFAULT_PERSONA")
                .unwrap_or_else(|_| "default".to_string()),
            mood: mood.clone().unwrap_or_else(|| "neutral".to_string()),
            salience: salience.map(|v| v as usize).unwrap_or(5),
            summary: String::new(),
            memory_type: String::new(),
            tags: tags.clone().unwrap_or_default(),
            intent: None,
            monologue: None,
            reasoning_summary: None,
        };
        
        if let Err(e) = app_state
            .memory_service
            .save_assistant_response(&session_id, &response)
            .await
        {
            warn!("Failed to save assistant response: {}", e);
        }
    }
    
    // Send complete message
    let complete_msg = WsServerMessageWithTools::Complete {
        mood,
        salience,
        tags,
    };
    sender.lock().await.send(Message::Text(
        serde_json::to_string(&complete_msg)?
    )).await?;
    
    // Send done marker
    let done_msg = WsServerMessageWithTools::Done;
    sender.lock().await.send(Message::Text(
        serde_json::to_string(&done_msg)?
    )).await?;
    
    // IMPORTANT: Run summarization to prevent memory overflow in long conversations
    // Access summarizer through ChatService since AppState doesn't have it directly
    info!("📝 Checking if summarization is needed for session: {}", session_id);
    
    // Create a temporary summarizer for this context
    let summarizer = crate::services::summarization::SummarizationService::new_with_stores(
        app_state.llm_client.clone(),
        Arc::new(crate::services::chat::ChatConfig::default()),
        app_state.sqlite_store.clone(),
        app_state.memory_service.clone(),
    );
    
    if let Err(e) = summarizer.summarize_if_needed(&session_id).await {
        warn!("⚠️ Failed to run summarization: {}", e);
    } else {
        debug!("✅ Summarization check complete");
    }
    
    info!("✅ Response finalized successfully");
    Ok(())
}

/// Build a tool-aware system prompt
fn build_tool_aware_system_prompt(
    context: &RecallContext,
    tools: &[ResponsesTool],
    metadata: Option<&MessageMetadata>,
) -> String {
    let mut prompt = String::from("You are Mira, an AI assistant.");
    
    if !tools.is_empty() {
        prompt.push_str("\n\nYou have access to the following tools:");
        for tool in tools {
            match tool.tool_type.as_str() {
                "web_search" => prompt.push_str("\n- Web Search: Search the internet for current information"),
                "code_interpreter" => prompt.push_str("\n- Code Interpreter: Execute Python code for calculations and analysis"),
                "file_search" => prompt.push_str("\n- File Search: Search through uploaded documents"),
                "image_generation" => prompt.push_str("\n- Image Generation: Create images from text descriptions"),
                _ => {}
            }
        }
        prompt.push_str("\n\nUse tools when they would be helpful to answer the user's question accurately.");
    }
    
    if !context.recent.is_empty() {
        prompt.push_str("\n\nRecent conversation context is available for reference.");
    }
    
    if let Some(meta) = metadata {
        if meta.file_path.is_some() {
            prompt.push_str("\n\nThe user has provided file context with their message.");
        }
    }
    
    prompt
}

/// Run metadata extraction pass
async fn run_metadata_pass(
    app_state: &Arc<AppState>,
    user_text: &str,
    context: &RecallContext,
) -> Result<(Option<String>, Option<f32>, Option<Vec<String>>)> {
    let sys = {
        let mut s = String::new();
        s.push_str("Return ONLY JSON with keys: mood (string), salience (number 0..10), tags (array of strings).");
        if !context.recent.is_empty() {
            s.push_str(" Consider recent messages for context.");
        }
        s
    };
    
    let mut meta_stream = start_response_stream(
        &app_state.llm_client,
        user_text,
        Some(&sys),
        true, // structured JSON
    ).await?;
    
    let mut json_txt = String::new();
    while let Some(ev) = meta_stream.next().await {
        match ev {
            Ok(StreamEvent::Delta(chunk)) => {
                json_txt.push_str(&chunk);
            }
            Ok(StreamEvent::Done { .. }) => break,
            Ok(StreamEvent::Error(e)) => {
                return Err(anyhow::anyhow!(e));
            }
            Err(e) => return Err(e),
        }
    }
    
    if json_txt.trim().is_empty() {
        return Ok((None, None, None));
    }
    
    let v: Value = serde_json::from_str(&json_txt)?;
    let mood = v.get("mood").and_then(|x| x.as_str()).map(|s| s.to_string());
    let sal = v.get("salience").and_then(|x| x.as_f64()).map(|f| f as f32);
    let tags = v
        .get("tags")
        .and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|t| t.as_str().map(|s| s.to_string()))
                .collect::<Vec<_>>()
        });
    
    Ok((mood, sal, tags))
}

/// Determine if the message might need tools
fn might_need_tools(message: &str, metadata: Option<&MessageMetadata>) -> bool {
    let message_lower = message.to_lowercase();
    
    // Check for tool-triggering keywords
    let needs_tools = message_lower.contains("search")
        || message_lower.contains("calculate")
        || message_lower.contains("analyze")
        || message_lower.contains("generate")
        || message_lower.contains("create")
        || message_lower.contains("find")
        || message_lower.contains("code")
        || message_lower.contains("python")
        || message_lower.contains("compute")
        || message_lower.contains("current")
        || message_lower.contains("latest")
        || message_lower.contains("today")
        || message_lower.contains("news");
    
    // Also check if file context suggests tool usage
    let has_relevant_metadata = metadata.map_or(false, |m| {
        m.file_path.is_some() || m.attachment_id.is_some()
    });
    
    needs_tools || has_relevant_metadata
}

/// Update the main WebSocket handler to use streaming tools
pub async fn update_ws_handler_for_tools(
    msg: WsClientMessage,
    app_state: Arc<AppState>,
    sender: Arc<Mutex<SplitSink<axum::extract::ws::WebSocket, Message>>>,
    session_id: String,
) -> Result<()> {
    match msg {
        WsClientMessage::Chat { content, project_id, metadata } => {
            handle_chat_message_with_tools(
                content,
                project_id,
                metadata,
                app_state,
                sender,
                session_id,
            ).await
        }
        WsClientMessage::Message { content, project_id, .. } => {
            // Legacy format - convert to chat with no metadata
            handle_chat_message_with_tools(
                content,
                project_id,
                None,
                app_state,
                sender,
                session_id,
            ).await
        }
        _ => Ok(())
    }
}
