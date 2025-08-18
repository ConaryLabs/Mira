// src/api/ws/chat.rs
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::{
    extract::{State, WebSocketUpgrade, ConnectInfo},
    response::IntoResponse,
};
use axum::extract::ws::{Message, WebSocket};
use futures::SinkExt;
use futures::StreamExt;
use futures_util::stream::SplitSink;
use tokio::sync::Mutex;
use tokio::time::{interval, timeout};
use tracing::{debug, error, info, warn};

use crate::api::ws::message::{WsClientMessage, WsServerMessage};
use crate::llm::streaming::StreamEvent;
use crate::persona::PersonaOverlay;
use crate::prompt::builder::build_system_prompt;
use crate::state::AppState;
use crate::memory::recall::{RecallContext, build_context};

pub async fn ws_chat_handler(
    ws: WebSocketUpgrade,
    State(app_state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<std::net::SocketAddr>,
) -> impl IntoResponse {
    info!("🔌 WebSocket upgrade request from {}", addr);
    ws.on_upgrade(move |socket| handle_socket(socket, app_state, addr))
}

async fn handle_socket(
    socket: WebSocket, 
    app_state: Arc<AppState>,
    addr: std::net::SocketAddr,
) {
    let connection_start = Instant::now();
    let (sender, mut receiver) = socket.split();
    let sender = Arc::new(Mutex::new(sender));

    info!("🔌 WS client connected from {} (new connection)", addr);

    // Heartbeat configuration from environment
    let heartbeat_interval_secs = std::env::var("MIRA_WS_HEARTBEAT_INTERVAL")
        .ok().and_then(|s| s.parse().ok()).unwrap_or(30);
    let connection_timeout_secs = std::env::var("MIRA_WS_CONNECTION_TIMEOUT")
        .ok().and_then(|s| s.parse().ok()).unwrap_or(300);

    // Track last activity
    let last_activity = Arc::new(Mutex::new(Instant::now()));
    
    // Heartbeat task with connection monitoring
    {
        let sender_for_ping = sender.clone();
        let last_activity_ping = last_activity.clone();
        
        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(heartbeat_interval_secs));
            let mut consecutive_failures = 0;
            
            loop {
                ticker.tick().await;
                
                // Check if connection is still alive based on last activity
                let last = *last_activity_ping.lock().await;
                if last.elapsed() > Duration::from_secs(connection_timeout_secs) {
                    warn!("⚠️ WebSocket connection timed out (no activity for {}s)", 
                          connection_timeout_secs);
                    break;
                }
                
                // Send ping
                let mut lock = sender_for_ping.lock().await;
                if let Err(e) = lock.send(Message::Ping(vec![0x9])).await {
                    consecutive_failures += 1;
                    warn!("Heartbeat ping failed (attempt {}): {}", consecutive_failures, e);
                    
                    if consecutive_failures >= 3 {
                        error!("❌ Heartbeat failed 3 times, closing connection");
                        break;
                    }
                } else {
                    consecutive_failures = 0;
                    debug!("💓 Heartbeat ping sent successfully");
                }
            }
            debug!("Heartbeat task ended");
        });
    }

    // Message handling with timeout
    let receive_timeout = Duration::from_secs(connection_timeout_secs);
    
    loop {
        // Use timeout to detect stalled connections
        match timeout(receive_timeout, receiver.next()).await {
            Ok(Some(Ok(msg))) => {
                // Update last activity
                *last_activity.lock().await = Instant::now();
                
                match msg {
                    Message::Text(text) => {
                        debug!("📥 Received text message: {} bytes", text.len());
                        
                        match serde_json::from_str::<WsClientMessage>(&text) {
                            Ok(WsClientMessage::Chat { content, project_id, .. })
                            | Ok(WsClientMessage::Message { content, project_id, .. }) => {
                                info!("💬 Processing chat message from {}", addr);
                                
                                let app_state = app_state.clone();
                                let sender = sender.clone();
                                
                                tokio::spawn(async move {
                                    if let Err(e) = handle_chat_message(
                                        content, 
                                        project_id, 
                                        app_state, 
                                        sender,
                                        addr
                                    ).await {
                                        error!("Error handling chat message: {}", e);
                                    }
                                });
                            }
                            Ok(WsClientMessage::Status { message }) => {
                                debug!("📊 Status message: {}", message);
                                // This might be a keep-alive from frontend
                                *last_activity.lock().await = Instant::now();
                            }
                            Ok(WsClientMessage::Command { .. }) => {
                                debug!("⚙️ Command received (ignored for now)");
                            }
                            Ok(other) => {
                                debug!("❓ Ignoring WS message type: {:?}", other);
                            }
                            Err(e) => {
                                warn!("⚠️ Invalid WS message: {}", e);
                            }
                        }
                    }
                    Message::Binary(data) => {
                        debug!("📥 Binary message ({} bytes) - ignored", data.len());
                    }
                    Message::Ping(data) => {
                        debug!("🏓 Ping received, sending pong");
                        let mut lock = sender.lock().await;
                        if let Err(e) = lock.send(Message::Pong(data)).await {
                            warn!("Failed to send pong: {}", e);
                        }
                    }
                    Message::Pong(_) => {
                        debug!("🏓 Pong received");
                    }
                    Message::Close(frame) => {
                        info!("🔌 Close frame received: {:?}", frame);
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
                warn!("⏱️ WebSocket receive timeout ({} seconds)", connection_timeout_secs);
                break;
            }
        }
    }

    let connection_duration = connection_start.elapsed();
    info!("🔌 WS handler done for {} (connected for {:?})", addr, connection_duration);
    
    // Clean shutdown
    if let Ok(mut lock) = sender.try_lock() {
        let _ = lock.close().await;
    }
}

async fn handle_chat_message(
    content: String,
    project_id: Option<String>,
    app_state: Arc<AppState>,
    sender: Arc<Mutex<SplitSink<WebSocket, Message>>>,
    addr: std::net::SocketAddr,
) -> anyhow::Result<()> {
    let msg_start = Instant::now();
    
    // Get session ID from environment
    let session_id = std::env::var("MIRA_SESSION_ID")
        .unwrap_or_else(|_| "peter-eternal".to_string());
    
    // Get default persona from environment
    let persona_str = std::env::var("MIRA_DEFAULT_PERSONA")
        .unwrap_or_else(|_| "default".to_string());
    let persona = persona_str.parse::<PersonaOverlay>()
        .unwrap_or(PersonaOverlay::Default);

    info!("💾 Saving user message to memory...");
    
    // Persist user message
    if let Err(e) = app_state
        .memory_service
        .save_user_message(&session_id, &content, project_id.as_deref())
        .await 
    {
        warn!("⚠️ Failed to save user message: {}", e);
    }

    // Build recall context with env-based limits
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

    info!("📝 Getting metadata from GPT-5...");
    
    // Phase 1: metadata
    let metadata = match crate::api::two_phase::get_metadata(
        &app_state.llm_client,
        &content,
        &persona,
        &context,
    )
    .await
    {
        Ok(m) => {
            info!("✅ Metadata received: mood={}, salience={}", m.mood, m.salience);
            m
        }
        Err(e) => {
            warn!("⚠️ Could not parse metadata, using defaults: {}", e);
            Default::default()
        }
    };

    info!("💬 Getting content from GPT-5...");
    
    // Phase 2: content
    let mut stream = match crate::api::two_phase::get_content_stream(
        &app_state.llm_client,
        &content,
        &persona,
        &context,
        &metadata.mood,
        &metadata.intent,
    )
    .await {
        Ok(s) => s,
        Err(e) => {
            error!("❌ Failed to get content stream: {}", e);
            
            // Send error to client
            let error_msg = WsServerMessage::Error {
                message: "Failed to generate response".to_string(),
                code: Some("STREAM_ERROR".to_string()),
            };
            
            let mut lock = sender.lock().await;
            let _ = lock.send(Message::Text(serde_json::to_string(&error_msg)?)).await;
            return Err(e);
        }
    };

    let mut full_text = String::new();
    let mut chunks_sent = 0;
    
    while let Some(event) = stream.next().await {
        match event {
            Ok(StreamEvent::Delta(chunk)) => {
                full_text.push_str(&chunk);
                chunks_sent += 1;
                
                // Send chunk to client
                let msg = WsServerMessage::Chunk {
                    content: chunk,
                    mood: Some(metadata.mood.clone()),
                };
                
                if let Ok(text) = serde_json::to_string(&msg) {
                    let mut lock = sender.lock().await;
                    if let Err(e) = lock.send(Message::Text(text)).await {
                        warn!("⚠️ Failed to send chunk {}: {}", chunks_sent, e);
                        break;
                    }
                }
            }
            Ok(StreamEvent::Done { .. }) => {
                info!("✅ Stream complete: {} chunks sent", chunks_sent);
                
                // Send completion
                let msg = WsServerMessage::Complete {
                    mood: Some(metadata.mood.clone()),
                    salience: Some(metadata.salience as f32),
                    tags: Some(metadata.tags.clone()),
                };
                
                if let Ok(text) = serde_json::to_string(&msg) {
                    let mut lock = sender.lock().await;
                    let _ = lock.send(Message::Text(text)).await;
                }
            }
            Ok(StreamEvent::Error(e)) => {
                error!("❌ Stream error: {}", e);
                break;
            }
            Err(e) => {
                error!("❌ Stream decode error: {}", e);
                break;
            }
        }
    }

    // Save assistant response
    if !full_text.is_empty() {
        info!("💾 Saving assistant response ({} chars)...", full_text.len());
        
        let response = crate::services::chat::ChatResponse {
            output: full_text,
            persona: persona.to_string(),
            mood: metadata.mood,
            salience: metadata.salience,
            summary: metadata.summary,
            memory_type: metadata.memory_type,
            tags: metadata.tags,
            intent: Some(metadata.intent),
            monologue: metadata.monologue,
            reasoning_summary: metadata.reasoning_summary,
        };

        if let Err(e) = app_state
            .memory_service
            .save_assistant_response(&session_id, &response)
            .await 
        {
            warn!("⚠️ Failed to save assistant response: {}", e);
        }
    }

    // Send done marker
    let done_msg = WsServerMessage::Done;
    if let Ok(text) = serde_json::to_string(&done_msg) {
        let mut lock = sender.lock().await;
        let _ = lock.send(Message::Text(text)).await;
    }

    let total_time = msg_start.elapsed();
    info!("✅ Message handled for {} in {:?}", addr, total_time);

    Ok(())
}
