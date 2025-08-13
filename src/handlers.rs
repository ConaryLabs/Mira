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
use crate::llm::schema::ChatResponse;
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

impl From<ChatResponse> for ChatReply {
    fn from(response: ChatResponse) -> Self {
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
            
            // Convert to API response format
            let reply: ChatReply = chat_response.into();
            
            Json(reply).into_response()
        }
        Err(e) => {
            error!("Chat service error: {:?}", e);
            
            // Return user-friendly error
            let error_response = serde_json::json!({
                "error": "Failed to process message",
                "code": "CHAT_ERROR",
                "details": if cfg!(debug_assertions) {
                    Some(e.to_string())
                } else {
                    None
                }
            });
            
            Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .header("Content-Type", "application/json")
                .body(axum::body::Body::from(error_response.to_string()))
                .unwrap()
                .into_response()
        }
    }
}

/// Get chat history for a session
pub async fn chat_history_handler(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HistoryQuery>,
) -> impl IntoResponse {
    let session_id = "peter-eternal".to_string();
    let limit = params.limit.unwrap_or(30).min(100); // Cap at 100
    let offset = params.offset.unwrap_or(0);

    info!("📜 Fetching chat history: session={}, limit={}, offset={}", 
          session_id, limit, offset);

    // Get messages from memory service
    match state.memory_service.get_recent_messages(&session_id, limit).await {
        Ok(messages) => {
            info!("✅ Retrieved {} messages", messages.len());
            
            let response = ChatHistoryResponse {
                messages,
                session_id,
            };
            
            Json(response).into_response()
        }
        Err(e) => {
            error!("Failed to fetch chat history: {:?}", e);
            
            let error_response = serde_json::json!({
                "error": "Failed to retrieve chat history",
                "code": "HISTORY_ERROR"
            });
            
            Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .header("Content-Type", "application/json")
                .body(axum::body::Body::from(error_response.to_string()))
                .unwrap()
                .into_response()
        }
    }
}

/// Health check endpoint
pub async fn health_handler() -> impl IntoResponse {
    Json(serde_json::json!({
        "status": "healthy",
        "model": "gpt-5",
        "api": "unified",
        "timestamp": chrono::Utc::now().to_rfc3339()
    }))
}

/// Get system status
pub async fn status_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    // Could expand this to check various system components
    let vector_stores = state.vector_store_manager
        .list_stores()
        .await
        .unwrap_or_else(|_| vec![]);
    
    Json(serde_json::json!({
        "status": "operational",
        "model": "gpt-5",
        "services": {
            "chat": "active",
            "memory": "active",
            "context": "active",
            "document": "active",
            "vector_stores": vector_stores.len()
        },
        "timestamp": chrono::Utc::now().to_rfc3339()
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chat_response_conversion() {
        let chat_response = ChatResponse {
            output: "Test output".to_string(),
            persona: "assistant".to_string(),
            mood: "helpful".to_string(),
            salience: 7,
            summary: Some("Test summary".to_string()),
            memory_type: "fact".to_string(),
            tags: vec!["test".to_string()],
            intent: "inform".to_string(),
            monologue: None,
            reasoning_summary: None,
            aside_intensity: None,
        };

        let reply: ChatReply = chat_response.into();
        assert_eq!(reply.output, "Test output");
        assert_eq!(reply.salience, 7);
        assert_eq!(reply.tags, vec!["test"]);
    }
}
