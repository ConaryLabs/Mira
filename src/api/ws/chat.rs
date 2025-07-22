// src/api/ws/chat.rs

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::IntoResponse;
use futures::{SinkExt, StreamExt};
use serde_json::json;
use std::sync::Arc;

use crate::api::ws::message::{WsClientMessage, WsServerMessage};
use crate::llm::openai::OpenAIClient;
use crate::handlers::AppState;
use crate::persona::PersonaOverlay;
use crate::memory::recall::RecallContext;
use crate::prompt;

pub async fn ws_chat_handler(
    ws: WebSocketUpgrade,
    State(app_state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_ws(socket, app_state))
}

async fn handle_ws(mut socket: WebSocket, _app_state: Arc<AppState>) {
    let llm_client = OpenAIClient::new();

    while let Some(Ok(msg)) = socket.recv().await {
        match msg {
            Message::Text(text) => {
                let incoming: Result<WsClientMessage, _> = serde_json::from_str(&text);
                match incoming {
                    Ok(WsClientMessage::Message { content, persona }) => {
                        let persona_overlay = persona
                            .as_ref()
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(PersonaOverlay::Default);

                        let recall_context = RecallContext::new(vec![], vec![]);

                        let system_prompt = prompt::build_system_prompt(&persona_overlay, &recall_context);
                        let model = "gpt-4.1";

                        let mut stream = llm_client
                            .stream_gpt4_ws_messages(
                                content,
                                Some(persona_overlay.to_string()),
                                system_prompt,
                                Some(model),
                            )
                            .await;

                        while let Some(server_msg) = stream.next().await {
                            let msg_text = serde_json::to_string(&server_msg).unwrap();
                            let _ = socket.send(Message::Text(msg_text)).await;
                        }
                    }
                    Ok(WsClientMessage::Typing { .. }) => {
                        // Optionally: handle typing indicator
                    }
                    Err(e) => {
                        let _ = socket.send(Message::Text(json!({
                            "type": "error",
                            "error": format!("Malformed WSClientMessage: {e}")
                        }).to_string())).await;
                    }
                }
            }
            Message::Binary(_) => {}
            Message::Ping(_) | Message::Pong(_) | Message::Close(_) => {}
        }
    }
}
