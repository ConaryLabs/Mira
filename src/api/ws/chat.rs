// src/api/ws/chat.rs

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::IntoResponse;
use futures::{SinkExt, StreamExt};
use std::sync::Arc;
use tokio::time::{interval, timeout, Duration};

use crate::api::ws::message::{WsClientMessage, WsServerMessage};
use crate::handlers::AppState;
use crate::persona::PersonaOverlay;
use crate::memory::recall::{RecallContext, build_context};
use crate::memory::traits::MemoryStore;
use crate::memory::types::MemoryEntry;
use crate::prompt::builder::build_system_prompt;
use crate::llm::emotional_weight;
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
    let (sender, mut receiver) = socket.split();
    let sender = Arc::new(tokio::sync::Mutex::new(sender));
    
    // Use peter-eternal for consistency with REST endpoint
    let session_id = "peter-eternal".to_string();
    let mut current_persona = PersonaOverlay::Default;
    let mut current_mood = "present".to_string();
    
    eprintln!("WebSocket connection established. Session: {}", session_id);
    
    // Spawn a heartbeat task
    let heartbeat_sender = sender.clone();
    let heartbeat_handle = tokio::spawn(async move {
        let mut interval = interval(Duration::from_secs(30));
        loop {
            interval.tick().await;
            let mut sender = heartbeat_sender.lock().await;
            if sender.send(Message::Ping(vec![])).await.is_err() {
                eprintln!("Heartbeat failed, connection likely closed");
                break;
            }
            eprintln!("Sent WebSocket ping");
        }
    });
    
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
                        
                        // Send immediate acknowledgment
                        let thinking_msg = WsServerMessage::Chunk {
                            content: "".to_string(),
                            persona: current_persona.to_string(),
                            mood: Some("thinking".to_string()),
                        };
                        
                        {
                            let mut sender_guard = sender.lock().await;
                            let _ = sender_guard.send(Message::Text(
                                serde_json::to_string(&thinking_msg).unwrap()
                            )).await;
                        }
                        
                        // Handle the message
                        stream_chat_response(
                            sender.clone(),
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
                                let mut sender_guard = sender.lock().await;
                                let _ = sender_guard.send(Message::Text(
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
                            let mut sender_guard = sender.lock().await;
                            let _ = sender_guard.send(Message::Text(
                                serde_json::to_string(&update).unwrap()
                            )).await;
                        }
                    }
                    
                    Ok(WsClientMessage::GetMemoryStats { session_id: query_session }) => {
                        send_memory_stats(
                            sender.clone(),
                            &app_state,
                            &query_session.unwrap_or(session_id.clone()),
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
                        let _ = sender_guard.send(Message::Text(
                            serde_json::to_string(&error_msg).unwrap()
                        )).await;
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
    
    // Clean up heartbeat task
    heartbeat_handle.abort();
    eprintln!("WebSocket handler ended for session: {}", session_id);
}

async fn stream_chat_response(
    sender: Arc<tokio::sync::Mutex<futures::stream::SplitSink<WebSocket, Message>>>,
    app_state: &Arc<AppState>,
    session_id: &str,
    content: String,
    persona: &PersonaOverlay,
    current_mood: &mut String,
) {
    eprintln!("[{}] Starting chat response stream for: {}", Utc::now(), content);
    
    // Get embedding for semantic search
    let user_embedding = match timeout(
        Duration::from_secs(10),
        app_state.llm_client.get_embedding(&content)
    ).await {
        Ok(Ok(emb)) => Some(emb),
        Ok(Err(e)) => {
            eprintln!("[{}] Failed to get embedding: {}", Utc::now(), e);
            None
        }
        Err(_) => {
            eprintln!("[{}] Embedding timeout", Utc::now());
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

    // Get emotional weight for model routing with timeout
    eprintln!("[{}] Starting emotional weight classification", Utc::now());
    let emotional_weight = match timeout(
        Duration::from_secs(5),
        emotional_weight::classify(&app_state.llm_client, &content)
    ).await {
        Ok(Ok(val)) => {
            eprintln!("[{}] Emotional weight: {}", Utc::now(), val);
            val
        }
        Ok(Err(e)) => {
            eprintln!("[{}] Failed to classify emotional weight: {}", Utc::now(), e);
            0.0
        }
        Err(_) => {
            eprintln!("[{}] Emotional weight timeout - using default", Utc::now());
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

    eprintln!("[{}] Calling LLM with model: {}", Utc::now(), model);
    
    // Get the complete structured response first with timeout
    let mira_reply = match timeout(
        Duration::from_secs(30),
        app_state.llm_client.chat_with_custom_prompt(&content, model, &system_prompt)
    ).await {
        Ok(Ok(resp)) => resp,
        Ok(Err(e)) => {
            eprintln!("[{}] Failed to call OpenAI: {}", Utc::now(), e);
            let error_msg = WsServerMessage::Error {
                message: "Service temporarily unavailable".to_string(),
                code: Some("LLM_ERROR".to_string()),
            };
            let mut sender_guard = sender.lock().await;
            let _ = sender_guard.send(Message::Text(
                serde_json::to_string(&error_msg).unwrap()
            )).await;
            return;
        }
        Err(_) => {
            eprintln!("[{}] LLM call timeout", Utc::now());
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

    eprintln!("[{}] Got LLM response, streaming chunks", Utc::now());

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
        
        let mut sender_guard = sender.lock().await;
        if let Ok(msg_text) = serde_json::to_string(&ws_chunk) {
            if sender_guard.send(Message::Text(msg_text)).await.is_err() {
                eprintln!("Failed to send chunk, connection likely closed");
                return;
            }
            
            // Small delay to simulate typing
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    // Send done message
    let done = WsServerMessage::Done;
    if let Ok(msg_text) = serde_json::to_string(&done) {
        let mut sender_guard = sender.lock().await;
        let _ = sender_guard.send(Message::Text(msg_text)).await;
    }

    eprintln!("[{}] Finished streaming response", Utc::now());

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
    sender: Arc<tokio::sync::Mutex<futures::stream::SplitSink<WebSocket, Message>>>,
    _app_state: &Arc<AppState>,
    _session_id: &str,
) {
    // TODO: Implement actual memory stats
    let stats = WsServerMessage::MemoryStats {
        total_memories: 0,
        high_salience_count: 0,
        avg_salience: 0.0,
        mood_distribution: std::collections::HashMap::new(),
    };
    
    let mut sender_guard = sender.lock().await;
    if let Ok(msg_text) = serde_json::to_string(&stats) {
        let _ = sender_guard.send(Message::Text(msg_text)).await;
    }
}
