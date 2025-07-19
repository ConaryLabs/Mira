// src/handlers.rs

use axum::{
    extract::{Extension, Json},
    http::{HeaderMap, header::SET_COOKIE},
    response::{IntoResponse, Response},
};
use std::sync::Arc;
use serde::Deserialize;

use crate::memory::sqlite::store::SqliteMemoryStore;
use crate::memory::traits::MemoryStore;
use crate::memory::types::MemoryEntry;
use crate::llm::{OpenAIClient, EvaluateMemoryRequest, EvaluateMemoryResponse, function_schema};
use crate::persona::{PersonaOverlay};
use chrono::Utc;

#[derive(Deserialize)]
pub struct ChatRequest {
    pub message: String,
    // (Optional: add persona_override, etc.)
}

#[derive(serde::Serialize)]
pub struct ChatReply {
    pub output: String,
    pub persona: String,
    pub salience: u8,
    pub summary: Option<String>,
    pub memory_type: String,
    pub tags: Vec<String>,
}

pub async fn chat_handler(
    Extension(memory_store): Extension<Arc<SqliteMemoryStore>>,
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

    // --- 2. Load recent memory (chat history) for this session ---
    let history = memory_store
        .load_recent(&session_id, 15)
        .await
        .unwrap_or_else(|_| vec![]);

    // --- 3. Build recall context (system prompt with persona, recency, etc.) ---
    // Persona overlay logic (could be dynamic later)
    let persona_overlay = PersonaOverlay::Default;
    let system_prompt = persona_overlay.prompt();

    // Build the message for the LLM
    let mut gpt_messages = history
        .iter()
        .map(|entry| {
            serde_json::json!({
                "role": entry.role.as_str(),
                "content": entry.content
            })
        })
        .collect::<Vec<_>>();

    // Add new user message
    gpt_messages.push(serde_json::json!({
        "role": "user",
        "content": &payload.message
    }));

    // --- 4. Call OpenAI Moderation endpoint ---
    let llm_client = OpenAIClient::new();
    let moderation_flag = match llm_client.moderate_message(&payload.message).await {
        Ok(flag) => flag,
        Err(_) => false,
    };

    // --- 5. Get embedding ---
    let embedding = match llm_client.get_embedding(&payload.message).await {
        Ok(emb) => Some(emb),
        Err(_) => None,
    };

    // --- 6. Run GPT-4.1 function-calling for metadata extraction ---
    let req = EvaluateMemoryRequest {
        content: payload.message.clone(),
        function_schema: function_schema(),
    };

    let eval: Option<EvaluateMemoryResponse> = match llm_client.evaluate_memory(&req).await {
        Ok(val) => Some(val),
        Err(_) => None,
    };

    // --- 7. Generate the LLM output as a "reply" ---
    // (Stub: In real usage, you'd pass the context to a GPT completion endpoint, using system prompt + messages)
    let output = format!(
        "Mira says: [persona: {}] — {}",
        persona_overlay.to_string(),
        payload.message
    );

    // --- 8. Save user and assistant messages to SQLite ---
    // Save user message
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
        embedding: embedding.clone(),
        salience: eval.as_ref().map(|e| e.salience as f32),
        tags: eval.as_ref().map(|e| e.tags.clone()),
        summary: eval.as_ref().and_then(|e| e.summary.clone()),
        memory_type: memory_type_converted,
        logprobs: None,
        moderation_flag: Some(moderation_flag),
        system_fingerprint: None,
    };
    let _ = memory_store.save(&user_entry).await;

    // Save Mira's reply
    let mira_entry = MemoryEntry {
        id: None,
        session_id: session_id.clone(),
        role: "mira".to_string(),
        content: output.clone(),
        timestamp: Utc::now(),
        embedding: None,
        salience: None,
        tags: None,
        summary: None,
        memory_type: None,
        logprobs: None,
        moderation_flag: None,
        system_fingerprint: None,
    };
    let _ = memory_store.save(&mira_entry).await;

    // --- 9. Build response ---
    let reply = ChatReply {
        output: output.clone(),
        persona: persona_overlay.to_string(),
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
