// src/handlers.rs
// Phase 7: Unified REST handler using ChatService

use axum::{
    extract::{Json, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{info, error};

use crate::memory::types::MemoryEntry;
use crate::state::AppState;

#[derive(Deserialize)]
pub struct ChatRequest {
    pub message: String,
    /// Project ID for vector store retrieval (Phase 6)
    pub project_id: Option<String>,
    /// Reserved for future multimodal support
    pub images: Option<Vec<String>>,
    pub pdfs: Option<Vec<String>>,
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
    pub aside_intensity: Option<u8>,
}

// Convert from services::chat::ChatResponse to ChatReply
impl From<crate::services::chat::ChatResponse> for ChatReply {
    fn from(response: crate::services::chat::ChatResponse) -> Self {
        Self {
            output: response.output,
            persona: response.persona,
            mood: response.mood,
            salience: response.salience as u8,
            summary: Some(response.summary),
            memory_type: response.memory_type,
            tags: response.tags,
            intent: response.intent,
            monologue: response.monologue,
            reasoning_summary: response.reasoning_summary,
            aside_intensity: None, // Not used in services::chat::ChatResponse
        }
    }
}

// Also handle conversion from llm::schema::ChatResponse if needed
impl From<crate::llm::schema::ChatResponse> for ChatReply {
    fn from(response: crate::llm::schema::ChatResponse) -> Self {
        Self {
            output: response.output,
            persona: response.persona,
            mood: response.mood,
            salience: response.salience,
            summary: response.summary,
            memory_type: response.memory_type,
            tags: response.tags,
            intent: response.intent,
            monologue: response.monologue,
            reasoning_summary: response.reasoning_summary,
            aside_intensity: response.aside_intensity,
        }
    }
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
    pub project_id: Option<String>,
}

/// Main REST chat handler - Phase 7 unified implementation
pub async fn chat_handler(
    State(state): State<Arc<AppState>>,
    _headers: HeaderMap,
    Json(payload): Json<ChatRequest>,
) -> Response {
    // Use consistent session ID with WebSocket
    let session_id = "peter-eternal".to_string();
    info!("📮 REST chat request for session: {}", session_id);
    
    if let Some(ref project_id) = payload.project_id {
        info!("📁 Using project context: {}", project_id);
    }

    // Call the unified ChatService
    let result = state
        .chat_service
        .process_message(
            &session_id,
            &payload.message,
            payload.project_id.as_deref(),
            true, // Request structured JSON for consistent response
        )
        .await;

    match result {
        Ok(chat_response) => {
            info!("✅ Chat response generated successfully");
            
            // Log key metrics
            info!("   Salience: {}/10", chat_response.salience);
            info!("   Mood: {}", chat_response.mood);
            if !chat_response.tags.is_empty() {
                info!("   Tags: {:?}", chat_response.tags);
            }
            
            // Convert to ChatReply
            let reply: ChatReply = chat_response.into();
            
            Json(reply).into_response()
        }
        Err(e) => {
            error!("❌ Chat processing failed: {:?}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": "Failed to process chat message",
                    "details": e.to_string()
                })),
            )
                .into_response()
        }
    }
}

/// Get chat history - Phase 7 implementation
pub async fn get_chat_history(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HistoryQuery>,
) -> Response {
    let session_id = "peter-eternal".to_string();
    let limit = params.limit.unwrap_or(20).min(100);
    
    info!("📖 Fetching chat history for session: {} (limit: {})", session_id, limit);
    
    match state.memory_service.get_recent_messages(&session_id, limit).await {
        Ok(messages) => {
            info!("✅ Retrieved {} messages", messages.len());
            Json(ChatHistoryResponse {
                messages,
                session_id,
            })
            .into_response()
        }
        Err(e) => {
            error!("❌ Failed to fetch history: {:?}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": "Failed to fetch chat history",
                    "details": e.to_string()
                })),
            )
                .into_response()
        }
    }
}

/// Health check endpoint
pub async fn health_check() -> impl IntoResponse {
    Json(serde_json::json!({
        "status": "healthy",
        "service": "mira-backend",
        "version": "0.4.1",
        "model": "gpt-5"
    }))
}
