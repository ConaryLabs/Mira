// src/api/http/chat.rs
// Phase 3: Extract Chat Handlers from mod.rs
// Updated to use centralized error handling and configuration

use axum::{
    extract::{Query, State},
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{error, info};

use crate::state::AppState;
use crate::api::error::{ApiResult, IntoApiError};
use crate::config::CONFIG;

// ---------- History response types ----------

#[derive(Serialize)]
pub struct ChatHistoryMessage {
    pub id: String,
    pub role: String,
    pub content: String,
    pub timestamp: i64,
    pub tags: Option<Vec<String>>,
}

#[derive(Serialize)]
pub struct ChatHistoryResponse {
    pub messages: Vec<ChatHistoryMessage>,
}

#[derive(Deserialize)]
pub struct HistoryQuery {
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default)]
    pub offset: usize,
}

fn default_limit() -> usize {
    CONFIG.history_default_limit
}

// ---------- Chat history handler ----------

pub async fn get_chat_history(
    State(app_state): State<Arc<AppState>>,
    Query(query): Query<HistoryQuery>,
) -> impl IntoResponse {
    let result: ApiResult<_> = async {
        let session_id = &CONFIG.session_id;

        info!(
            "📚 Fetching history for session: {} (offset: {}, limit: {})",
            session_id,
            query.offset,
            query.limit
        );

        // Enforce maximum limit from CONFIG
        let max_limit = CONFIG.history_max_limit;
        let skip = query.offset;
        let take = query.limit.min(max_limit);

        // Fetch messages for session
        let memories = app_state
            .memory_service
            .get_recent_context(session_id, skip + take)
            .await
            .into_api_error("Failed to fetch chat history")?;

        info!("📚 Retrieved {} total memories from database", memories.len());

        let messages: Vec<ChatHistoryMessage> = memories
            .into_iter()
            .skip(skip)
            .take(take)
            .map(|m| ChatHistoryMessage {
                id: m.id.map_or_else(
                    || "msg_unknown".to_string(),
                    |id| format!("msg_{}", id)
                ),
                role: m.role,
                content: m.content,
                timestamp: m.timestamp.timestamp(),
                tags: m.tags.filter(|t| !t.is_empty()),
            })
            .collect();

        info!(
            "📚 Returning {} messages (skipped: {}, took: {})",
            messages.len(),
            skip,
            take
        );

        Ok(Json(ChatHistoryResponse { messages }))
    }.await;

    match result {
        Ok(response) => response.into_response(),
        Err(error) => error.into_response(),
    }
}

// ---------- REST Chat Request/Response ----------

#[derive(Deserialize)]
pub struct RestChatRequest {
    pub message: String,
    pub project_id: Option<String>,
    pub persona_override: Option<String>,
    pub file_context: Option<serde_json::Value>,
}

#[derive(Serialize)]
pub struct RestChatResponse {
    pub response: String,
    pub persona: String,
    pub mood: String,
    pub tags: Vec<String>,
    pub summary: String,
    pub tool_results: Option<Vec<serde_json::Value>>,
    pub citations: Option<Vec<serde_json::Value>>,
}

// ---------- REST chat handler ----------

pub async fn rest_chat_handler(
    State(app_state): State<Arc<AppState>>,
    Json(request): Json<RestChatRequest>,
) -> impl IntoResponse {
    let result: ApiResult<_> = async {
        let session_id = &CONFIG.session_id;

        info!("💬 REST chat request for session: {}", session_id);

        // Use regular chat method for now (chat_with_tools will be added in Phase 2)
        let response = app_state.chat_service.chat(
            session_id,
            &request.message,
            request.project_id.as_deref(),
        ).await
        .into_api_error("Chat service failed")?;

        Ok(Json(RestChatResponse {
            response: response.output,
            persona: response.persona,
            mood: response.mood,
            tags: response.tags,
            summary: response.summary,
            tool_results: None,  // Will be populated in Phase 2
            citations: None,     // Will be populated in Phase 2
        }))
    }.await;

    match result {
        Ok(response) => response.into_response(),
        Err(error) => {
            error!("Chat service error: {}", error.message);
            error.into_response()
        }
    }
}
