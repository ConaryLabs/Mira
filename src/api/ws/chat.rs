// src/api/ws/chat.rs

use axum::{
    extract::{WebSocketUpgrade, State},
    response::IntoResponse,
};
use axum::extract::ws::{WebSocket, Message};
use futures::{sink::SinkExt, stream::StreamExt};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{timeout, Duration};
// Removed unused imports
use chrono::Utc;

use crate::handlers::AppState;
use crate::api::ws::message::{WsClientMessage, WsServerMessage};
use crate::persona::PersonaOverlay;

/// Main WebSocket handler for chat connections
pub async fn ws_chat_handler(
    ws: WebSocketUpgrade,
    State(app_state): State<Arc<AppState>>,
) -> impl IntoResponse {
    eprintln!("New WebSocket connection request");
    ws.on_upgrade(move |socket| handle_socket(socket, app_state))
}

/// Handles an individual WebSocket connection
async fn handle_socket(socket: WebSocket, app_state: Arc<AppState>) {
    let session_id = "peter-eternal".to_string();
    eprintln!("WebSocket connected for session: {}", session_id);
    
    let (sender, mut receiver) = socket.split();
    let sender = Arc::new(Mutex::new(sender));
    
    // Track connection state
    let mut current_mood = "attentive".to_string();
    let current_persona = PersonaOverlay::Default;
    let mut active_project_id: Option<String> = None;
    
    // Send initial greeting chunk instead of Connected message
    let greeting_msg = WsServerMessage::Chunk {
        content: "Connected! How can I help you today?".to_string(),
        mood: Some("attentive".to_string()),
    };
    
    {
        let mut sender_guard = sender.lock().await;
        if let Err(e) = sender_guard.send(Message::Text(
            serde_json::to_string(&greeting_msg).unwrap()
        )).await {
            eprintln!("Failed to send greeting message: {}", e);
            return;
        }
    }
    
    // Setup heartbeat
    let sender_clone = sender.clone();
    let (heartbeat_shutdown_tx, mut heartbeat_shutdown_rx) = tokio::sync::mpsc::channel::<()>(1);
    let heartbeat_handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(30));
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    let mut sender_guard = sender_clone.lock().await;
                    match sender_guard.send(Message::Ping(vec![])).await {
                        Ok(_) => {
                            eprintln!("Sent WebSocket ping");
                        }
                        Err(e) => {
                            eprintln!("Heartbeat failed: {}, stopping heartbeat", e);
                            break;
                        }
                    }
                    // Drop the guard immediately after use
                    drop(sender_guard);
                }
                _ = heartbeat_shutdown_rx.recv() => {
                    eprintln!("Heartbeat shutdown signal received");
                    break;
                }
            }
        }
        eprintln!("Heartbeat task ended");
    });
    
    // Main message handling loop
    while let Some(msg) = receiver.next().await {
        match msg {
            Ok(Message::Text(text)) => {
                eprintln!("Received WebSocket message: {}", text);
                
                let incoming: Result<WsClientMessage, _> = serde_json::from_str(&text);
                match incoming {
                    Ok(WsClientMessage::Message { content, persona: _, project_id }) => {
                        eprintln!("Processing message: {}", content);
                        
                        // NOTE: Ignoring persona field - personas emerge naturally
                        
                        // Update active project if specified
                        if project_id.is_some() {
                            active_project_id = project_id.clone();
                        }
                        
                        // Send immediate acknowledgment (no persona shown)
                        let thinking_msg = WsServerMessage::Chunk {
                            content: "".to_string(),
                            mood: Some("thinking".to_string()),
                        };
                        
                        {
                            let mut sender_guard = sender.lock().await;
                            if let Err(e) = sender_guard.send(Message::Text(
                                serde_json::to_string(&thinking_msg).unwrap()
                            )).await {
                                eprintln!("Failed to send thinking message: {}", e);
                                break; // Connection is broken, exit the loop
                            }
                        }
                        
                        // Handle the message
                        stream_chat_response(
                            sender.clone(),
                            &app_state,
                            &session_id,
                            content,
                            &current_persona,
                            &mut current_mood,
                            active_project_id.as_deref(),
                        ).await;
                    }
                    
                    Ok(WsClientMessage::Typing { .. }) => {
                        // Ignore typing indicators for now
                        eprintln!("Received typing indicator");
                    }
                    
                    Err(e) => {
                        eprintln!("Failed to parse WebSocket message: {}", e);
                        let error_msg = WsServerMessage::Error {
                            message: "Invalid message format".to_string(),
                            code: Some("PARSE_ERROR".to_string()),
                        };
                        let mut sender_guard = sender.lock().await;
                        if let Err(e) = sender_guard.send(Message::Text(
                            serde_json::to_string(&error_msg).unwrap()
                        )).await {
                            eprintln!("Failed to send error message: {}", e);
                            break;
                        }
                    }
                }
            }
            Ok(Message::Pong(_)) => {
                eprintln!("Received pong");
            }
            Ok(Message::Close(_)) => {
                eprintln!("WebSocket connection closed by client");
                break;
            }
            Err(e) => {
                eprintln!("WebSocket error: {}", e);
                break;
            }
            _ => {}
        }
    }
    
    // Signal heartbeat to stop
    let _ = heartbeat_shutdown_tx.send(()).await;
    
    // Wait for heartbeat task to finish (with timeout)
    let _ = tokio::time::timeout(Duration::from_secs(1), heartbeat_handle).await;
    
    eprintln!("WebSocket handler ended for session: {}", session_id);
}

async fn stream_chat_response(
    sender: Arc<Mutex<futures::stream::SplitSink<WebSocket, Message>>>,
    app_state: &Arc<AppState>,
    session_id: &str,
    content: String,
    persona: &PersonaOverlay,
    current_mood: &mut String,
    project_id: Option<&str>,
) {
    eprintln!("[{}] Starting chat response stream for: {}", Utc::now(), content);
    eprintln!("Using persona internally: {}", persona);
    
    // Use the chat service with timeout
    let response = match timeout(
        Duration::from_secs(30),
        app_state.chat_service.process_message(
            session_id,
            &content,
            persona,
            project_id,
        )
    ).await {
        Ok(Ok(resp)) => resp,
        Ok(Err(e)) => {
            eprintln!("[{}] Chat service error: {:?}", Utc::now(), e);
            let error_msg = WsServerMessage::Error {
                message: "Failed to process message".to_string(),
                code: Some("PROCESSING_ERROR".to_string()),
            };
            let mut sender_guard = sender.lock().await;
            let _ = sender_guard.send(Message::Text(
                serde_json::to_string(&error_msg).unwrap()
            )).await;
            return;
        }
        Err(_) => {
            eprintln!("[{}] Chat service timeout", Utc::now());
            let error_msg = WsServerMessage::Error {
                message: "Request timed out".to_string(),
                code: Some("TIMEOUT".to_string()),
            };
            let mut sender_guard = sender.lock().await;
            let _ = sender_guard.send(Message::Text(
                serde_json::to_string(&error_msg).unwrap()
            )).await;
            return;
        }
    };

    eprintln!("[{}] Got chat response, streaming chunks", Utc::now());

    // Update mood
    *current_mood = response.mood.clone();

    // Stream the response in chunks
    let output = response.output.clone();
    let words: Vec<&str> = output.split_whitespace().collect();
    let chunk_size = 5; // Words per chunk
    
    for (i, chunk) in words.chunks(chunk_size).enumerate() {
        let is_first = i == 0;
        let chunk_text = if is_first {
            chunk.join(" ")
        } else {
            format!(" {}", chunk.join(" "))
        };
        
        // Send chunk WITHOUT persona info
        let chunk_msg = WsServerMessage::Chunk {
            content: chunk_text,
            mood: Some(current_mood.clone()),
        };
        
        {
            let mut sender_guard = sender.lock().await;
            if let Err(e) = sender_guard.send(Message::Text(
                serde_json::to_string(&chunk_msg).unwrap()
            )).await {
                eprintln!("Failed to send chunk: {}", e);
                return; // Stop streaming if connection is broken
            }
        }
        
        // Small delay between chunks for streaming effect
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    
    // Send emotional asides if present
    if let Some(monologue) = &response.monologue {
        if !monologue.is_empty() {
            let aside_msg = WsServerMessage::Aside {
                emotional_cue: monologue.clone(),
                intensity: response.aside_intensity,
            };
            let mut sender_guard = sender.lock().await;
            let _ = sender_guard.send(Message::Text(
                serde_json::to_string(&aside_msg).unwrap()
            )).await;
        }
    }
    
    // Send done message
    let done_msg = WsServerMessage::Done;
    {
        let mut sender_guard = sender.lock().await;
        let _ = sender_guard.send(Message::Text(
            serde_json::to_string(&done_msg).unwrap()
        )).await;
    }

    eprintln!("[{}] Finished streaming response", Utc::now());
}
