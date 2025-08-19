// src/api/ws/chat.rs
// FIXED VERSION - Actually sends chunk messages during streaming
// Key fix: Sends each delta as a WebSocket chunk message to the frontend

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
use crate::llm::streaming::{start_response_stream, StreamEvent};
use crate::persona::PersonaOverlay;
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
    // kept for receive timeout only
    let connection_timeout_secs = std::env::var("MIRA_WS_CONNECTION_TIMEOUT")
        .ok().and_then(|s| s.parse().ok()).unwrap_or(180);

    let last_activity = Arc::new(Mutex::new(Instant::now())); // any inbound
    let last_any_send = Arc::new(Mutex::new(Instant::now())); // any outbound
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
        let sender_for_tick   = sender.clone();
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

                        // 1) Primary protocol
                        if let Ok(parsed) = serde_json::from_str::<WsClientMessage>(&text) {
                            match parsed {
                                // NOTE: WsClientMessage::Message carries { content, project_id, persona }
                                WsClientMessage::Chat { content, project_id, .. }
                                | WsClientMessage::Message { content, project_id, .. } => {
                                    info!("💬 User message: \"{}\"", content);

                                    *is_processing.lock().await = true;
                                    if let Err(e) = handle_chat_message(
                                        content,
                                        project_id,
                                        app_state.clone(),
                                        sender.clone(),
                                        addr,
                                        last_any_send.clone(),
                                    )
                                    .await
                                    {
                                        error!("❌ handle_chat_message error: {}", e);
                                    }
                                    *is_processing.lock().await = false;
                                }
                                WsClientMessage::Status { .. } | WsClientMessage::Command { .. } => {
                                    debug!("⚙️ Command received (ignored for now)");
                                }
                                // don't assume fields; just ack
                                WsClientMessage::Typing { .. } => {
                                    debug!("⌨️ Typing signal received");
                                    let mut lock = sender.lock().await;
                                    let _ = lock
                                        .send(Message::Text(json!({"type":"typing_ack"}).to_string()))
                                        .await;
                                    *last_any_send.lock().await = Instant::now();
                                }
                            }
                            continue;
                        }

                        // 2) Canary payloads
                        if let Ok(c) = serde_json::from_str::<Canary>(&text) {
                            let ack = json!({
                                "type": "canary_ack",
                                "id": c.id,
                                "part": c.part,
                                "seen": format!("seen-{}", c.part),
                            }).to_string();

                            let mut lock = sender.lock().await;
                            if let Err(e) = lock.send(Message::Text(ack)).await {
                                warn!("⚠️ Failed to send canary ack: {}", e);
                            } else {
                                *last_any_send.lock().await = Instant::now();
                            }

                            if c.complete || c.done.unwrap_or(false) {
                                let done = serde_json::to_string(&WsServerMessage::Done).unwrap();
                                let _ = lock.send(Message::Text(done)).await;
                                *last_any_send.lock().await = Instant::now();
                            }

                            continue;
                        }

                        // 3) Unknown payload
                        warn!("⚠️ Unrecognized WS text payload (ignored)");
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
                        *last_activity.lock().await = Instant::now();
                    }
                    Message::Pong(_) => {
                        debug!("🏓 Pong received");
                        *last_activity.lock().await = Instant::now();
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

async fn handle_chat_message(
    content: String,
    project_id: Option<String>,
    app_state: Arc<AppState>,
    sender: Arc<Mutex<SplitSink<WebSocket, Message>>>,
    addr: std::net::SocketAddr,
    last_any_send: Arc<Mutex<Instant>>,
) -> anyhow::Result<()> {
    let msg_start = Instant::now();

    // Session + persona (env defaults only; persona details handled backend-side)
    let session_id = std::env::var("MIRA_SESSION_ID").unwrap_or_else(|_| "peter-eternal".to_string());
    let persona_name = std::env::var("MIRA_DEFAULT_PERSONA").unwrap_or_else(|_| "default".to_string());
    let _persona_overlay = persona_name.parse::<PersonaOverlay>().unwrap_or(PersonaOverlay::Default);

    info!("💾 Saving user message to memory...");

    if let Err(e) = app_state
        .memory_service
        .save_user_message(&session_id, &content, project_id.as_deref())
        .await
    {
        warn!("⚠️ Failed to save user message: {}", e);
    }

    // Build recall context
    let history_cap = std::env::var("MIRA_WS_HISTORY_CAP").ok().and_then(|s| s.parse().ok()).unwrap_or(100);
    let vector_k    = std::env::var("MIRA_WS_VECTOR_SEARCH_K").ok().and_then(|s| s.parse().ok()).unwrap_or(15);

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

    // Compose a small system prompt (no persona helpers required)
    let system_prompt = {
        let mut s = String::new();
        s.push_str("You are Mira. Be concise, helpful, and stream your response as plain text.");
        if !context.recent.is_empty() {
            s.push_str("\nRecent conversation context is available; reference prior answers succinctly.");
        }
        Some(s)
    };

    info!("💬 Starting main response stream (plain text mode, structured_json=false)...");

    // --- Phase A: stream plain text for the UI ---
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

    // FIXED: Actually send chunks to the frontend!
    while let Some(event) = stream.next().await {
        match event {
            Ok(StreamEvent::Delta(chunk)) => {
                info!("📝 Received delta chunk: {} chars", chunk.len());
                
                // Accumulate for saving later
                full_text.push_str(&chunk);
                chunks_sent += 1;

                // CRITICAL FIX: Send the chunk message to frontend
                let msg = WsServerMessage::Chunk {
                    content: chunk,
                    mood: None, // Could set mood on first chunk if desired
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
            Ok(StreamEvent::Done { .. }) => {
                info!("✅ Stream complete: {} chunks sent", chunks_sent);
                break;
            }
            Ok(StreamEvent::Error(e)) => {
                error!("❌ Stream error: {}", e);
                let mut lock = sender.lock().await;
                let err = WsServerMessage::Error { message: e, code: Some("STREAM_ERROR".into()) };
                let _ = lock.send(Message::Text(serde_json::to_string(&err)?)).await;
                *last_any_send.lock().await = Instant::now();
                break;
            }
            Err(e) => {
                error!("❌ Stream decode error: {}", e);
                let mut lock = sender.lock().await;
                let err = WsServerMessage::Error { message: "Stream decode error".to_string(), code: Some("STREAM_DECODE".into()) };
                let _ = lock.send(Message::Text(serde_json::to_string(&err)?)).await;
                *last_any_send.lock().await = Instant::now();
                break;
            }
        }
    }

    // --- Phase B: fetch rich metadata in a second (buffered) call ---
    // Keep this entirely backend-only; the UI gets a tiny 'complete' summary.
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

    // Save assistant response with metadata (don't lose detail)
    if !full_text.is_empty() {
        info!("💾 Saving assistant response ({} chars)...", full_text.len());

        let response = crate::services::chat::ChatResponse {
            output: full_text.clone(),
            persona: normalize_persona(&persona_name),
            mood: mood.clone().unwrap_or_default(),
            salience: salience.map(|v| v as usize).unwrap_or(0),
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

    // Tell the UI we're done (include metadata, which it can ignore)
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

    // Done marker
    let done_msg = WsServerMessage::Done;
    if let Ok(text) = serde_json::to_string(&done_msg) {
        let mut lock = sender.lock().await;
        let _ = lock.send(Message::Text(text)).await;
        *last_any_send.lock().await = Instant::now();
    }

    let total_time = msg_start.elapsed();
    info!("✅ Message handled for {} in {:?}", addr, total_time);

    Ok(())
}

/// Run a tiny second pass asking only for metadata as strict JSON.
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
        /* structured_json = */ true,
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
