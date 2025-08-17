// src/api/ws/chat.rs
// NDJSON chunking handler - sends multiple JSON chunks for long responses

use std::sync::Arc;
use std::time::Duration;

use axum::{
    extract::{State, WebSocketUpgrade},
    response::IntoResponse,
};
use axum::extract::ws::{Message, WebSocket};
use futures::{SinkExt, StreamExt};
use tokio::sync::Mutex;
use tokio::time::interval;
use tracing::{error, info, warn, debug};

use crate::api::ws::message::{WsClientMessage, WsServerMessage};
use crate::llm::streaming::{stream_response, StreamEvent};
use crate::state::AppState;
use crate::persona::PersonaOverlay;
use crate::memory::recall::RecallContext;
use crate::services::chat::ChatResponse;

/// WebSocket upgrade endpoint for chat
pub async fn ws_chat_handler(
    ws: WebSocketUpgrade,
    State(app_state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, app_state))
}

/// Build system prompt for NDJSON chunking mode
fn build_chunked_system_prompt(persona: &PersonaOverlay, context: &RecallContext) -> String {
    let mut prompt = String::new();
    
    // Add persona
    prompt.push_str(persona.prompt());
    prompt.push_str("\n\n");
    
    // Add context
    if !context.recent.is_empty() {
        prompt.push_str("Recent conversation:\n");
        for entry in &context.recent {
            prompt.push_str(&format!("{}: {}\n", entry.role, entry.content));
        }
        prompt.push_str("\n");
    }
    
    // CRITICAL: Modified instructions for NDJSON chunking
    prompt.push_str(r#"CRITICAL OUTPUT FORMAT:

You must respond with one or more JSON objects, each on its own line (NDJSON format).
Each JSON object represents a chunk of your response.

For responses under ~1000 tokens, send a single JSON object:
{"chunk": 1, "total": 1, "output": "your complete response", "mood": "your mood", "salience": 7, "memory_type": "event", "tags": ["relevant", "tags"], "intent": "your intent", "summary": "brief summary", "complete": true}

For longer responses, split into multiple chunks:
{"chunk": 1, "total": 3, "output": "first ~1000 tokens of response...", "mood": "excited", "salience": 8}
{"chunk": 2, "total": 3, "output": "next ~1000 tokens...", "mood": "excited", "salience": 8}
{"chunk": 3, "total": 3, "output": "final part", "mood": "excited", "salience": 8, "memory_type": "explanation", "tags": ["technical", "detailed"], "intent": "educate", "summary": "Explained X in detail", "complete": true}

Rules:
- Each line must be a valid JSON object
- Keep each chunk's "output" under 1000 tokens to stay within limits
- Include "chunk" (current number) and "total" (expected total) in each object
- The final chunk must include "complete": true
- The final chunk should include: memory_type, tags, intent, and summary
- All chunks should include mood and salience
- Do not add any text before, after, or between the JSON lines
- Each JSON object should be on its own line with no line breaks within the JSON

Remember: You are Mira. Never break character."#);
    
    prompt
}

async fn handle_socket(socket: WebSocket, app_state: Arc<AppState>) {
    let (sender, mut receiver) = socket.split();
    let sender = Arc::new(Mutex::new(sender));

    info!("🔌 WS client connected");
    
    // Track current persona for this session
    let current_persona = PersonaOverlay::Default;
    
    // Spawn heartbeat task
    let heartbeat_sender = sender.clone();
    let heartbeat_handle = tokio::spawn(async move {
        let mut ticker = interval(Duration::from_secs(30));
        loop {
            ticker.tick().await;
            let status = WsServerMessage::Status {
                message: "heartbeat".to_string(),
                detail: Some("ping".to_string()),
            };
            let mut lock = heartbeat_sender.lock().await;
            if lock
                .send(Message::Text(serde_json::to_string(&status).unwrap()))
                .await
                .is_err()
            {
                break;
            }
        }
        debug!("Heartbeat task ended");
    });

    // Buffers for accumulating responses
    let mut accumulated_output = String::new();
    let mut current_chunk_buffer = String::new();
    let mut chunks_received = 0;
    let mut last_mood = "present".to_string();
    let mut last_salience = 5;
    let mut final_metadata: Option<serde_json::Value> = None;

    while let Some(msg) = receiver.next().await {
        match msg {
            Ok(Message::Text(text)) => {
                let parsed: Result<WsClientMessage, _> = serde_json::from_str(&text);
                match parsed {
                    Ok(WsClientMessage::Chat { content, project_id, .. }) => {
                        if content.trim().to_lowercase() == "pong" {
                            debug!("Received pong from client");
                            continue;
                        }
                        
                        info!("💬 Received message: {}", content);
                        
                        // Reset state for new message
                        accumulated_output.clear();
                        current_chunk_buffer.clear();
                        chunks_received = 0;
                        final_metadata = None;
                        
                        // Save user message
                        let _ = app_state.memory_service.save_user_message(
                            "peter-eternal",
                            &content,
                            project_id.as_deref(),
                        ).await;
                        info!("💾 Saved user message to memory");

                        // Get memory context
                        let embedding = match app_state.llm_client.get_embedding(&content).await {
                            Ok(emb) => Some(emb),
                            Err(e) => {
                                warn!("Failed to generate embedding: {}", e);
                                None
                            }
                        };
                        
                        let context = match app_state.context_service
                            .build_context(
                                "peter-eternal",
                                embedding.as_deref(),
                                project_id.as_deref()
                            )
                            .await {
                            Ok(ctx) => ctx,
                            Err(e) => {
                                warn!("Failed to get memory context: {}", e);
                                RecallContext::new(Vec::new(), Vec::new())
                            }
                        };
                        
                        // Build prompt for NDJSON chunking
                        let system_prompt = build_chunked_system_prompt(&current_persona, &context);
                        
                        // Always use structured JSON (but with chunking support)
                        let structured_json = true;
                        
                        match stream_response(
                            &app_state.llm_client,
                            &content,
                            Some(&system_prompt),
                            structured_json,
                        ).await {
                            Ok(mut stream) => {
                                while let Some(event) = stream.next().await {
                                    match event {
                                        Ok(StreamEvent::Delta(chunk)) => {
                                            if !chunk.is_empty() {
                                                current_chunk_buffer.push_str(&chunk);
                                                
                                                // Try to parse complete JSON lines
                                                while let Some(newline_pos) = current_chunk_buffer.find('\n') {
                                                    let (line, rest) = current_chunk_buffer.split_at(newline_pos);
                                                    let line = line.trim();
                                                    
                                                    if !line.is_empty() {
                                                        // Try to parse this line as JSON
                                                        if let Ok(json_chunk) = serde_json::from_str::<serde_json::Value>(line) {
                                                            chunks_received += 1;
                                                            info!("📦 Received chunk {}", chunks_received);
                                                            
                                                            // Extract output text
                                                            if let Some(output) = json_chunk.get("output").and_then(|v| v.as_str()) {
                                                                accumulated_output.push_str(output);
                                                                
                                                                // Extract metadata
                                                                let mood = json_chunk.get("mood")
                                                                    .and_then(|v| v.as_str())
                                                                    .unwrap_or("present");
                                                                last_mood = mood.to_string();
                                                                
                                                                if let Some(sal) = json_chunk.get("salience").and_then(|v| v.as_u64()) {
                                                                    last_salience = sal as usize;
                                                                }
                                                                
                                                                // Send chunk to frontend
                                                                let ws_msg = WsServerMessage::Chunk {
                                                                    content: output.to_string(),
                                                                    mood: Some(mood.to_string()),
                                                                };
                                                                
                                                                let mut lock = sender.lock().await;
                                                                if lock.send(Message::Text(serde_json::to_string(&ws_msg).unwrap())).await.is_err() {
                                                                    error!("Failed to send chunk");
                                                                    break;
                                                                }
                                                                
                                                                // Check if this is the final chunk
                                                                if json_chunk.get("complete").and_then(|v| v.as_bool()).unwrap_or(false) {
                                                                    info!("✅ Final chunk received");
                                                                    final_metadata = Some(json_chunk.clone());
                                                                }
                                                            }
                                                        } else if line.contains("\"output\"") {
                                                            // Might be partial JSON, keep accumulating
                                                            debug!("Partial JSON detected, continuing accumulation");
                                                        }
                                                    }
                                                    
                                                    // Move past the newline
                                                    current_chunk_buffer = rest[1..].to_string();
                                                }
                                            }
                                        }
                                        Ok(StreamEvent::Done { .. }) => {
                                            info!("✅ Stream complete");
                                            
                                            // Try to parse any remaining buffer as final chunk
                                            if !current_chunk_buffer.trim().is_empty() {
                                                if let Ok(json_chunk) = serde_json::from_str::<serde_json::Value>(current_chunk_buffer.trim()) {
                                                    if let Some(output) = json_chunk.get("output").and_then(|v| v.as_str()) {
                                                        accumulated_output.push_str(output);
                                                        
                                                        let ws_msg = WsServerMessage::Chunk {
                                                            content: output.to_string(),
                                                            mood: json_chunk.get("mood")
                                                                .and_then(|v| v.as_str())
                                                                .map(String::from),
                                                        };
                                                        
                                                        let mut lock = sender.lock().await;
                                                        let _ = lock.send(Message::Text(serde_json::to_string(&ws_msg).unwrap())).await;
                                                        
                                                        if json_chunk.get("complete").and_then(|v| v.as_bool()).unwrap_or(false) {
                                                            final_metadata = Some(json_chunk);
                                                        }
                                                    }
                                                }
                                            }
                                            
                                            // Save complete response with metadata
                                            let response = if let Some(ref metadata) = final_metadata {
                                                ChatResponse {
                                                    output: accumulated_output.clone(),
                                                    persona: "mira".to_string(),
                                                    mood: metadata.get("mood")
                                                        .and_then(|v| v.as_str())
                                                        .unwrap_or("present")
                                                        .to_string(),
                                                    salience: metadata.get("salience")
                                                        .and_then(|v| v.as_u64())
                                                        .unwrap_or(5) as usize,
                                                    summary: metadata.get("summary")
                                                        .and_then(|v| v.as_str())
                                                        .unwrap_or("Chat response")
                                                        .to_string(),
                                                    memory_type: metadata.get("memory_type")
                                                        .and_then(|v| v.as_str())
                                                        .unwrap_or("event")
                                                        .to_string(),
                                                    tags: metadata.get("tags")
                                                        .and_then(|v| v.as_array())
                                                        .map(|arr| arr.iter()
                                                            .filter_map(|v| v.as_str().map(String::from))
                                                            .collect())
                                                        .unwrap_or_else(Vec::new),
                                                    intent: metadata.get("intent")
                                                        .and_then(|v| v.as_str())
                                                        .unwrap_or("response")
                                                        .to_string(),
                                                    monologue: metadata.get("monologue")
                                                        .and_then(|v| v.as_str())
                                                        .map(String::from),
                                                    reasoning_summary: metadata.get("reasoning_summary")
                                                        .and_then(|v| v.as_str())
                                                        .map(String::from),
                                                }
                                            } else {
                                                // Fallback if no final metadata
                                                ChatResponse {
                                                    output: accumulated_output.clone(),
                                                    persona: "mira".to_string(),
                                                    mood: last_mood.clone(),
                                                    salience: last_salience,
                                                    summary: "Chat response".to_string(),
                                                    memory_type: "event".to_string(),
                                                    tags: vec![],
                                                    intent: "response".to_string(),
                                                    monologue: None,
                                                    reasoning_summary: None,
                                                }
                                            };
                                            
                                            let _ = app_state.memory_service.save_assistant_response(
                                                "peter-eternal",
                                                &response,
                                            ).await;
                                            
                                            info!("💾 Saved complete response ({} chars, {} chunks)", 
                                                accumulated_output.len(), chunks_received);
                                            
                                            // Send completion message
                                            let complete = WsServerMessage::Complete {
                                                mood: Some(response.mood),
                                                salience: Some(response.salience as f32),
                                                tags: Some(response.tags),
                                            };
                                            
                                            let mut lock = sender.lock().await;
                                            let _ = lock.send(Message::Text(serde_json::to_string(&complete).unwrap())).await;
                                            
                                            current_chunk_buffer.clear();
                                        }
                                        Ok(StreamEvent::Error(e)) => {
                                            error!("Stream error: {}", e);
                                            let msg = WsServerMessage::Error {
                                                message: format!("Stream error: {}", e),
                                                code: Some("STREAM_ERROR".to_string()),
                                            };
                                            let mut lock = sender.lock().await;
                                            let _ = lock.send(Message::Text(serde_json::to_string(&msg).unwrap())).await;
                                            break;
                                        }
                                        Err(e) => {
                                            error!("Stream processing error: {}", e);
                                            break;
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                error!("Failed to start stream: {e}");
                                let msg = WsServerMessage::Error {
                                    message: format!("Failed to start stream: {e}"),
                                    code: Some("STREAM_START_FAILED".to_string()),
                                };
                                let mut lock = sender.lock().await;
                                let _ = lock.send(Message::Text(serde_json::to_string(&msg).unwrap())).await;
                            }
                        }
                    }

                    Ok(WsClientMessage::Command { command, .. }) => {
                        if command == "pong" || command == "heartbeat" {
                            debug!("Received heartbeat response");
                        } else {
                            let msg = WsServerMessage::Status {
                                message: format!("ack:{command}"),
                                detail: None,
                            };
                            let mut lock = sender.lock().await;
                            let _ = lock.send(Message::Text(serde_json::to_string(&msg).unwrap())).await;
                        }
                    }

                    Ok(_) => {
                        debug!("Ignoring client-side meta message");
                    }

                    Err(e) => {
                        warn!("Failed to parse client message: {e}");
                        let msg = WsServerMessage::Error {
                            message: format!("Bad client message: {e}"),
                            code: Some("BAD_CLIENT_MESSAGE".to_string()),
                        };
                        let mut lock = sender.lock().await;
                        let _ = lock.send(Message::Text(serde_json::to_string(&msg).unwrap())).await;
                    }
                }
            }

            Ok(Message::Ping(p)) => {
                let mut lock = sender.lock().await;
                let _ = lock.send(Message::Pong(p)).await;
            }
            Ok(Message::Pong(_)) => {
                debug!("Received pong");
            }
            Ok(Message::Close(_)) => {
                info!("🔌 WS client closed");
                break;
            }
            Ok(other) => {
                warn!("Ignoring non-text WS message: {:?}", other);
            }
            Err(e) => {
                error!("WS receive error: {e}");
                break;
            }
        }
    }

    heartbeat_handle.abort();
    info!("🔌 WS handler done");
}
