use axum::{
    extract::{Extension, Json, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use std::sync::Arc;
use serde::{Deserialize, Serialize};

use crate::memory::sqlite::store::SqliteMemoryStore;
use crate::memory::qdrant::store::QdrantMemoryStore;
use crate::memory::traits::MemoryStore;
use crate::memory::types::MemoryEntry;
use crate::memory::recall::{build_context, RecallContext};
use crate::llm::{OpenAIClient, EvaluateMemoryRequest, EvaluateMemoryResponse, MiraStructuredReply, function_schema, emotional_weight};
use crate::persona::{PersonaOverlay};
use crate::prompt::builder::build_system_prompt;
use chrono::Utc;

#[derive(Deserialize)]
pub struct ChatRequest {
    pub message: String,
    pub persona_override: Option<String>,
}

#[derive(Serialize)]
pub struct ChatReply {
    pub output: String,
    pub persona: String,
    pub mood: String,
    pub salience: u8,
    pub summary: Option<String>,
    pub memory_type: String,
    pub tags: Vec<String>,
    pub intent: String,
    pub monologue: Option<String>,
    pub reasoning_summary: Option<String>,
}

#[derive(Serialize)]
pub struct ChatHistoryResponse {
    pub messages: Vec<MemoryEntry>,
    pub session_id: String,
}

#[derive(Deserialize)]
pub struct HistoryQuery {
    pub limit: Option<usize>,
    pub offset: Option<i64>,
}

pub struct AppState {
    pub sqlite_store: Arc<SqliteMemoryStore>,
    pub qdrant_store: Arc<QdrantMemoryStore>,
    pub llm_client: Arc<OpenAIClient>,
}

pub async fn chat_handler(
    Extension(state): Extension<Arc<AppState>>,
    _headers: HeaderMap,
    Json(payload): Json<ChatRequest>,
) -> Response {
    let session_id = "peter-eternal".to_string();
    eprintln!("Using eternal session: {}", session_id);

    // --- 1. Get embedding for the user message (for semantic search) ---
    let user_embedding = match state.llm_client.get_embedding(&payload.message).await {
        Ok(emb) => Some(emb),
        Err(e) => {
            eprintln!("Failed to get embedding: {}", e);
            None
        }
    };

    // --- 2. Build recall context using BOTH memory stores ---
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

    // --- 3. Determine persona overlay ---
    let persona_overlay = if let Some(ref override_str) = payload.persona_override {
        override_str.parse::<PersonaOverlay>().unwrap_or(PersonaOverlay::Default)
    } else {
        PersonaOverlay::Default
    };

    // --- 4. Build system prompt with persona and memory context ---
    let system_prompt = build_system_prompt(&persona_overlay, &recall_context);

    // --- 5. Moderate user message (log-only) ---
    let _ = state.llm_client.moderate(&payload.message).await;

    // --- 6. Get emotional weight for auto model routing ---
    let emotional_weight = match emotional_weight::classify(&state.llm_client, &payload.message).await {
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

    // --- 7. Call LLM with chosen model and persona-aware system prompt ---
    let mira_reply = match state.llm_client.chat_with_custom_prompt(&payload.message, model, &system_prompt).await {
        Ok(resp) => resp,
        Err(e) => {
            eprintln!("Failed to call OpenAI: {}", e);
            return Response::builder()
                .status(StatusCode::SERVICE_UNAVAILABLE)
                .body(axum::body::Body::from("Service temporarily unavailable"))
                .unwrap();
        }
    };

    // --- 8. Evaluate user message for memory metadata (unchanged) ---
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

    // --- 9. Save user message to both stores ---
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
        moderation_flag: None, // log-only moderation!
        system_fingerprint: None,
    };

    // Save to SQLite
    let _ = state.sqlite_store.save(&user_entry).await;

    // Save to Qdrant if we have embeddings and meaningful salience
    if user_embedding.is_some() && eval.as_ref().map(|e| e.salience >= 3).unwrap_or(false) {
        let _ = state.qdrant_store.save(&user_entry).await;
    }

    // --- 10. Get embedding for Mira's response ---
    let mira_embedding = match state.llm_client.get_embedding(&mira_reply.output).await {
        Ok(emb) => Some(emb),
        Err(_) => None,
    };

    // --- 11. Save Mira's reply ---
    let mira_entry = MemoryEntry {
        id: None,
        session_id: session_id.clone(),
        role: "assistant".to_string(),
        content: mira_reply.output.clone(),
        timestamp: Utc::now(),
        embedding: mira_embedding.clone(),
        salience: Some(mira_reply.salience as f32),
        tags: Some(mira_reply.tags.clone()),
        summary: mira_reply.summary.clone(),
        memory_type: Some(crate::memory::types::MemoryType::Other),
        logprobs: None,
        moderation_flag: None, // log-only
        system_fingerprint: None,
    };

    let _ = state.sqlite_store.save(&mira_entry).await;
    if mira_embedding.is_some() {
        let _ = state.qdrant_store.save(&mira_entry).await;
    }

    // --- 12. Build API response from structured output ---
    let reply = ChatReply {
        output: mira_reply.output,
        persona: persona_overlay.to_string(),   // <<< FIXED: always return overlay, not LLM output
        mood: mira_reply.mood,
        salience: mira_reply.salience,
        summary: mira_reply.summary,
        memory_type: mira_reply.memory_type,
        tags: mira_reply.tags,
        intent: mira_reply.intent,
        monologue: mira_reply.monologue,
        reasoning_summary: mira_reply.reasoning_summary,
    };

    Json(reply).into_response()
}

pub async fn chat_history_handler(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HistoryQuery>,
) -> impl IntoResponse {
    let session_id = "peter-eternal".to_string();
    let limit = params.limit.unwrap_or(30);

    match state.sqlite_store.load_recent(&session_id, limit).await {
        Ok(messages) => {
            let response = ChatHistoryResponse {
                messages,
                session_id,
            };
            Json(response).into_response()
        }
        Err(e) => {
            eprintln!("Failed to load chat history: {}", e);
            Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(axum::body::Body::from("Failed to load chat history"))
                .unwrap()
                .into_response()
        }
    }
}
