// src/api/ws/chat.rs
use std::sync::Arc;
use std::time::Duration;

use axum::{
    extract::{State, WebSocketUpgrade},
    response::IntoResponse,
};
use axum::extract::ws::{Message, WebSocket};
use futures::SinkExt;
use futures::StreamExt;
use futures_util::stream::SplitSink; // <- correct SplitSink type path
use tokio::sync::Mutex;
use tokio::time::interval;
use tracing::{debug, error, info};

use crate::api::ws::message::{WsClientMessage, WsServerMessage};
use crate::llm::streaming::StreamEvent;
use crate::persona::PersonaOverlay;
use crate::prompt::builder::build_system_prompt;
use crate::state::AppState;
use crate::memory::recall::{RecallContext, build_context};

pub async fn ws_chat_handler(
    ws: WebSocketUpgrade,
    State(app_state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, app_state))
}

async fn handle_socket(socket: WebSocket, app_state: Arc<AppState>) {
    // Split once; keep only the write half for our heartbeat task & replies.
    let (sender, mut receiver) = socket.split();
    let sender = Arc::new(Mutex::new(sender));

    info!("🔌 WS client connected");

    // Heartbeat: send Ping frames only. NO “pong” text ever sent.
    {
        let sender_for_ping = sender.clone();
        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(30));
            loop {
                ticker.tick().await;
                let mut lock = sender_for_ping.lock().await;
                if let Err(e) = lock.send(Message::Ping(b"hb".to_vec())).await {
                    debug!("Heartbeat ping failed: {}", e);
                    break;
                }
            }
            debug!("Heartbeat task ended");
        });
    }

    while let Some(msg) = receiver.next().await {
        match msg {
            Ok(Message::Text(text)) => {
                match serde_json::from_str::<WsClientMessage>(&text) {
                    Ok(WsClientMessage::Chat { content, project_id, .. })
                    | Ok(WsClientMessage::Message { content, project_id, .. }) => {
                        let app_state = app_state.clone();
                        let sender = sender.clone();
                        tokio::spawn(async move {
                            if let Err(e) = handle_chat_message(content, project_id, app_state, sender).await {
                                error!("Error in handle_chat_message: {}", e);
                            }
                        });
                    }
                    Ok(WsClientMessage::Command { .. }) => {
                        // Ignore commands for now (no-op)
                        debug!("Ignoring WS command");
                    }
                    Ok(other) => debug!("Ignoring WS message: {:?}", other),
                    Err(e) => error!("Invalid WS message: {}", e),
                }
            }
            Ok(Message::Binary(_)) => { /* ignore */ }
            Ok(Message::Ping(_)) => { /* axum auto-pongs; ignore */ }
            Ok(Message::Pong(_)) => { /* ignore; do NOT surface as text */ }
            Ok(Message::Close(_)) => { break; }
            Err(e) => {
                error!("WebSocket error: {}", e);
                break;
            }
        }
    }

    info!("🔌 WS handler done");
}

async fn handle_chat_message(
    content: String,
    project_id: Option<String>,
    app_state: Arc<AppState>,
    sender: Arc<Mutex<SplitSink<WebSocket, Message>>>,
) -> anyhow::Result<()> {
    let session_id = "peter-eternal";
    let persona = PersonaOverlay::Default;

    // Persist user message; ignore errors here to keep UX smooth
    let _ = app_state
        .memory_service
        .save_user_message(session_id, &content, project_id.as_deref())
        .await;

    // Build recall context (recent + semantic)
    let user_embedding = app_state.llm_client.get_embedding(&content).await.ok();
    let context = build_context(
        session_id,
        user_embedding.as_deref(),
        30,   // history cap
        15,   // vector search k
        app_state.sqlite_store.as_ref(),
        app_state.qdrant_store.as_ref(),
    )
    .await
    .unwrap_or_else(|_| RecallContext { recent: vec![], semantic: vec![] });

    // (Optional) keep around for dev inspection / future routing
    let _system_prompt = build_system_prompt(&persona, &context);

    // Phase 1: metadata (non-streaming structured JSON); fall back to defaults on error
    let metadata = match crate::api::two_phase::get_metadata(
        &app_state.llm_client,
        &content,
        &persona,
        &context,
    )
    .await
    {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!("⚠️ Could not parse metadata, using defaults: {e}");
            Default::default()
        }
    };

    // Phase 2: content (returns a small stream wrapper even though generation is non-streaming)
    let mut stream = crate::api::two_phase::get_content_stream(
        &app_state.llm_client,
        &content,
        &persona,
        &context,
        &metadata.mood,
        &metadata.intent,
    )
    .await?;

    let mut full_content = String::new();

    while let Some(event) = stream.next().await {
        match event {
            Ok(StreamEvent::Delta(chunk)) => {
                full_content.push_str(&chunk);

                // Send chunk to client
                let ws_msg = WsServerMessage::Chunk {
                    content: chunk,
                    mood: Some(metadata.mood.clone()),
                };
                let mut lock = sender.lock().await;
                let _ = lock.send(Message::Text(serde_json::to_string(&ws_msg)?)).await;
            }
            Ok(StreamEvent::Done { .. }) => {
                info!("✅ Content stream complete: {} chars", full_content.len());

                let complete_output = if metadata.output.is_empty() {
                    full_content.clone()
                } else {
                    // prepend metadata.output if present, then the streamed text
                    format!("{}\n\n{}", metadata.output, full_content)
                };

                // Build response object mirroring ChatService::ChatResponse
                let response = crate::services::chat::ChatResponse {
                    output: complete_output,
                    persona: persona.to_string(),
                    mood: metadata.mood.clone(),
                    salience: metadata.salience,
                    summary: metadata.summary.clone(),
                    memory_type: if metadata.memory_type.is_empty() {
                        "other".into()
                    } else {
                        metadata.memory_type.clone()
                    },
                    tags: metadata.tags.clone(),
                    intent: metadata.intent.clone(),
                    monologue: metadata.monologue.clone(),
                    reasoning_summary: metadata.reasoning_summary.clone(),
                };

                // Persist assistant response; ignore errors for UX
                let _ = app_state
                    .memory_service
                    .save_assistant_response(session_id, &response)
                    .await;

                // Send completion meta to client
                let complete_msg = WsServerMessage::Complete {
                    mood: Some(response.mood.clone()),
                    salience: Some(response.salience as f32),
                    tags: Some(response.tags.clone()),
                };
                let mut lock = sender.lock().await;
                let _ = lock.send(Message::Text(serde_json::to_string(&complete_msg)?)).await;
                let _ = lock.send(Message::Text(serde_json::to_string(&WsServerMessage::Done)?)).await;
                break;
            }
            Ok(StreamEvent::Error(e)) => {
                error!("Content stream error: {}", e);
                break;
            }
            Err(e) => {
                error!("Stream error: {}", e);
                break;
            }
        }
    }

    Ok(())
}
