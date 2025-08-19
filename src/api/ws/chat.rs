// src/api/ws/chat.rs
// VERIFIED VERSION - Ensures real-time streaming is working properly
// Key features:
// 1. Streams tokens immediately as they arrive
// 2. Runs metadata pass separately after streaming completes
// 3. Saves full response with metadata to memory
// 4. Handles both simple chat and integrates with tool-enabled chat

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
use serde_json::{json, Value};
use tokio::sync::Mutex;
use tokio::time::{interval, timeout};
use tracing::{debug, error, info, warn};

use crate::api::ws::message::{WsClientMessage, WsServerMessage};
use crate::api::ws::chat_tools::handle_chat_message_with_tools;
use crate::llm::streaming::{start_response_stream, StreamEvent};
use crate::state::AppState;
use crate::memory::recall::{build_context, RecallContext};

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

    // ---- Heartbeat configuration (soft) ----
    let heartbeat_interval_secs = std::env::var("MIRA_WS_HEARTBEAT_INTERVAL")
        .ok().and_then(|s| s.parse().ok()).unwrap_or(25);
    let connection_timeout_secs = std::env::var("MIRA_WS_CONNECTION_TIMEOUT")
        .ok().and_then(|s| s.parse().ok()).unwrap_or(180);

    let last_activity = Arc::new(Mutex::new(Instant::now()));
    let last_any_send = Arc::new(Mutex::new(Instant::now()));
    let is_processing = Arc::new(Mutex::new(false));

    // Send immediate hello + ready
    {
        let mut lock = sender.lock().await;
        let _ = lock.send(Message::Text(json!({
            "type": "hello",
            "ts": chrono::Utc::now().to_rfc3339(),
            "server": "mira-backend"
        }).to_string())).await;

        let _ = lock.send(Message::Text(json!({
            "type": "ready"
        }).to_string())).await;

        *last_any_send.lock().await = Instant::now();
    }

    // ---- Heartbeat task (app heartbeat + ping; NO auto-close) ----
    {
        let sender_for_tick = sender.clone();
        let last_any_send_ref = last_any_send.clone();
        let is_processing_ref = is_processing.clone();

        let interval_dur = Duration::from_secs(heartbeat_interval_secs);

        tokio::spawn(async move {
            let mut ticker = interval(interval_dur);
            ticker.tick().await; // prime
            tokio::time::sleep(Duration::from_secs(2)).await;

            loop {
                ticker.tick().await;

                // Avoid competing with streaming writes
                if *is_processing_ref.lock().await {
                    debug!("💓 Skip heartbeat while processing");
                    continue;
                }

                // Take a normal lock and send heartbeat + ping
                let mut l = sender_for_tick.lock().await;

                if l.send(Message::Text(json!({
                    "type":"heartbeat",
                    "ts": chrono::Utc::now().to_rfc3339()
                }).to_string())).await.is_ok() {
                    *last_any_send_ref.lock().await = Instant::now();
                    debug!("💓 App heartbeat sent");
                } else {
                    debug!("Heartbeat send failed (socket likely closed)");
                    break;
                }

                if l.send(Message::Ping(Vec::new())).await.is_err() {
                    debug!("Ping failed (socket likely closed)");
                    break;
                }
            }
            debug!("Heartbeat task ended");
        });
    }

    // ---- Message loop with receive timeout ----
    let receive_timeout = Duration::from_secs(connection_timeout_secs);

    loop {
        match timeout(receive_timeout, receiver.next()).await {
            Ok(Some(Ok(msg))) => {
                *last_activity.lock().await = Instant::now();

                match msg {
                    Message::Text(text) => {
                        debug!("📥 Received text message: {} bytes", text.len());

                        // 1) Primary protocol - check if tools are enabled
                        let enable_tools = std::env::var("ENABLE_CHAT_TOOLS")
                            .unwrap_or_else(|_| "false".to_string()) == "true";

                        if let Ok(parsed) = serde_json::from_str::<WsClientMessage>(&text) {
                            match parsed {
                                WsClientMessage::Chat { content, project_id, metadata } => {
                                    info!("💬 Chat message received: {} chars", content.len());
                                    *is_processing.lock().await = true;

                                    // Route to appropriate handler based on tools setting
                                    let result = if enable_tools && metadata.is_some() {
                                        // Use tool-enabled streaming handler
                                        let session_id = std::env::var("MIRA_SESSION_ID")
                                            .unwrap_or_else(|_| "peter-eternal".to_string());
                                        
                                        handle_chat_message_with_tools(
                                            content,
                                            project_id,
                                            metadata,
                                            app_state.clone(),
                                            sender.clone(),
                                            session_id,
                                        ).await
                                    } else {
                                        // Use simple streaming handler
                                        handle_chat_message(
                                            content,
                                            project_id,
                                            app_state.clone(),
                                            sender.clone(),
                                            addr,
                                            last_any_send.clone(),
                                        ).await
                                    };

                                    *is_processing.lock().await = false;

                                    if let Err(e) = result {
                                        error!("❌ Error handling chat message: {}", e);
                                    }
                                }
                                WsClientMessage::Message { content, project_id, .. } => {
                                    // Legacy format
                                    info!("💬 Legacy message received: {} chars", content.len());
                                    *is_processing.lock().await = true;

                                    let result = handle_chat_message(
                                        content,
                                        project_id,
                                        app_state.clone(),
                                        sender.clone(),
                                        addr,
                                        last_any_send.clone(),
                                    ).await;

                                    *is_processing.lock().await = false;

                                    if let Err(e) = result {
                                        error!("❌ Error handling message: {}", e);
                                    }
                                }
                                _ => {
                                    debug!("📥 Other message type received");
                                }
                            }
                        }

                        // 2) Canary protocol (for testing)
                        if let Ok(canary) = serde_json::from_str::<Canary>(&text) {
                            debug!("🐤 Canary {} part {}/{}", canary.id, canary.part, canary.total);
                            
                            let echo = json!({
                                "type": "canary_echo",
                                "id": canary.id,
                                "part": canary.part,
                                "total": canary.total,
                                "complete": canary.complete
                            });
                            
                            let mut lock = sender.lock().await;
                            let _ = lock.send(Message::Text(echo.to_string())).await;
                            *last_any_send.lock().await = Instant::now();

                            if canary.complete || canary.done.unwrap_or(false) {
                                debug!("🐤 Canary complete");
                            }
                        }
                    }
                    Message::Binary(_) => {
                        debug!("📥 Binary message received (ignored)");
                    }
                    Message::Ping(data) => {
                        debug!("🏓 Ping received, sending pong");
                        let mut lock = sender.lock().await;
                        let _ = lock.send(Message::Pong(data)).await;
                        *last_any_send.lock().await = Instant::now();
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
                if !*is_processing.lock().await {
                    warn!("⏱️ WebSocket receive timeout after {:?}", receive_timeout);
                    break;
                }
            }
        }
    }

    let connection_duration = connection_start.elapsed();
    info!("🔌 WS handler done for {} (connected for {:?})", addr, connection_duration);

    // Clean shutdown
    if let Ok(mut lock) = sender.try_lock() {
        let _ = lock.send(Message::Close(None)).await;
        let _ = lock.close().await;
    }
}

/// Handle a simple chat message with real-time streaming
async fn handle_chat_message(
    content: String,
    project_id: Option<String>,
    app_state: Arc<AppState>,
    sender: Arc<Mutex<SplitSink<WebSocket, Message>>>,
    addr: std::net::SocketAddr,
    last_any_send: Arc<Mutex<Instant>>,
) -> anyhow::Result<()> {
    let msg_start = Instant::now();

    // Session + persona
    let session_id = std::env::var("MIRA_SESSION_ID")
        .unwrap_or_else(|_| "peter-eternal".to_string());
    let persona_name = std::env::var("MIRA_DEFAULT_PERSONA")
        .unwrap_or_else(|_| "default".to_string());

    info!("💾 Saving user message to memory...");
    if let Err(e) = app_state
        .memory_service
        .save_user_message(&session_id, &content, project_id.as_deref())
        .await
    {
        warn!("⚠️ Failed to save user message: {}", e);
    }

    // Build recall context
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

    // Build system prompt
    let system_prompt = {
        let mut s = String::new();
        s.push_str("You are Mira. Be concise, helpful, and stream your response as plain text.");
        if !context.recent.is_empty() {
            s.push_str("\nRecent conversation context is available; reference prior answers succinctly.");
        }
        Some(s)
    };

    info!("💬 Starting main response stream (plain text mode, structured_json=false)...");

    // --- Phase A: Stream plain text for the UI ---
    // CRITICAL: Must use structured_json = false for streaming!
    let mut stream = match start_response_stream(
        &app_state.llm_client,
        &content,
        system_prompt.as_deref(),
        false, // MUST be false for plain text streaming!
    ).await {
        Ok(s) => {
            info!("✅ Main response stream created successfully");
            s
        }
        Err(e) => {
            error!("❌ Failed to get main content stream: {}", e);

            let error_msg = WsServerMessage::Error {
                message: "Failed to generate response".to_string(),
                code: Some("STREAM_ERROR".to_string()),
            };

            let mut lock = sender.lock().await;
            let _ = lock.send(Message::Text(serde_json::to_string(&error_msg)?)).await;
            *last_any_send.lock().await = Instant::now();
            return Err(e.into());
        }
    };

    let mut full_text = String::new();
    let mut chunks_sent: usize = 0;

    info!("📨 Starting to process stream events...");

    // VERIFIED: Send chunks to the frontend immediately!
    while let Some(event) = stream.next().await {
        match event {
            Ok(StreamEvent::Delta(chunk)) => {
                debug!("📝 Received delta chunk: {} chars", chunk.len());
                
                // Accumulate for saving later
                full_text.push_str(&chunk);
                chunks_sent += 1;

                // CRITICAL: Send the chunk message to frontend immediately
                let msg = WsServerMessage::Chunk {
                    content: chunk,
                    mood: None,
                };

                if let Ok(text) = serde_json::to_string(&msg) {
                    let mut lock = sender.lock().await;
                    if let Err(e) = lock.send(Message::Text(text)).await {
                        warn!("⚠️ Failed to send chunk {}: {}", chunks_sent, e);
                        break;
                    } else {
                        debug!("➡️ Sent chunk #{} to frontend", chunks_sent);
                        *last_any_send.lock().await = Instant::now();
                    }
                } else {
                    warn!("⚠️ Failed to serialize chunk message");
                }
            }
            Ok(StreamEvent::Done { full_text: done_text, .. }) => {
                // Use the complete text from Done event if available
                if !done_text.is_empty() {
                    full_text = done_text;
                }
                info!("✅ Stream complete: {} chunks sent, {} total chars", 
                     chunks_sent, full_text.len());
                break;
            }
            Ok(StreamEvent::Error(e)) => {
                error!("❌ Stream error: {}", e);
                let mut lock = sender.lock().await;
                let err = WsServerMessage::Error { 
                    message: e, 
                    code: Some("STREAM_ERROR".into()) 
                };
                let _ = lock.send(Message::Text(serde_json::to_string(&err)?)).await;
                *last_any_send.lock().await = Instant::now();
                break;
            }
            Err(e) => {
                error!("❌ Stream decode error: {}", e);
                let mut lock = sender.lock().await;
                let err = WsServerMessage::Error { 
                    message: "Stream decode error".to_string(), 
                    code: Some("STREAM_DECODE".into()) 
                };
                let _ = lock.send(Message::Text(serde_json::to_string(&err)?)).await;
                *last_any_send.lock().await = Instant::now();
                break;
            }
        }
    }

    // --- Phase B: Fetch rich metadata in a second (buffered) call ---
    info!("🔮 Starting metadata pass (structured_json=true)...");
    let (mood, salience, tags) = match metadata_pass(&app_state, &content, &context).await {
        Ok((m, s, t)) => {
            info!("✅ Metadata pass complete: mood={:?}, salience={:?}, tags={:?}", m, s, t);
            (m, s, t)
        }
        Err(e) => {
            warn!("⚠️ Metadata pass failed: {}", e);
            (None, None, None)
        }
    };

    // Save assistant response with metadata
    if !full_text.is_empty() {
        info!("💾 Saving assistant response ({} chars)...", full_text.len());

        let response = crate::services::chat::ChatResponse {
            output: full_text.clone(),
            persona: normalize_persona(&persona_name),
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
            warn!("⚠️ Failed to save assistant response: {}", e);
        }
    }

    // Send complete message with metadata
    {
        let mut lock = sender.lock().await;
        let complete = WsServerMessage::Complete {
            mood,
            salience,
            tags,
        };
        let _ = lock.send(Message::Text(serde_json::to_string(&complete)?)).await;
        *last_any_send.lock().await = Instant::now();
    }

    // Send done marker
    let done_msg = WsServerMessage::Done;
    if let Ok(text) = serde_json::to_string(&done_msg) {
        let mut lock = sender.lock().await;
        let _ = lock.send(Message::Text(text)).await;
        *last_any_send.lock().await = Instant::now();
    }

    // IMPORTANT: Run summarization if needed to prevent memory overflow
    info!("📝 Checking if summarization is needed...");
    
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

    let total_time = msg_start.elapsed();
    info!("✅ Message handled for {} in {:?}", addr, total_time);

    Ok(())
}

/// Run a metadata pass to extract mood, salience, and tags
async fn metadata_pass(
    app_state: &Arc<AppState>,
    user_text: &str,
    context: &RecallContext,
) -> anyhow::Result<(Option<String>, Option<f32>, Option<Vec<String>>)> {
    let sys = {
        let mut s = String::new();
        s.push_str("Return ONLY JSON with keys: mood (string), salience (number 0..10), tags (array of strings).");
        if !context.recent.is_empty() {
            s.push_str(" Consider recent messages for mood and tags.");
        }
        s
    };

    let mut meta_stream = start_response_stream(
        &app_state.llm_client,
        user_text,
        Some(&sys),
        true, // structured_json = true for metadata extraction
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

    // Parse and extract fields gently
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

fn normalize_persona(p: &str) -> String {
    if p.is_empty() { "default".to_string() } else { p.to_string() }
}
