// src/api/ws/chat.rs
// Final version with borrow checker fixes and cleanup

use std::sync::Arc;
use std::time::Duration;

use axum::{
    extract::{State, WebSocketUpgrade},
    response::IntoResponse,
};
use axum::extract::ws::{Message, WebSocket};
use futures::{stream::SplitSink, SinkExt, StreamExt};
use tokio::sync::Mutex;
use tokio::time::interval;
use tracing::{error, info, debug};

use crate::api::ws::message::{WsClientMessage, WsServerMessage};
use crate::llm::streaming::StreamEvent;
use crate::state::AppState;
use crate::persona::PersonaOverlay;
use crate::services::chat::ChatResponse;
use crate::api::two_phase::{get_metadata, get_content_stream};
use crate::memory::recall::RecallContext;

/// WebSocket upgrade endpoint for chat
pub async fn ws_chat_handler(
    ws: WebSocketUpgrade,
    State(app_state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, app_state))
}

async fn handle_socket(socket: WebSocket, app_state: Arc<AppState>) {
    let (sender, mut receiver) = socket.split();
    let sender = Arc::new(Mutex::new(sender));

    info!("🔌 WS client connected");

    let heartbeat_sender = sender.clone();
    tokio::spawn(async move {
        let mut ticker = interval(Duration::from_secs(30));
        loop {
            ticker.tick().await;
            let status = WsServerMessage::Status {
                message: "heartbeat".to_string(),
                detail: Some("ping".to_string()),
            };
            let mut lock = heartbeat_sender.lock().await;
            if lock.send(Message::Text(serde_json::to_string(&status).unwrap())).await.is_err() {
                break;
            }
        }
        debug!("Heartbeat task ended");
    });

    while let Some(msg) = receiver.next().await {
        if let Ok(Message::Text(text)) = msg {
            if let Ok(client_msg) = serde_json::from_str::<WsClientMessage>(&text) {
                match client_msg {
                    WsClientMessage::Chat { content, project_id, .. } => {
                        let app_state_clone = app_state.clone();
                        let sender_clone = sender.clone();
                        tokio::spawn(async move {
                            if let Err(e) = handle_chat_message(content, project_id, app_state_clone, sender_clone.clone()).await {
                                error!("Error in handle_chat_message: {}", e);
                                let err_msg = WsServerMessage::Error {
                                    message: "An internal error occurred.".to_string(),
                                    code: Some("INTERNAL_ERROR".to_string()),
                                };
                                let mut lock = sender_clone.lock().await;
                                let _ = lock.send(Message::Text(serde_json::to_string(&err_msg).unwrap())).await;
                            }
                        });
                    }
                    WsClientMessage::Command { command, .. } if command == "pong" || command == "heartbeat" => {
                        debug!("Received heartbeat response");
                    }
                    _ => {}
                }
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
) -> Result<(), anyhow::Error> {

    info!("💬 Received message: {}", content);

    let persona = PersonaOverlay::Default;
    let session_id = "peter-eternal";

    app_state.memory_service.save_user_message(
        session_id,
        &content,
        project_id.as_deref(),
    ).await?;

    let context = app_state.context_service
        .build_context(session_id, None, project_id.as_deref())
        .await?;

    let metadata = get_metadata(
        &app_state.llm_client,
        &content,
        &persona,
        &context,
    ).await?;

    info!("📊 Metadata: mood={}, salience={}", metadata.mood, metadata.salience);

    if !metadata.output.is_empty() {
        let preview_msg = WsServerMessage::Chunk {
            content: metadata.output.clone(),
            mood: Some(metadata.mood.clone()),
        };
        let mut lock = sender.lock().await;
        lock.send(Message::Text(serde_json::to_string(&preview_msg)?)).await?;
    }

    let mut content_stream = get_content_stream(
        &app_state.llm_client,
        &content,
        &persona,
        &context,
        &metadata,
    ).await?;

    let mut full_content = String::new();
    while let Some(event) = content_stream.next().await {
        match event? {
            StreamEvent::Delta(chunk) => {
                full_content.push_str(&chunk);
                let ws_msg = WsServerMessage::Chunk {
                    content: chunk,
                    mood: Some(metadata.mood.clone()),
                };
                let mut lock = sender.lock().await;
                lock.send(Message::Text(serde_json::to_string(&ws_msg)?)).await?;
            }
            StreamEvent::Done { .. } => {
                info!("✅ Content stream complete: {} chars", full_content.len());
                break;
            }
            StreamEvent::Error(e) => {
                error!("Content stream error: {}", e);
                break;
            }
        }
    }

    let complete_output = if metadata.output.is_empty() {
        full_content
    } else {
        format!("{}\n\n{}", metadata.output, full_content)
    };
    
    let response = ChatResponse {
        output: complete_output,
        persona: persona.name().to_string(),
        mood: metadata.mood.clone(),
        salience: metadata.salience,
        summary: metadata.summary.clone(),
        memory_type: metadata.memory_type.clone(),
        tags: metadata.tags.clone(),
        intent: metadata.intent.clone(),
        monologue: metadata.monologue.clone(),
        reasoning_summary: metadata.reasoning_summary.clone(),
    };
    
    app_state.memory_service.save_assistant_response(
        session_id,
        &response,
    ).await?;
    
    info!("💾 Saved complete response to memory");

    let complete_msg = WsServerMessage::Complete {
        mood: Some(metadata.mood),
        salience: Some(metadata.salience as f32),
        tags: Some(metadata.tags),
    };
    let mut lock = sender.lock().await;
    lock.send(Message::Text(serde_json::to_string(&complete_msg)?)).await?;

    Ok(())
}
