// src/handlers.rs
use axum::{
    extract::{Json, Query, State},
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
use crate::llm::{OpenAIClient, EvaluateMemoryRequest, EvaluateMemoryResponse, emotional_weight, function_schema};
use crate::persona::{PersonaOverlay};
use crate::prompt::builder::build_system_prompt;
use crate::project::store::ProjectStore;
use chrono::{Utc, TimeZone};
use sqlx::Row;

#[derive(Deserialize)]
pub struct ChatRequest {
    pub message: String,
    pub persona_override: Option<String>,
    pub project_id: Option<String>, // NEW: Optional project context
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
    pub project_id: Option<String>, // NEW: Filter by project
}

#[derive(Clone)]
pub struct AppState {
    pub sqlite_store: Arc<SqliteMemoryStore>,
    pub qdrant_store: Arc<QdrantMemoryStore>,
    pub llm_client: Arc<OpenAIClient>,
    pub project_store: Arc<ProjectStore>, // NEW: Add project store
}

pub async fn chat_handler(
    State(state): State<Arc<AppState>>,
    _headers: HeaderMap,
    Json(payload): Json<ChatRequest>,
) -> Response {
    let session_id = "peter-eternal".to_string();
    eprintln!("Using eternal session: {}", session_id);

    // --- 1. Get embedding for the user message (for semantic search) ---
    eprintln!("📊 Getting embedding for user message...");
    let user_embedding = match state.llm_client.get_embedding(&payload.message).await {
        Ok(emb) => {
            eprintln!("✅ Embedding generated successfully (length: {})", emb.len());
            Some(emb)
        },
        Err(e) => {
            eprintln!("❌ Failed to get embedding: {:?}", e);
            None
        }
    };

    // --- 2. Build recall context using BOTH memory stores ---
    let recall_context = build_context(
        &session_id,
        user_embedding.as_deref(),
        30,  // INCREASED - recent messages
        15,  // INCREASED - semantic matches
        state.sqlite_store.as_ref(),
        state.qdrant_store.as_ref(),
    )
    .await
    .unwrap_or_else(|e| {
        eprintln!("⚠️ Failed to build recall context: {:?}", e);
        RecallContext::new(vec![], vec![])
    });

    eprintln!("📚 Recall context: {} recent, {} semantic", 
        recall_context.recent.len(), 
        recall_context.semantic.len()
    );
    
    // Log the recent messages to see what's being loaded
    eprintln!("📜 Recent messages in context:");
    for (i, msg) in recall_context.recent.iter().enumerate() {
        eprintln!("  {}. [{}] {} - {}", 
            i+1, 
            msg.role, 
            msg.timestamp.format("%H:%M:%S"),
            msg.content.chars().take(80).collect::<String>()
        );
    }
    
    // Also log semantic matches if any
    if !recall_context.semantic.is_empty() {
        eprintln!("🔍 Semantic matches:");
        for (i, msg) in recall_context.semantic.iter().take(5).enumerate() {
            eprintln!("  {}. [salience: {}] {}", 
                i+1, 
                msg.salience.unwrap_or(0.0),
                msg.content.chars().take(80).collect::<String>()
            );
        }
    }

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
        Ok(val) => {
            eprintln!("🎭 Emotional weight: {}", val);
            val
        },
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

    eprintln!("🤖 Using model: {}", model);

    // --- 7. Call LLM with chosen model and persona-aware system prompt ---
    let mira_reply = match state.llm_client.chat_with_custom_prompt(&payload.message, model, &system_prompt).await {
        Ok(resp) => resp,
        Err(e) => {
            eprintln!("Failed to call OpenAI: {}", e);
            return Response::builder()
                .status(StatusCode::SERVICE_UNAVAILABLE)
                .body(axum::body::Body::from("Service temporarily unavailable"))
                .unwrap()
                .into_response();
        }
    };

    // --- 8. Save user message to both stores ---
    eprintln!("💾 Saving user message to memory stores...");
    let user_entry = MemoryEntry {
        id: None,
        session_id: session_id.clone(),
        role: "user".to_string(),
        content: payload.message.clone(),
        timestamp: Utc::now(),
        embedding: user_embedding.clone(),
        salience: None,
        tags: None,
        summary: None,
        memory_type: None,
        logprobs: None,
        moderation_flag: None,
        system_fingerprint: None,
    };

    // Save to SQLite (always)
    match state.sqlite_store.save(&user_entry).await {
        Ok(_) => eprintln!("✅ User message saved to SQLite"),
        Err(e) => eprintln!("❌ FAILED to save user message to SQLite: {:?}", e),
    }

    // --- 9. Evaluate Mira's reply for metadata (salience, summary, etc.) ---
    eprintln!("📝 Evaluating Mira's reply...");
    let eval_request = EvaluateMemoryRequest {
        content: format!(
            "User: {}\nMira: {}",
            &payload.message, &mira_reply.output
        ),
        function_schema: function_schema(), // Use the default schema function
    };

    let eval_response = match state.llm_client.evaluate_memory(&eval_request).await {
        Ok(resp) => {
            eprintln!("✅ Memory evaluation complete - salience: {}, memory_type: {:?}", 
                resp.salience, resp.memory_type
            );
            resp
        }
        Err(e) => {
            eprintln!("⚠️ Memory evaluation failed: {:?}", e);
            EvaluateMemoryResponse {
                salience: 0,
                summary: None,
                memory_type: crate::llm::schema::MemoryType::Other,
                tags: vec![],
            }
        }
    };

    // --- 10. Get embedding for Mira's reply (if salience >= 7) ---
    let mira_embedding = if eval_response.salience >= 7 {
        eprintln!("🌟 High salience response ({}), generating embedding...", eval_response.salience);
        match state.llm_client.get_embedding(&mira_reply.output).await {
            Ok(emb) => {
                eprintln!("✅ Embedding generated for Mira's response");
                Some(emb)
            }
            Err(e) => {
                eprintln!("❌ Failed to get embedding for Mira's response: {:?}", e);
                None
            }
        }
    } else {
        eprintln!("💭 Low salience response ({}), skipping embedding", eval_response.salience);
        None
    };

    // --- 11. Save Mira's reply to memory stores ---
    let memory_type_for_db = match eval_response.memory_type {
        crate::llm::schema::MemoryType::Feeling => crate::memory::types::MemoryType::Feeling,
        crate::llm::schema::MemoryType::Fact => crate::memory::types::MemoryType::Fact,
        crate::llm::schema::MemoryType::Joke => crate::memory::types::MemoryType::Joke,
        crate::llm::schema::MemoryType::Promise => crate::memory::types::MemoryType::Promise,
        crate::llm::schema::MemoryType::Event => crate::memory::types::MemoryType::Event,
        crate::llm::schema::MemoryType::Other => crate::memory::types::MemoryType::Other,
    };

    let mira_entry = MemoryEntry {
        id: None,
        session_id: session_id.clone(),
        role: "assistant".to_string(),
        content: mira_reply.output.clone(),
        timestamp: Utc::now(),
        embedding: mira_embedding.clone(),
        salience: Some(eval_response.salience as f32),
        tags: Some(eval_response.tags.clone()),
        summary: eval_response.summary.clone(),
        memory_type: Some(memory_type_for_db),
        logprobs: None,
        moderation_flag: None,
        system_fingerprint: None,
    };

    // Always save to SQLite
    eprintln!("💾 Saving Mira's response to SQLite...");
    match state.sqlite_store.save(&mira_entry).await {
        Ok(_) => eprintln!("✅ Mira's response saved to SQLite"),
        Err(e) => eprintln!("❌ FAILED to save Mira's response to SQLite: {:?}", e),
    }
    
    if mira_embedding.is_some() {
        eprintln!("🔍 Saving Mira's response to Qdrant...");
        match state.qdrant_store.save(&mira_entry).await {
            Ok(_) => eprintln!("✅ Mira's response saved to Qdrant"),
            Err(e) => eprintln!("❌ FAILED to save Mira's response to Qdrant: {:?}", e),
        }
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

    eprintln!("🎉 Chat handler complete, returning response");
    Json(reply).into_response()
}

pub async fn chat_history_handler(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HistoryQuery>,
) -> impl IntoResponse {
    let session_id = "peter-eternal".to_string();
    let limit = params.limit.unwrap_or(30);
    let offset = params.offset.unwrap_or(0);

    eprintln!("📜 Loading chat history: session={}, limit={}, offset={}", 
        session_id, limit, offset
    );

    // Clone project_id to avoid move issues
    let project_id_filter = params.project_id.clone();

    // Build query with optional project filter
    let query = if project_id_filter.is_some() {
        r#"
        SELECT id, session_id, role, content, timestamp, embedding, salience, tags, 
               summary, memory_type, logprobs, moderation_flag, system_fingerprint
        FROM chat_history
        WHERE session_id = ? AND project_id = ?
        ORDER BY timestamp DESC
        LIMIT ? OFFSET ?
        "#
    } else {
        r#"
        SELECT id, session_id, role, content, timestamp, embedding, salience, tags, 
               summary, memory_type, logprobs, moderation_flag, system_fingerprint
        FROM chat_history
        WHERE session_id = ?
        ORDER BY timestamp DESC
        LIMIT ? OFFSET ?
        "#
    };
    
    let rows = if let Some(ref project_id) = params.project_id {
        sqlx::query(query)
            .bind(&session_id)
            .bind(project_id)
            .bind(limit as i64)
            .bind(offset as i64)
            .fetch_all(&state.sqlite_store.pool)
            .await
    } else {
        sqlx::query(query)
            .bind(&session_id)
            .bind(limit as i64)
            .bind(offset as i64)
            .fetch_all(&state.sqlite_store.pool)
            .await
    };

    match rows {
        Ok(rows) => {
            eprintln!("✅ Found {} messages in history", rows.len());
            let mut messages = Vec::new();
            
            for row in rows {
                let id: i64 = row.get("id");
                let session_id: String = row.get("session_id");
                let role: String = row.get("role");
                let content: String = row.get("content");
                let timestamp: chrono::NaiveDateTime = row.get("timestamp");
                let salience: Option<f32> = row.get("salience");
                let tags: Option<String> = row.get("tags");
                let summary: Option<String> = row.get("summary");
                let memory_type: Option<String> = row.get("memory_type");
                let moderation_flag: Option<bool> = row.get("moderation_flag");
                let system_fingerprint: Option<String> = row.get("system_fingerprint");

                // Deserialize tags
                let tags_vec = tags
                    .as_ref()
                    .and_then(|s| serde_json::from_str::<Vec<String>>(s).ok());

                let memory_type_enum = memory_type.as_ref().and_then(|mt| match mt.as_str() {
                    "Feeling" => Some(crate::memory::types::MemoryType::Feeling),
                    "Fact" => Some(crate::memory::types::MemoryType::Fact),
                    "Joke" => Some(crate::memory::types::MemoryType::Joke),
                    "Promise" => Some(crate::memory::types::MemoryType::Promise),
                    "Event" => Some(crate::memory::types::MemoryType::Event),
                    _ => Some(crate::memory::types::MemoryType::Other),
                });

                messages.push(MemoryEntry {
                    id: Some(id),
                    session_id,
                    role,
                    content,
                    timestamp: Utc.from_utc_datetime(&timestamp),
                    embedding: None, // Skip embedding for API response
                    salience,
                    tags: tags_vec,
                    summary,
                    memory_type: memory_type_enum,
                    logprobs: None,
                    moderation_flag,
                    system_fingerprint,
                });
            }
            
            // DON'T reverse - keep them in DESC order (newest first) for the frontend
            let response = ChatHistoryResponse {
                messages,
                session_id,
            };
            
            Json(response).into_response()
        }
        Err(e) => {
            eprintln!("❌ Failed to load chat history: {:?}", e);
            Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(axum::body::Body::from("Failed to load chat history"))
                .unwrap()
                .into_response()
        }
    }
}
