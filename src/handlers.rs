// src/handlers.rs
// Final version with borrow checker fixes and cleanup

use axum::{
    extract::{Query, State, Json},
    http::StatusCode,
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{error, info};
use futures::StreamExt;

use crate::state::AppState;
use crate::services::chat::ChatResponse;
use crate::api::two_phase::{get_metadata, get_content_stream};
use crate::llm::streaming::StreamEvent;
use crate::persona::PersonaOverlay;

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

pub async fn health_handler() -> impl IntoResponse {
    Json(HealthResponse {
        status: "healthy".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

pub async fn chat_handler(
    State(state): State<Arc<AppState>>,
    Query(_params): Query<ChatQueryParams>,
    Json(payload): Json<ChatRequest>,
) -> (StatusCode, Json<ChatResponseWrapper>) {
    info!(
        "📨 HTTP Chat request - session: {}",
        payload.session_id
    );

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

    let persona = PersonaOverlay::Default;
    let context = match state.context_service
        .build_context(&payload.session_id, None, Some(&payload.session_id))
        .await
    {
        Ok(ctx) => ctx,
        Err(e) => {
            error!("Failed to build context: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(ChatResponseWrapper {
                success: false,
                data: None,
                error: Some(format!("Failed to build context: {}", e)),
            }));
        }
    };

    let metadata = match get_metadata(
        &state.llm_client,
        &payload.message,
        &persona,
        &context,
    ).await {
        Ok(m) => m,
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(ChatResponseWrapper {
                success: false,
                data: None,
                error: Some(format!("Failed to get metadata: {}", e)),
            }));
        }
    };

    let mut content_stream = match get_content_stream(
        &state.llm_client,
        &payload.message,
        &persona,
        &context,
        &metadata,
    ).await {
        Ok(s) => s,
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(ChatResponseWrapper {
                success: false,
                data: None,
                error: Some(format!("Failed to get content: {}", e)),
            }));
        }
    };

    let mut full_content = String::new();
    while let Some(event) = content_stream.next().await {
        if let Ok(StreamEvent::Delta(chunk)) = event {
            full_content.push_str(&chunk);
        }
    }

    let complete_output = if metadata.output.is_empty() {
        full_content
    } else {
        format!("{}\n\n{}", metadata.output, full_content)
    };
    
    let response = ChatResponse {
        output: complete_output,
        persona: persona.name().to_string(),
        mood: metadata.mood,
        salience: metadata.salience,
        summary: metadata.summary,
        memory_type: metadata.memory_type,
        tags: metadata.tags,
        intent: metadata.intent,
        monologue: metadata.monologue,
        reasoning_summary: metadata.reasoning_summary,
    };

    if let Err(e) = state.memory_service.save_assistant_response(
        &payload.session_id,
        &response,
    ).await {
        error!("Failed to save assistant response: {}", e);
    }

    (StatusCode::OK, Json(ChatResponseWrapper {
        success: true,
        data: Some(response),
        error: None,
    }))
}


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
