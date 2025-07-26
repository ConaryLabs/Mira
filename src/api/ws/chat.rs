// src/api/ws/chat.rs

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::IntoResponse;
use futures::{SinkExt, StreamExt};
use std::sync::Arc;
use tokio::time::{interval, timeout, Duration};
use tokio::sync::Mutex;

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
    let sender = Arc::new(Mutex::new(sender));
    
    // Use peter-eternal for consistency with REST endpoint
    let session_id = "peter-eternal".to_string();
    let current_persona = PersonaOverlay::Default;  // Fixed: removed mut
    let mut current_mood = "present".to_string();
    let mut active_project_id: Option<String> = None;
    
    eprintln!("WebSocket connection established. Session: {}", session_id);
    
    // Create a channel for heartbeat cancellation
    let (heartbeat_shutdown_tx, mut heartbeat_shutdown_rx) = tokio::sync::mpsc::channel::<()>(1);
    
    // Spawn a heartbeat task with proper shutdown handling
    let heartbeat_sender = sender.clone();
    let heartbeat_handle = tokio::spawn(async move {
        let mut interval = interval(Duration::from_secs(30));
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    // Check if we can still send before attempting
                    let mut sender_guard = heartbeat_sender.lock().await;
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
    _project_id: Option<&str>,  // TODO: Use for project context injection in Sprint 3
) {
    eprintln!("[{}] Starting chat response stream for: {}", Utc::now(), content);
    eprintln!("Using persona internally: {}", persona);
    
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
    
    // Use ORIGINAL model selection logic
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
    if let Some(monologue) = &mira_reply.monologue {
        if !monologue.is_empty() {
            let aside_msg = WsServerMessage::Aside {
                emotional_cue: monologue.clone(),
                intensity: Some(mira_reply.salience as f32 / 10.0),
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
        embedding: mira_embedding.clone(),
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
    
    // Save to SQLite
    if let Err(e) = app_state.sqlite_store.save(&mira_entry).await {
        eprintln!("Failed to save assistant message to SQLite: {}", e);
    }
    
    // Save to Qdrant if embedding exists
    if mira_embedding.is_some() {
        if let Err(e) = app_state.qdrant_store.save(&mira_entry).await {
            eprintln!("Failed to save assistant message to Qdrant: {}", e);
        }
    }
}
