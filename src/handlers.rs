// src/handlers.rs

use axum::{
    extract::{Extension, Json},
    http::{HeaderMap, header::SET_COOKIE},
    response::{IntoResponse, Response},
};
use std::sync::Arc;
use serde::Deserialize;

use crate::memory::sqlite::store::SqliteMemoryStore;
use crate::memory::qdrant::store::QdrantMemoryStore;
use crate::memory::traits::MemoryStore;
use crate::memory::types::MemoryEntry;
use crate::memory::recall::{build_context, RecallContext};
use crate::llm::{OpenAIClient, EvaluateMemoryRequest, EvaluateMemoryResponse, function_schema};
use crate::llm::intent::{ChatIntent, chat_intent_function_schema};
use crate::persona::{PersonaOverlay};
use crate::prompt::builder::build_system_prompt;
use chrono::Utc;

#[derive(Deserialize)]
pub struct ChatRequest {
    pub message: String,
    pub persona_override: Option<String>,
}

#[derive(serde::Serialize)]
pub struct ChatReply {
    pub output: String,
    pub persona: String,
    pub mood: String,
    pub salience: u8,
    pub summary: Option<String>,
    pub memory_type: String,
    pub tags: Vec<String>,
}

pub struct AppState {
    pub sqlite_store: Arc<SqliteMemoryStore>,
    pub qdrant_store: Arc<QdrantMemoryStore>,
    pub llm_client: Arc<OpenAIClient>,
}

pub async fn chat_handler(
    Extension(state): Extension<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<ChatRequest>,
) -> Response {
    // --- 1. Session ID from cookie (or generate) ---
    let session_id = headers
        .get(axum::http::header::COOKIE)
        .and_then(|c| c.to_str().ok())
        .and_then(|cookie_str| {
            cookie_str.split(';').find_map(|pair| {
                let mut kv = pair.trim().splitn(2, '=');
                match (kv.next(), kv.next()) {
                    (Some(k), Some(v)) if k == "mira_session" => Some(v.to_string()),
                    _ => None,
                }
            })
        })
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    // --- 2. Get embedding for the user message (for semantic search) ---
    let user_embedding = match state.llm_client.get_embedding(&payload.message).await {
        Ok(emb) => Some(emb),
        Err(e) => {
            eprintln!("Failed to get embedding: {}", e);
            None
        }
    };

    // --- 3. Build recall context using BOTH memory stores ---
    let recall_context = build_context(
        &session_id,
        user_embedding.as_deref(),
        15,  // recent messages
        5,   // semantic matches
        state.sqlite_store.as_ref(),
        state.qdrant_store.as_ref(),
    )
    .await
    .unwrap_or_else(|_| RecallContext::new(vec![], vec![]));

    // --- 4. Determine persona overlay ---
    let persona_overlay = if let Some(ref override_str) = payload.persona_override {
        override_str.parse::<PersonaOverlay>().unwrap_or(PersonaOverlay::Default)
    } else {
        PersonaOverlay::Default
    };

    // --- 5. Build system prompt with persona and memory context ---
    let system_prompt = build_system_prompt(&persona_overlay, &recall_context);

    // --- 6. Build messages for GPT including context ---
    let mut gpt_messages = vec![
        serde_json::json!({
            "role": "system",
            "content": system_prompt
        })
    ];

    // Add recent history
    for entry in &recall_context.recent {
        gpt_messages.push(serde_json::json!({
            "role": entry.role.as_str(),
            "content": entry.content
        }));
    }

    // Add semantic memories as context if any
    if !recall_context.semantic.is_empty() {
        let semantic_context = recall_context.semantic.iter()
            .map(|m| format!("[Memory: {}]", m.content))
            .collect::<Vec<_>>()
            .join("\n");
        
        gpt_messages.push(serde_json::json!({
            "role": "system",
            "content": format!("Relevant memories from our past:\n{}", semantic_context)
        }));
    }

    // Add new user message
    gpt_messages.push(serde_json::json!({
        "role": "user",
        "content": &payload.message
    }));

    // --- 7. Call GPT-4.1 for actual chat completion with function calling ---
    let chat_completion_body = serde_json::json!({
        "model": "gpt-4.1",
        "messages": gpt_messages,
        "functions": [chat_intent_function_schema()],
        "function_call": { "name": "format_response" },
        "temperature": 0.9,
        "max_tokens": 500
    });

    let gpt_response = match state.llm_client.client
        .post(&format!("{}/chat/completions", state.llm_client.api_base))
        .header("Authorization", format!("Bearer {}", state.llm_client.api_key))
        .json(&chat_completion_body)
        .send()
        .await
    {
        Ok(resp) => resp,
        Err(e) => {
            eprintln!("Failed to call OpenAI: {}", e);
            let error_reply = ChatReply {
                output: "I'm having trouble connecting right now. Please try again.".to_string(),
                persona: persona_overlay.to_string(),
                mood: "apologetic".to_string(),
                salience: 1,
                summary: None,
                memory_type: "Other".to_string(),
                tags: vec![],
            };
            return Json(error_reply).into_response();
        }
    };

    if !gpt_response.status().is_success() {
        let error_text = gpt_response.text().await.unwrap_or_default();
        eprintln!("OpenAI API error: {}", error_text);
        let error_reply = ChatReply {
            output: "Something went wrong with my response. Let me try again.".to_string(),
            persona: persona_overlay.to_string(),
            mood: "concerned".to_string(),
            salience: 1,
            summary: None,
            memory_type: "Other".to_string(),
            tags: vec![],
        };
        return Json(error_reply).into_response();
    }

    let gpt_json: serde_json::Value = match gpt_response.json().await {
        Ok(json) => json,
        Err(e) => {
            eprintln!("Failed to parse GPT response: {}", e);
            let error_reply = ChatReply {
                output: "I couldn't parse my own thoughts. That's... concerning.".to_string(),
                persona: persona_overlay.to_string(),
                mood: "confused".to_string(),
                salience: 1,
                summary: None,
                memory_type: "Other".to_string(),
                tags: vec![],
            };
            return Json(error_reply).into_response();
        }
    };

    let chat_intent = ChatIntent::from_function_result(&gpt_json);

    // --- 8. Moderate the messages ---
    let user_moderation_flag = match state.llm_client.moderate_message(&payload.message).await {
        Ok(flag) => flag,
        Err(_) => false,
    };

    let mira_moderation_flag = match state.llm_client.moderate_message(&chat_intent.output).await {
        Ok(flag) => flag,
        Err(_) => false,
    };

    // --- 9. Evaluate user message for memory metadata ---
    let eval_req = EvaluateMemoryRequest {
        content: payload.message.clone(),
        function_schema: function_schema(),
    };

    let eval: Option<EvaluateMemoryResponse> = match state.llm_client.evaluate_memory(&eval_req).await {
        Ok(val) => Some(val),
        Err(e) => {
            eprintln!("Failed to evaluate memory: {}", e);
            None
        }
    };

    // --- 10. Save user message to both stores ---
    let now = Utc::now();
    let memory_type_converted = eval.as_ref().map(|e| match e.memory_type {
        crate::llm::schema::MemoryType::Feeling => crate::memory::types::MemoryType::Feeling,
        crate::llm::schema::MemoryType::Fact => crate::memory::types::MemoryType::Fact,
        crate::llm::schema::MemoryType::Joke => crate::memory::types::MemoryType::Joke,
        crate::llm::schema::MemoryType::Promise => crate::memory::types::MemoryType::Promise,
        crate::llm::schema::MemoryType::Event => crate::memory::types::MemoryType::Event,
        _ => crate::memory::types::MemoryType::Other,
    });

    let user_entry = MemoryEntry {
        id: None,
        session_id: session_id.clone(),
        role: "user".to_string(),
        content: payload.message.clone(),
        timestamp: now,
        embedding: user_embedding.clone(),
        salience: eval.as_ref().map(|e| e.salience as f32),
        tags: eval.as_ref().map(|e| e.tags.clone()),
        summary: eval.as_ref().and_then(|e| e.summary.clone()),
        memory_type: memory_type_converted.clone(),
        logprobs: None,
        moderation_flag: Some(user_moderation_flag),
        system_fingerprint: Some(gpt_json["system_fingerprint"].as_str().unwrap_or("").to_string()),
    };

    // Save to SQLite
    let _ = state.sqlite_store.save(&user_entry).await;

    // Save to Qdrant if we have embeddings and meaningful salience
    if user_embedding.is_some() && eval.as_ref().map(|e| e.salience >= 3).unwrap_or(false) {
        let _ = state.qdrant_store.save(&user_entry).await;
    }

    // --- 11. Get embedding for Mira's response ---
    let mira_embedding = match state.llm_client.get_embedding(&chat_intent.output).await {
        Ok(emb) => Some(emb),
        Err(_) => None,
    };

    // --- 12. Save Mira's reply ---
    let mira_entry = MemoryEntry {
        id: None,
        session_id: session_id.clone(),
        role: "assistant".to_string(),
        content: chat_intent.output.clone(),
        timestamp: Utc::now(),
        embedding: mira_embedding.clone(),
        salience: Some(5.0), // Default salience for Mira's responses
        tags: Some(vec![chat_intent.mood.clone(), chat_intent.persona.clone()]),
        summary: None,
        memory_type: Some(crate::memory::types::MemoryType::Other),
        logprobs: None,
        moderation_flag: Some(mira_moderation_flag),
        system_fingerprint: Some(gpt_json["system_fingerprint"].as_str().unwrap_or("").to_string()),
    };

    // Save to SQLite
    let _ = state.sqlite_store.save(&mira_entry).await;

    // Save to Qdrant if we have embeddings
    if mira_embedding.is_some() {
        let _ = state.qdrant_store.save(&mira_entry).await;
    }

    // --- 13. Build response ---
    let reply = ChatReply {
        output: chat_intent.output,
        persona: chat_intent.persona,
        mood: chat_intent.mood,
        salience: eval.as_ref().map(|e| e.salience).unwrap_or(5),
        summary: eval.as_ref().and_then(|e| e.summary.clone()),
        memory_type: eval.as_ref().map(|e| format!("{:?}", e.memory_type)).unwrap_or_else(|| "Other".to_string()),
        tags: eval.as_ref().map(|e| e.tags.clone()).unwrap_or_default(),
    };

    let mut response = Json(reply).into_response();
    *response.status_mut() = axum::http::StatusCode::OK;

    // Set session cookie
    response.headers_mut().insert(
        SET_COOKIE,
        format!("mira_session={}; Path=/; HttpOnly; SameSite=Lax", session_id)
            .parse()
            .unwrap(),
    );

    response
}
