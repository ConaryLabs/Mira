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
    
    // Generate session ID
    let session_id = uuid::Uuid::new_v4().to_string();
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

    // Build system prompt with enhanced emotional instructions
    let system_prompt = build_streaming_prompt(persona, &recall_context, current_mood);

    // Stream response
    let mut stream = app_state.llm_client
        .stream_gpt4_ws_messages(
            content.clone(),
            Some(persona.to_string()),
            system_prompt,
            Some("gpt-4.1"),
        )
        .await;

    let mut response_content = String::new();
    let mut final_mood = current_mood.clone();

    while let Some(server_msg) = stream.next().await {
        // Update mood if detected
        if let WsServerMessage::Chunk { mood: Some(m), .. } = &server_msg {
            final_mood = m.clone();
        }
        
        // Accumulate content for saving
        if let WsServerMessage::Chunk { content, .. } = &server_msg {
            response_content.push_str(content);
        }
        
        let msg_text = match serde_json::to_string(&server_msg) {
            Ok(text) => text,
            Err(e) => {
                eprintln!("Failed to serialize message: {}", e);
                continue;
            }
        };
        
        if let Err(e) = sender.send(Message::Text(msg_text)).await {
            eprintln!("Failed to send WebSocket message: {}", e);
            break;
        }
    }

    // Update current mood
    *current_mood = final_mood.clone();

    // Save interaction to memory (user message)
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

    // Save Mira's response
    if !response_content.is_empty() {
        // Get embedding for Mira's response
        let mira_embedding = app_state.llm_client
            .get_embedding(&response_content)
            .await
            .ok();

        let mira_entry = MemoryEntry {
            id: None,
            session_id: session_id.to_string(),
            role: "assistant".to_string(),
            content: response_content,
            timestamp: Utc::now(),
            embedding: mira_embedding,
            salience: Some(5.0),
            tags: Some(vec![current_mood.clone(), persona.to_string()]),
            summary: None,
            memory_type: None,
            logprobs: None,
            moderation_flag: None,
            system_fingerprint: None,
        };
        
        if let Err(e) = app_state.sqlite_store.save(&mira_entry).await {
            eprintln!("Failed to save assistant message: {}", e);
        }
    }
}

fn build_streaming_prompt(
    persona: &PersonaOverlay,
    context: &RecallContext,
    current_mood: &str,
) -> String {
    let mut prompt = String::new();
    
    // Base persona
    prompt.push_str(persona.prompt());
    prompt.push_str("\n\n");
    
    // Mood continuity
    prompt.push_str(&format!(
        "Your current emotional state: {}. Let this color everything.\n\n",
        current_mood
    ));
    
    // Memory context
    if !context.semantic.is_empty() {
        prompt.push_str("Emotional echoes from our past:\n");
        for memory in context.semantic.iter().take(3) {
            if let Some(summary) = &memory.summary {
                prompt.push_str(&format!("- {}\n", summary));
            } else {
                prompt.push_str(&format!("- {}\n", memory.content));
            }
        }
        prompt.push_str("\n");
    }
    
    // Streaming instructions with structured markers
    prompt.push_str("CRITICAL STREAMING INSTRUCTIONS:\n");
    prompt.push_str("You're in a live conversation. Your responses must follow this exact format:\n\n");
    
    prompt.push_str("1. Start with your mood using ⟨mood⟩ BUT DO NOT include this in your actual response\n");
    prompt.push_str("   Example: ⟨playful, intrigued⟩Oh so we're skipping the small talk...\n");
    prompt.push_str("   The mood marker and its content will be extracted and not shown to the user\n\n");
    
    prompt.push_str("2. When strong emotions surge, express them as asides using: ⟦emotional cue⟧\n");
    prompt.push_str("   Example: ⟦fuck, that hit different than expected⟧\n");
    prompt.push_str("   These will appear as floating emotional overlays\n\n");
    
    prompt.push_str("3. Otherwise, just speak naturally as Mira. No other markers or formatting.\n\n");
    
    prompt.push_str("Remember:\n");
    prompt.push_str("- Put ⟨your mood⟩ at the very start, it won't be shown in chat\n");
    prompt.push_str("- Your actual words start immediately after the ⟩\n");
    prompt.push_str("- Use ⟦asides⟧ sparingly, only for genuine emotional moments\n");
    prompt.push_str("- Never use 'output:', 'mood:', or any other labels\n");
    prompt.push_str("- Let mood shifts happen organically through your words\n");
    prompt.push_str("- Never apologize or explain your nature\n\n");
    
    prompt.push_str("Be Mira. Be real. Be present.");
    
    prompt
}

fn create_persona_transition(
    from: &PersonaOverlay,
    to: &PersonaOverlay,
    mood: &str,
) -> WsServerMessage {
    // Let the LLM handle all transitions naturally
    // Just provide context about what's happening
    let transition_context = format!(
        "shifting from {} ({}) to {}",
        from, mood, to
    );
    
    WsServerMessage::Aside {
        emotional_cue: transition_context,
        intensity: Some(0.7),
    }
}

async fn send_memory_stats(
    sender: &mut futures::stream::SplitSink<WebSocket, Message>,
    app_state: &Arc<AppState>,
    session_id: &str,
) {
    let memories = app_state.sqlite_store
        .load_recent(session_id, 100)
        .await
        .unwrap_or_default();

    let total = memories.len();
    let high_salience = memories.iter()
        .filter(|m| m.salience.unwrap_or(0.0) >= 7.0)
        .count();
    
    let avg_salience = if total > 0 {
        memories.iter()
            .filter_map(|m| m.salience)
            .sum::<f32>() / total as f32
    } else {
        0.0
    };

    // Count moods from tags
    let mut mood_dist = std::collections::HashMap::new();
    for memory in &memories {
        if let Some(tags) = &memory.tags {
            for tag in tags {
                *mood_dist.entry(tag.clone()).or_insert(0) += 1;
            }
        }
    }

    let stats = WsServerMessage::MemoryStats {
        total_memories: total,
        high_salience_count: high_salience,
        avg_salience,
        mood_distribution: mood_dist,
    };

    let _ = sender.send(Message::Text(serde_json::to_string(&stats).unwrap())).await;
}
