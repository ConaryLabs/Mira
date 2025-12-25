//! Advisory REST API handlers
//!
//! Exposes advisory session management via REST endpoints:
//! - GET /api/advisory/sessions - List sessions
//! - GET /api/advisory/sessions/:id - Get session details
//! - POST /api/advisory/sessions/:id/close - Close/archive a session

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::Json,
    routing::get,
    Router,
};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::sync::Arc;

use crate::server::handlers::advisory;

/// Shared state for advisory routes
#[derive(Clone)]
pub struct AdvisoryState {
    pub db: Arc<SqlitePool>,
}

/// Query params for listing sessions
#[derive(Debug, Deserialize)]
pub struct ListParams {
    pub limit: Option<i64>,
    pub project_id: Option<i64>,
}

/// Error response
#[derive(Serialize)]
pub struct ErrorResponse {
    pub success: bool,
    pub error: String,
}

impl ErrorResponse {
    pub fn new(msg: impl Into<String>) -> Self {
        Self {
            success: false,
            error: msg.into(),
        }
    }
}

/// List advisory sessions
async fn list_sessions(
    State(state): State<AdvisoryState>,
    Query(params): Query<ListParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let limit = params.limit.unwrap_or(20);

    advisory::list(&state.db, params.project_id, limit)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse::new(e.to_string()))))
}

/// Get a specific session with messages, pins, and decisions
async fn get_session(
    State(state): State<AdvisoryState>,
    Path(session_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    match advisory::get(&state.db, &session_id).await {
        Ok(result) => Ok(Json(result)),
        Err(e) => {
            let msg = e.to_string();
            let status = if msg.contains("not found") {
                StatusCode::NOT_FOUND
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            Err((status, Json(ErrorResponse::new(msg))))
        }
    }
}

/// Close/archive a session
async fn close_session(
    State(state): State<AdvisoryState>,
    Path(session_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    advisory::close(&state.db, &session_id)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse::new(e.to_string()))))
}

/// Create advisory router
pub fn create_router(db: Arc<SqlitePool>) -> Router {
    let state = AdvisoryState { db };

    Router::new()
        .route("/api/advisory/sessions", get(list_sessions))
        .route("/api/advisory/sessions/{id}", get(get_session))
        .route("/api/advisory/sessions/{id}/close", axum::routing::post(close_session))
        .with_state(state)
}
