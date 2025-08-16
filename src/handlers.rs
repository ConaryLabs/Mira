// src/handlers.rs
// Fixed version - changed process_message to chat

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{error, info};

use crate::state::AppState;
use crate::services::chat::ChatResponse;

// ============================================================================
// Request/Response Types
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct ChatRequest {
    pub session_id: String,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct ChatResponseWrapper {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<ChatResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ChatQueryParams {
    #[serde(default)]
    pub structured: bool,
}

#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
}

#[derive(Debug, Deserialize)]
pub struct HistoryRequest {
    pub session_id: String,
}

#[derive(Debug, Serialize)]
pub struct HistoryResponse {
    pub success: bool,
    pub messages: Vec<MessageEntry>,
}

#[derive(Debug, Serialize)]
pub struct MessageEntry {
    pub role: String,
    pub content: String,
}

// ============================================================================
// Handlers
// ============================================================================

/// Health check endpoint
pub async fn health_handler() -> impl IntoResponse {
    Json(HealthResponse {
        status: "healthy".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

/// Main chat endpoint
pub async fn chat_handler(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ChatQueryParams>,
    Json(payload): Json<ChatRequest>,
) -> impl IntoResponse {
    info!(
        "📨 Chat request - session: {}, structured: {}",
        payload.session_id, params.structured
    );

    // Validate input
    if payload.message.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ChatResponseWrapper {
                success: false,
                data: None,
                error: Some("Message cannot be empty".to_string()),
            }),
        );
    }

    if payload.session_id.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ChatResponseWrapper {
                success: false,
                data: None,
                error: Some("Session ID cannot be empty".to_string()),
            }),
        );
    }

    let return_structured = params.structured;

    // FIXED: Changed from process_message to chat
    let result = state
        .chat_service
        .chat(
            &payload.session_id,
            &payload.message,
            None,  // project_id - None for now, can be added to payload later
            return_structured,
        )
        .await;

    match result {
        Ok(response) => {
            info!("✅ Chat response generated successfully");
            (
                StatusCode::OK,
                Json(ChatResponseWrapper {
                    success: true,
                    data: Some(response),
                    error: None,
                }),
            )
        }
        Err(e) => {
            error!("❌ Chat processing failed: {:?}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ChatResponseWrapper {
                    success: false,
                    data: None,
                    error: Some(format!("Failed to process message: {}", e)),
                }),
            )
        }
    }
}

/// Get chat history endpoint
pub async fn history_handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<HistoryRequest>,
) -> impl IntoResponse {
    info!("📜 History request for session: {}", payload.session_id);

    let messages = state
        .thread_manager
        .get_full_conversation(&payload.session_id)
        .await;

    let entries: Vec<MessageEntry> = messages
        .into_iter()
        .filter_map(|msg| {
            msg.content.map(|content| MessageEntry {
                role: msg.role,
                content,
            })
        })
        .collect();

    Json(HistoryResponse {
        success: true,
        messages: entries,
    })
}

/// Clear session endpoint
pub async fn clear_session_handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<HistoryRequest>,
) -> impl IntoResponse {
    info!("🧹 Clear session request: {}", payload.session_id);

    match state
        .thread_manager
        .clear_session(&payload.session_id)
        .await
    {
        Ok(_) => {
            info!("✅ Session cleared successfully");
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "success": true,
                    "message": "Session cleared"
                })),
            )
        }
        Err(e) => {
            error!("❌ Failed to clear session: {:?}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "success": false,
                    "error": format!("Failed to clear session: {}", e)
                })),
            )
        }
    }
}
