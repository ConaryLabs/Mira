// src/api/ws/chat.rs
// REFACTORED VERSION - Phase 4: Simplified Main Handler
// Reduced from ~750 lines to ~200 lines by extracting modules
// 
// EXTRACTED MODULES:
// - connection.rs: WebSocket connection management
// - message_router.rs: Message routing and handling logic  
// - heartbeat.rs: Heartbeat/timeout management
// 
// PRESERVED CRITICAL INTEGRATIONS:
// - chat_tools.rs integration via handle_chat_message_with_tools
// - CONFIG-based routing logic
// - All original message types and parsing
// - Parallel context building and streaming logic

use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::{
    extract::{ConnectInfo, State, WebSocketUpgrade},
    response::IntoResponse,
};
use axum::extract::ws::{Message, WebSocket};
use futures::StreamExt;
use futures_util::SinkExt;
use futures_util::stream::SplitSink;
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::Mutex;
use tokio::time::timeout;
use tracing::{debug, error, info, warn};

// Import our extracted modules
use crate::api::ws::connection::WebSocketConnection;
use crate::api::ws::message_router::MessageRouter;
use crate::api::ws::heartbeat::HeartbeatManager;

// Import existing dependencies
use crate::api::ws::message::{WsClientMessage, WsServerMessage};
use crate::llm::streaming::{start_response_stream, StreamEvent};
use crate::state::AppState;
use crate::memory::recall::RecallContext;
use crate::memory::parallel_recall::build_context_parallel;
use crate::config::CONFIG;

#[derive(Deserialize)]
struct Canary {
    id: String,
    part: u32,
    total: u32,
    complete: bool,
    #[serde(default)]
    done: Option<bool>,
    #[allow(dead_code)]
    msg: Option<String>,
}

/// Main WebSocket handler entry point
pub async fn ws_chat_handler(
    ws: WebSocketUpgrade,
    State(app_state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<std::net::SocketAddr>,
) -> impl IntoResponse {
    info!("🔌 WebSocket upgrade request from {}", addr);
    ws.on_upgrade(move |socket| handle_socket(socket, app_state, addr))
}

/// Simplified socket handler using extracted modules
async fn handle_socket(
    socket: WebSocket,
    app_state: Arc<AppState>,
    addr: std::net::SocketAddr,
) {
    let connection_start = Instant::now();
    let (sender, mut receiver) = socket.split();
    
    info!("🔌 WS client connected from {} (new connection)", addr);

    // Create connection wrapper with existing state management
    let last_activity = Arc::new(Mutex::new(Instant::now()));
    let last_any_send = Arc::new(Mutex::new(Instant::now()));
    let is_processing = Arc::new(Mutex::new(false));
    let sender = Arc::new(Mutex::new(sender));

    let connection = Arc::new(WebSocketConnection::new_with_parts(
        sender.clone(),
        last_activity.clone(),
        is_processing.clone(),
        last_any_send.clone(),
    ));

    // Send initial connection messages
    if let Err(e) = connection.send_connection_ready().await {
        error!("❌ Failed to send connection ready: {}", e);
        return;
    }

    // Create message router
    let router = MessageRouter::new(
        app_state.clone(),
        connection.clone(),
        addr,
    );

    // Start heartbeat manager
    let heartbeat = HeartbeatManager::new(connection.clone());
    let heartbeat_task = heartbeat.start();

    // Main message loop - simplified!
    let receive_timeout = Duration::from_secs(CONFIG.ws_receive_timeout);

    loop {
        let recv_future = timeout(receive_timeout, receiver.next());
        
        match recv_future.await {
            Ok(Some(Ok(msg))) => {
                // Update activity timestamp
                connection.update_activity().await;
                
                match msg {
                    Message::Text(text) => {
                        debug!("📥 Received text message: {} bytes", text.len());

                        // Parse and route messages
                        if let Ok(parsed) = serde_json::from_str::<WsClientMessage>(&text) {
                            if let Err(e) = router.route_message(parsed).await {
                                error!("❌ Error routing message: {}", e);
                            }
                        } else if let Ok(canary) = serde_json::from_str::<Canary>(&text) {
                            debug!("🐤 Canary message: id={}, part={}/{}", 
                                   canary.id, canary.part, canary.total);
                            
                            if canary.complete || canary.done.unwrap_or(false) {
                                info!("🐤 Canary complete");
                            }
                        } else {
                            warn!("❓ Unable to parse message: {}", text);
                        }
                    }
                    Message::Binary(_) => {
                        debug!("📥 Binary message received (ignored)");
                    }
                    Message::Ping(data) => {
                        if let Err(e) = connection.send_pong(data).await {
                            error!("❌ Failed to send pong: {}", e);
                        }
                    }
                    Message::Pong(_) => {
                        debug!("🏓 Pong received");
                    }
                    Message::Close(_) => {
                        info!("🔌 Close frame received");
                        break;
                    }
                }
            }
            Ok(Some(Err(e))) => {
                error!("❌ WebSocket error: {}", e);
                break;
            }
            Ok(None) => {
                info!("🔌 WebSocket stream ended");
                break;
            }
            Err(_) => {
                // Timeout - check if we should break
                if !connection.is_processing().await {
                    warn!("⏱️ WebSocket receive timeout after {:?}", receive_timeout);
                    break;
                }
                // Continue if processing
            }
        }
    }

    // Cleanup
    heartbeat_task.abort();
    
    let connection_duration = connection_start.elapsed();
    info!("🔌 WS handler done for {} (connected for {:?})", addr, connection_duration);

    // Clean shutdown
    if let Ok(mut lock) = sender.try_lock() {
        let _ = lock.send(Message::Close(None)).await;
        let _ = lock.close().await;
    }
}

/// Handle a simple chat message with real-time streaming
/// This function is extracted from the original handle_chat_message and used by message_router.rs
/// IMPORTANT: This maintains the original streaming logic for non-tool chat
pub async fn handle_simple_chat_message(
    content: String,
    project_id: Option<String>,
    app_state: Arc<AppState>,
    sender: Arc<Mutex<SplitSink<WebSocket, Message>>>,
    _addr: std::net::SocketAddr,
    last_any_send: Arc<Mutex<Instant>>,
) -> anyhow::Result<()> {
    let msg_start = Instant::now();

    // Session + persona (from CONFIG)
    let session_id = CONFIG.session_id.clone();
    
    info!("💾 Saving user message to memory...");
    if let Err(e) = app_state
        .memory_service
        .save_user_message(&session_id, &content, project_id.as_deref())
        .await
    {
        warn!("⚠️ Failed to save user message: {}", e);
    }

    // Build recall context (using CONFIG values)
    let history_cap = CONFIG.ws_history_cap;
    let vector_k = CONFIG.ws_vector_search_k;

    info!("🔍 Building context (PARALLEL): history_cap={}, vector_k={}", history_cap, vector_k);
    
    // OPTIMIZATION: Use parallel context building
    let context = build_context_parallel(
        &session_id,
        &content,
        history_cap,
        vector_k,
        &app_state.llm_client,
        app_state.sqlite_store.as_ref(),
        app_state.qdrant_store.as_ref(),
    )
    .await
    .unwrap_or_else(|e| {
        warn!("⚠️ Failed to build context: {}. Falling back to empty context.", e);
        RecallContext { recent: vec![], semantic: vec![] }
    });

    // Build system prompt with context
    let base_prompt = "You are Mira, a helpful AI assistant.";
    let system_prompt = build_system_prompt(base_prompt, &context);

    // --- Phase A: Stream tokens in real-time ---
    info!("🚀 Starting real-time response stream...");
    let mut stream = start_response_stream(
        &app_state.llm_client,
        &content,
        Some(&system_prompt),
        false, // structured_json = false for normal streaming
    ).await?;

    let mut full_text = String::new();
    let mut chunks_sent = 0;

    while let Some(event) = stream.next().await {
        match event {
            Ok(StreamEvent::Delta(chunk)) => {
                full_text.push_str(&chunk);
                chunks_sent += 1;

                let chunk_msg = WsServerMessage::Chunk {
                    content: chunk,
                    mood: None,
                };

                if let Ok(text) = serde_json::to_string(&chunk_msg) {
                    let mut lock = sender.lock().await;
                    if let Err(e) = lock.send(Message::Text(text)).await {
                        warn!("❌ Failed to send chunk: {}", e);
                        break;
                    }
                    *last_any_send.lock().await = Instant::now();
                }
            }
            Ok(StreamEvent::Done { .. }) => {
                info!("✅ Streaming complete: {} chunks, {} chars", chunks_sent, full_text.len());
                break;
            }
            Ok(StreamEvent::Error(e)) => {
                error!("❌ Stream error: {}", e);
                break;
            }
            Err(e) => {
                error!("❌ Stream processing error: {}", e);
                break;
            }
        }
    }

    // --- Phase B: Run metadata extraction ---
    info!("📊 Running metadata extraction pass...");
    let (mood, salience, tags) = run_metadata_pass(&app_state, &content, &context).await?;

    // Send completion message
    let complete_msg = WsServerMessage::Complete {
        mood: mood.clone(),
        salience,
        tags: tags.clone(),
    };

    if let Ok(text) = serde_json::to_string(&complete_msg) {
        let mut lock = sender.lock().await;
        let _ = lock.send(Message::Text(text)).await;
        *last_any_send.lock().await = Instant::now();
    }

    // --- Phase C: Save assistant response ---
    info!("💾 Saving assistant response to memory...");
    
    // Create a ChatResponse object for the memory service using the correct structure
    use crate::services::chat::ChatResponse;
    let chat_response = ChatResponse {
        output: full_text.clone(),
        persona: CONFIG.default_persona.clone(),
        mood: mood.clone().unwrap_or_else(|| "neutral".to_string()),
        salience: salience.map(|s| s as usize).unwrap_or(5),
        summary: "".to_string(), // Empty string for non-summary responses
        memory_type: "other".to_string(),
        tags: tags.clone().unwrap_or_default(),
        intent: Some("response".to_string()),
        monologue: None,
        reasoning_summary: None,
    };
    
    if let Err(e) = app_state
        .memory_service
        .save_assistant_response(&session_id, &chat_response)
        .await
    {
        warn!("⚠️ Failed to save assistant response: {}", e);
    }

    let total_time = msg_start.elapsed();
    info!("✅ Simple chat completed in {:?}", total_time);

    Ok(())
}

/// Build system prompt with context
fn build_system_prompt(base_prompt: &str, context: &RecallContext) -> String {
    let mut prompt = base_prompt.to_string();
    
    if !context.recent.is_empty() {
        prompt.push_str("\n\nRecent conversation context is available for reference.");
    }
    
    if !context.semantic.is_empty() {
        prompt.push_str("\n\nRelevant historical context is available.");
    }
    
    prompt
}

/// Run metadata extraction pass
async fn run_metadata_pass(
    app_state: &Arc<AppState>,
    user_text: &str,
    context: &RecallContext,
) -> anyhow::Result<(Option<String>, Option<f32>, Option<Vec<String>>)> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_system_prompt() {
        let base = "You are Mira.";
        
        // Empty context
        let empty_context = RecallContext { recent: vec![], semantic: vec![] };
        assert_eq!(build_system_prompt(base, &empty_context), "You are Mira.");
        
        // With recent context
        let recent_context = RecallContext { 
            recent: vec!["test".to_string()], 
            semantic: vec![] 
        };
        assert!(build_system_prompt(base, &recent_context).contains("Recent conversation"));
        
        // With semantic context
        let semantic_context = RecallContext { 
            recent: vec![], 
            semantic: vec!["test".to_string()] 
        };
        assert!(build_system_prompt(base, &semantic_context).contains("historical context"));
        
        // With both
        let full_context = RecallContext { 
            recent: vec!["test".to_string()], 
            semantic: vec!["test".to_string()] 
        };
        let result = build_system_prompt(base, &full_context);
        assert!(result.contains("Recent conversation"));
        assert!(result.contains("historical context"));
    }

    #[test]
    fn test_canary_parsing() {
        let canary_json = r#"{"id":"test","part":1,"total":3,"complete":false}"#;
        let canary: Canary = serde_json::from_str(canary_json).unwrap();
        
        assert_eq!(canary.id, "test");
        assert_eq!(canary.part, 1);
        assert_eq!(canary.total, 3);
        assert!(!canary.complete);
        assert_eq!(canary.done, None);
    }
}
