//! Advisory REST API handlers
//!
//! Exposes advisory session management via REST endpoints:
//! - GET /api/advisory/sessions - List sessions
//! - GET /api/advisory/sessions/:id - Get session details
//! - POST /api/advisory/sessions/:id/close - Close/archive a session
//! - POST /api/advisory/deliberate - SSE streaming deliberation

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{
        sse::{Event, KeepAlive, Sse},
        Json,
    },
    routing::get,
    Router,
};
use futures::stream::Stream;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::convert::Infallible;
use std::sync::Arc;
use tokio::sync::mpsc;

use crate::advisory::{AdvisoryService, streaming::CouncilProgress};
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

/// Request for starting a streaming deliberation
#[derive(Debug, Deserialize)]
pub struct DeliberateRequest {
    /// The message/question to deliberate on
    pub message: String,
    /// Optional existing session ID (creates new if not provided)
    pub session_id: Option<String>,
    /// Optional project ID for the session
    pub project_id: Option<i64>,
}

/// SSE streaming deliberation endpoint
///
/// Starts a council deliberation and streams progress events in real-time.
/// Events include: round_started, model_started, model_completed, moderator_analyzing,
/// moderator_complete, early_consensus, synthesis_started, deliberation_complete
async fn deliberate_stream(
    State(state): State<AdvisoryState>,
    Json(request): Json<DeliberateRequest>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, (StatusCode, Json<ErrorResponse>)> {
    // Create advisory service from environment
    let service = Arc::new(AdvisoryService::from_env().map_err(|e| {
        (StatusCode::SERVICE_UNAVAILABLE, Json(ErrorResponse::new(format!("Advisory service not configured: {}", e))))
    })?);

    // Create or get session
    let session_id = match request.session_id {
        Some(id) => id,
        None => {
            use crate::advisory::session::{create_session, SessionMode, update_status, SessionStatus};
            let id = create_session(
                &state.db,
                request.project_id,
                SessionMode::Council,
                None,
                Some("Advisory council session"),
            ).await.map_err(|e| {
                (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse::new(e.to_string())))
            })?;

            // Mark as deliberating
            let _ = update_status(&state.db, &id, SessionStatus::Deliberating).await;
            id
        }
    };

    // Create channel for progress events
    let (tx, rx) = mpsc::channel::<CouncilProgress>(100);

    // Clone what we need for the spawned task
    let service = service.clone();
    let db = state.db.clone();
    let message = request.message.clone();
    let session_id_clone = session_id.clone();

    // Spawn the deliberation task
    tokio::spawn(async move {
        use crate::advisory::session::{update_status, SessionStatus, add_message_with_usage};

        // Store the user message
        let _ = add_message_with_usage(
            &db,
            &session_id_clone,
            "user",
            &message,
            None,
            None,
            None,
        ).await;

        // Run deliberation
        let result = service.council_deliberate_streaming(
            &message,
            None,
            &db,
            &session_id_clone,
            tx,
        ).await;

        // Update session status based on result
        match result {
            Ok(synthesis) => {
                let _ = update_status(&db, &session_id_clone, SessionStatus::Active).await;

                // Store synthesis as assistant message
                let synthesis_json = serde_json::to_string(&synthesis.to_json()).ok();
                let _ = add_message_with_usage(
                    &db,
                    &session_id_clone,
                    "assistant",
                    &synthesis.synthesis.to_markdown(),
                    Some("council"),
                    synthesis_json.as_deref(),
                    None,
                ).await;
            }
            Err(e) => {
                tracing::error!(error = %e, session_id = %session_id_clone, "Deliberation failed");
                let _ = update_status(&db, &session_id_clone, SessionStatus::Failed).await;
            }
        }
    });

    // Convert channel to SSE stream
    let stream = async_stream::stream! {
        let mut rx = rx;

        // Send initial event with session ID
        let init_event = serde_json::json!({
            "type": "session_created",
            "session_id": session_id,
        });
        yield Ok(Event::default().data(serde_json::to_string(&init_event).unwrap_or_default()));

        // Stream progress events
        while let Some(event) = rx.recv().await {
            let data = serde_json::to_string(&event).unwrap_or_default();
            yield Ok(Event::default().data(data));
        }
    };

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

/// Create advisory router
pub fn create_router(db: Arc<SqlitePool>) -> Router {
    let state = AdvisoryState { db };

    Router::new()
        .route("/api/advisory/sessions", get(list_sessions))
        .route("/api/advisory/sessions/{id}", get(get_session))
        .route("/api/advisory/sessions/{id}/close", axum::routing::post(close_session))
        .route("/api/advisory/deliberate", axum::routing::post(deliberate_stream))
        .with_state(state)
}
