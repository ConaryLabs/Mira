// src/api/ws/chat.rs
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::{
    extract::{ConnectInfo, State, WebSocketUpgrade},
    response::IntoResponse,
};
use axum::extract::ws::{Message, WebSocket};
use futures::SinkExt;
use futures::StreamExt;
use futures_util::stream::SplitSink;
use serde::Deserialize;
use serde_json::json;
use tokio::sync::Mutex;
use tokio::time::{interval, timeout};
use tracing::{debug, error, info, warn};

use crate::api::ws::message::{WsClientMessage, WsServerMessage};
use crate::llm::streaming::StreamEvent;
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
                                WsClientMessage::Chat { content, project_id, .. }
                                | WsClientMessage::Message { content, project_id, .. } => {
                                    info!("💬 Processing chat message from {}", addr);
                                    *is_processing.lock().await = true;

                                    let app_state = app_state.clone();
                                    let sender = sender.clone();
                                    let is_processing_clone = is_processing.clone();
                                    let last_activity_clone = last_activity.clone();
                                    let last_any_send_clone = last_any_send.clone();

                                    tokio::spawn(async move {
                                        if let Err(e) = handle_chat_message(
                                            content,
                                            project_id,
                                            app_state,
                                            sender,
                                            addr,
                                            last_any_send_clone,
                                        ).await {
                                            error!("Error handling chat message: {}", e);
                                        }
                                        *is_processing_clone.lock().await = false;
                                        *last_activity_clone.lock().await = Instant::now();
                                    });
                                }
                                WsClientMessage::Status { message } => {
                                    debug!("📊 Status message: {}", message);
                                }
                                WsClientMessage::Command { .. } => {
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

    // Session + persona
    let session_id = std::env::var("MIRA_SESSION_ID").unwrap_or_else(|_| "peter-eternal".to_string());
    let persona_str = std::env::var("MIRA_DEFAULT_PERSONA").unwrap_or_else(|_| "default".to_string());
    let persona = persona_str.parse::<PersonaOverlay>().unwrap_or(PersonaOverlay::Default);

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
    .await
    {
        Ok(s) => s,
        Err(e) => {
            error!("❌ Failed to get content stream: {}", e);

            let error_msg = WsServerMessage::Error {
                message: "Failed to generate response".to_string(),
                code: Some("STREAM_ERROR".to_string()),
            };

            let mut lock = sender.lock().await;
            let _ = lock.send(Message::Text(serde_json::to_string(&error_msg)?)).await;
            *last_any_send.lock().await = Instant::now();
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

                let msg = WsServerMessage::Chunk {
                    content: chunk,
                    mood: if chunks_sent == 1 { Some(metadata.mood.clone()) } else { None },
                };

                if let Ok(text) = serde_json::to_string(&msg) {
                    let mut lock = sender.lock().await;
                    if let Err(e) = lock.send(Message::Text(text)).await {
                        warn!("⚠️ Failed to send chunk {}: {}", chunks_sent, e);
                        break;
                    } else {
                        *last_any_send.lock().await = Instant::now();
                    }
                } else {
                    warn!("Failed to serialize chunk message");
                }
            }
            Ok(StreamEvent::Done { .. }) => {
                info!("✅ Stream complete: {} chunks sent", chunks_sent);
                break;
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
