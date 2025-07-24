// src/api/ws/chat.rs

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::IntoResponse;
use futures::{SinkExt, StreamExt};
use std::sync::Arc;

use crate::api::ws::message::{WsClientMessage, WsServerMessage};
use crate::handlers::AppState;
use crate::persona::PersonaOverlay;
use crate::memory::recall::{RecallContext, build_context};
use crate::memory::traits::MemoryStore;
use crate::memory::types::MemoryEntry;
use crate::prompt::builder::build_system_prompt;
use crate::llm::{emotional_weight, MiraStructuredReply};
use chrono::Utc;

pub async fn ws_chat_handler(
    ws: WebSocketUpgrade,
    State(app_state): State<Arc<AppState>>,
) -> impl IntoResponse {
    eprintln!("WebSocket upgrade requested!");
    eprintln!("App state is valid: {:?}", Arc::strong_count(&app_state));
    
    ws.on_upgrade(move |socket| {
        eprintln!("WebSocket upgrade callback triggered!");
        handle_ws(socket, app_state)
    })
}

async fn handle_ws(socket: WebSocket, app_state: Arc<AppState>) {
    let (mut sender, mut receiver) = socket.split();
    
    // Use peter-eternal for consistency with REST endpoint
    let session_id = "peter-eternal".to_string();
    let mut current_persona = PersonaOverlay::Default;
    let mut current_mood = "present".to_string();
    
    eprintln!("WebSocket connection established. Session: {}", session_id);
    
    while let Some(msg) = receiver.next().await {
        match msg {
            Ok(Message::Text(text)) => {
                eprintln!("Received WebSocket message: {}", text);
                
                let incoming: Result<WsClientMessage, _> = serde_json::from_str(&text);
                match incoming {
                    Ok(WsClientMessage::Message { content, persona }) => {
                        eprintln!("Processing message: {}", content);
                        
                        // Override persona if specified
                        if let Some(p) = persona.as_ref() {
                            if let Ok(new_persona) = p.parse::<PersonaOverlay>() {
                                current_persona = new_persona;
                            }
                        }
                        
                        // Handle the message
                        stream_chat_response(
                            &mut sender,
                            &app_state,
                            &session_id,
                            content,
                            &current_persona,
                            &mut current_mood,
                        ).await;
                    }
                    
                    Ok(WsClientMessage::SwitchPersona { persona, smooth_transition }) => {
                        if let Ok(new_persona) = persona.parse::<PersonaOverlay>() {
                            // Send transition if switching
                            if new_persona != current_persona && smooth_transition {
                                let transition_msg = create_persona_transition(
                                    &current_persona,
                                    &new_persona,
                                    &current_mood,
                                );
                                let _ = sender.send(Message::Text(
                                    serde_json::to_string(&transition_msg).unwrap()
                                )).await;
                            }
                            
                            current_persona = new_persona;
                            
                            // Send update confirmation
                            let update = WsServerMessage::PersonaUpdate {
                                persona: current_persona.to_string(),
                                mood: Some(current_mood.clone()),
                                transition_note: None,
                            };
                            let _ = sender.send(Message::Text(
                                serde_json::to_string(&update).unwrap()
                            )).await;
                        }
                    }
                    
                    Ok(WsClientMessage::GetMemoryStats { session_id: query_session }) => {
                        send_memory_stats(
                            &mut sender,
                            &app_state,
                            &query_session.unwrap_or(session_id.clone()),
                        ).await;
                    }
                    
                    Ok(WsClientMessage::Typing { .. }) => {
                        // Ignore typing indicators for now
                    }
                    
                    Err(e) => {
                        eprintln!("Failed to parse WebSocket message: {}", e);
                        let error_msg = WsServerMessage::Error {
                            message: "Invalid message format".to_string(),
                            code: Some("PARSE_ERROR".to_string()),
                        };
                        let _ = sender.send(Message::Text(
                            serde_json::to_string(&error_msg).unwrap()
                        )).await;
                    }
                }
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
    
    eprintln!("WebSocket handler ended for session: {}", session_id);
}

async fn stream_chat_response(
    sender: &mut futures::stream::SplitSink<WebSocket, Message>,
    app_state: &Arc<AppState>,
    session_id: &str,
    content: String,
    persona: &PersonaOverlay,
    current_mood: &mut String,
) {
    eprintln!("Starting chat response stream for: {}", content);
    
    // Get embedding for semantic search
    let user_embedding = match app_state.llm_client.get_embedding(&content).await {
        Ok(emb) => Some(emb),
        Err(e) => {
            eprintln!("Failed to get embedding: {}", e);
            None
        }
    };

    // Build recall context
    let recall_context = build_context(
        session_id,
        user_embedding.as_deref(),
        15,  // recent messages
        5,   // semantic matches
        app_state.sqlite_store.as_ref(),
        app_state.qdrant_store.as_ref(),
    )
    .await
    .unwrap_or_else(|e| {
        eprintln!("Failed to build recall context: {}", e);
        RecallContext::new(vec![], vec![])
    });

    // Use the SAME system prompt as REST for structured JSON
    let system_prompt = build_system_prompt(persona, &recall_context);

    // Get emotional weight for model routing
    let emotional_weight = match emotional_weight::classify(&app_state.llm_client, &content).await {
        Ok(val) => val,
        Err(e) => {
            eprintln!("Failed to classify emotional weight: {}", e);
            0.0
        }
    };
    
    let model = if emotional_weight > 0.95 {
        "o3"
    } else if emotional_weight > 0.6 {
        "o4-mini"
    } else {
        "gpt-4.1"
    };

    // Get the complete structured response first
    let mira_reply = match app_state.llm_client.chat_with_model(&content, model).await {
        Ok(resp) => resp,
        Err(e) => {
            eprintln!("Failed to call OpenAI: {}", e);
            let error_msg = WsServerMessage::Error {
                message: "Service temporarily unavailable".to_string(),
                code: Some("LLM_ERROR".to_string()),
            };
            let _ = sender.send(Message::Text(
                serde_json::to_string(&error_msg).unwrap()
            )).await;
            return;
        }
    };

    // Update mood
    *current_mood = mira_reply.mood.clone();

    // Stream the response in chunks
    let output = mira_reply.output.clone();
    let words: Vec<&str> = output.split_whitespace().collect();
    let chunk_size = 5; // Words per chunk
    
    for (i, chunk) in words.chunks(chunk_size).enumerate() {
        let is_first = i == 0;
        let chunk_text = if is_first {
            chunk.join(" ")
        } else {
            format!(" {}", chunk.join(" "))
        };
        
        let ws_chunk = WsServerMessage::Chunk {
            content: chunk_text,
            persona: persona.to_string(),
            mood: if is_first { Some(mira_reply.mood.clone()) } else { None },
        };
        
        if let Ok(msg_text) = serde_json::to_string(&ws_chunk) {
            let _ = sender.send(Message::Text(msg_text)).await;
            
            // Small delay to simulate typing
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        }
    }

    // Send done message
    let done = WsServerMessage::Done;
    if let Ok(msg_text) = serde_json::to_string(&done) {
        let _ = sender.send(Message::Text(msg_text)).await;
    }

    // Save interaction to memory (same as REST endpoint)
    // Save user message
    if let Some(embedding) = user_embedding {
        let entry = MemoryEntry {
            id: None,
            session_id: session_id.to_string(),
            role: "user".to_string(),
            content,
            timestamp: Utc::now(),
            embedding: Some(embedding),
            salience: Some(5.0),
            tags: Some(vec![current_mood.clone()]),
            summary: None,
            memory_type: None,
            logprobs: None,
            moderation_flag: None,
            system_fingerprint: None,
        };
        
        if let Err(e) = app_state.sqlite_store.save(&entry).await {
            eprintln!("Failed to save user message: {}", e);
        }
    }

    // Save Mira's response with full metadata
    let mira_embedding = app_state.llm_client
        .get_embedding(&mira_reply.output)
        .await
        .ok();

    let mira_entry = MemoryEntry {
        id: None,
        session_id: session_id.to_string(),
        role: "assistant".to_string(),
        content: mira_reply.output,
        timestamp: Utc::now(),
        embedding: mira_embedding,
        salience: Some(mira_reply.salience as f32),
        tags: Some(mira_reply.tags),
        summary: mira_reply.summary,
        memory_type: Some(match mira_reply.memory_type.as_str() {
            "feeling" => crate::memory::types::MemoryType::Feeling,
            "fact" => crate::memory::types::MemoryType::Fact,
            "joke" => crate::memory::types::MemoryType::Joke,
            "promise" => crate::memory::types::MemoryType::Promise,
            "event" => crate::memory::types::MemoryType::Event,
            _ => crate::memory::types::MemoryType::Other,
        }),
        logprobs: None,
        moderation_flag: None,
        system_fingerprint: None,
    };
    
    if let Err(e) = app_state.sqlite_store.save(&mira_entry).await {
        eprintln!("Failed to save assistant message: {}", e);
    }
}

fn create_persona_transition(
    from: &PersonaOverlay,
    to: &PersonaOverlay,
    mood: &str,
) -> WsServerMessage {
    WsServerMessage::PersonaUpdate {
        persona: to.to_string(),
        mood: Some(mood.to_string()),
        transition_note: Some(format!("Shifting from {} to {}", from, to)),
    }
}

async fn send_memory_stats(
    sender: &mut futures::stream::SplitSink<WebSocket, Message>,
    app_state: &Arc<AppState>,
    session_id: &str,
) {
    // TODO: Implement actual memory stats
    let stats = WsServerMessage::MemoryStats {
        total_memories: 0,
        high_salience_count: 0,
        avg_salience: 0.0,
        mood_distribution: std::collections::HashMap::new(),
    };
    
    if let Ok(msg_text) = serde_json::to_string(&stats) {
        let _ = sender.send(Message::Text(msg_text)).await;
    }
}
