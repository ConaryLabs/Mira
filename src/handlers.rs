// src/handlers.rs
use axum::{
    extract::{Json, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::error;

use crate::services::chat::ChatResponse;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct ChatRequest {
    pub session_id: String,
    pub message: String,
    pub project_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ChatQueryParams {
    #[serde(default)]
    pub structured: bool,
}

#[derive(Debug, Serialize)]
pub struct ChatResponseWrapper {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<ChatResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

pub async fn chat_handler(
    State(state): State<Arc<AppState>>,
    Query(_params): Query<ChatQueryParams>,
    Json(payload): Json<ChatRequest>,
) -> impl IntoResponse {
    match state
        .chat_service
        .chat(&payload.session_id, &payload.message, payload.project_id.as_deref())
        .await
    {
        Ok(resp) => axum::Json(ChatResponseWrapper {
            success: true,
            data: Some(resp),
            error: None,
        })
        .into_response(),
        Err(e) => {
            error!("chat_handler error: {}", e);
            Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(axum::body::Body::from("Service temporarily unavailable"))
                .unwrap()
        }
    }
}
