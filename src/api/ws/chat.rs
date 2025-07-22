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
use crate::memory::recall::{RecallContext, build_context};
use crate::memory::traits::MemoryStore;
use crate::memory::types::MemoryEntry;
use crate::prompt;
use chrono::Utc;

pub async fn ws_chat_handler(
    ws: WebSocketUpgrade,
    State(app_state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_ws(socket, app_state))
}

async fn handle_ws(socket: WebSocket, app_state: Arc<AppState>) {
    let (mut sender, mut receiver) = socket.split();
    
    // Generate or extract session ID
    let session_id = uuid::Uuid::new_v4().to_string();
    let mut current_persona = PersonaOverlay::Default;
    let mut current_mood = "present".to_string();
    
    while let Some(Ok(msg)) = receiver.next().await {
        match msg {
            Message::Text(text) => {
                let incoming: Result<WsClientMessage, _> = serde_json::from_str(&text);
                match incoming {
                    Ok(WsClientMessage::Message { content, persona }) => {
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
                    
                    _ => {}
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }
}

async fn stream_chat_response(
    sender: &mut futures::stream::SplitSink<WebSocket, Message>,
    app_state: &Arc<AppState>,
    session_id: &str,
    content: String,
    persona: &PersonaOverlay,
    current_mood: &mut String,
) {
    // Get embedding for semantic search
    let user_embedding = app_state.llm_client.get_embedding(&content).await.ok();

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
    .unwrap_or_else(|_| RecallContext::new(vec![], vec![]));

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

    while let Some(server_msg) = stream.next().await {
        // Update mood if detected
        if let WsServerMessage::Chunk { mood: Some(m), .. } = &server_msg {
            *current_mood = m.clone();
        }
        
        let msg_text = serde_json::to_string(&server_msg).unwrap();
        let _ = sender.send(Message::Text(msg_text)).await;
    }

    // Save interaction to memory
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
        let _ = app_state.sqlite_store.save(&entry).await;
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
            }
        }
        prompt.push_str("\n");
    }
    
    // Streaming instructions
    prompt.push_str("You're in a live conversation. Your responses should:\n");
    prompt.push_str("- Include emotional asides when feelings surge (mark with 'emotional_cue')\n");
    prompt.push_str("- Flow naturally, never mechanical\n");
    prompt.push_str("- Let mood shifts happen organically\n");
    prompt.push_str("- Use 'output' for your words, 'mood' for your emotional state\n");
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
