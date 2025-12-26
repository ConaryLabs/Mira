//! Advisory REST API handlers
//!
//! Exposes advisory session management via REST endpoints:
//! - GET /api/advisory/sessions - List sessions
//! - GET /api/advisory/sessions/:id - Get session details
//! - GET /api/advisory/sessions/:id/stream - SSE stream for ongoing deliberation
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
use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, RwLock};

use crate::advisory::{AdvisoryService, streaming::CouncilProgress};
use crate::core::SemanticSearch;
use crate::server::handlers::advisory;

/// Manages broadcast channels for active deliberation sessions
/// Allows multiple clients to subscribe to the same session's progress
#[derive(Clone, Default)]
pub struct SessionBroadcaster {
    /// Active session broadcast senders (session_id -> sender)
    sessions: Arc<RwLock<HashMap<String, broadcast::Sender<CouncilProgress>>>>,
}

impl SessionBroadcaster {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create a new broadcast channel for a session, returns (sender, receiver)
    pub async fn create_session(&self, session_id: &str) -> (broadcast::Sender<CouncilProgress>, broadcast::Receiver<CouncilProgress>) {
        let (tx, rx) = broadcast::channel(100);
        self.sessions.write().await.insert(session_id.to_string(), tx.clone());
        (tx, rx)
    }

    /// Subscribe to an existing session's progress
    pub async fn subscribe(&self, session_id: &str) -> Option<broadcast::Receiver<CouncilProgress>> {
        self.sessions.read().await.get(session_id).map(|tx| tx.subscribe())
    }

    /// Check if a session is actively broadcasting
    pub async fn is_active(&self, session_id: &str) -> bool {
        self.sessions.read().await.contains_key(session_id)
    }

    /// Remove a completed session
    pub async fn remove_session(&self, session_id: &str) {
        self.sessions.write().await.remove(session_id);
    }
}

/// Shared state for advisory routes
#[derive(Clone)]
pub struct AdvisoryState {
    pub db: Arc<SqlitePool>,
    pub semantic: Arc<SemanticSearch>,
    pub broadcaster: SessionBroadcaster,
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

    // Create broadcast channel for this session (allows multiple subscribers)
    let (broadcast_tx, broadcast_rx) = state.broadcaster.create_session(&session_id).await;

    // Create mpsc channel that forwards to broadcast
    let (tx, mut rx) = mpsc::channel::<CouncilProgress>(100);

    // Forward mpsc to broadcast
    let broadcaster = state.broadcaster.clone();
    let session_id_for_forward = session_id.clone();
    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            // Send to all broadcast subscribers (ignore errors - no subscribers is ok)
            let _ = broadcast_tx.send(event);
        }
        // Clean up when deliberation completes
        broadcaster.remove_session(&session_id_for_forward).await;
    });

    // Clone what we need for the spawned task
    let service = service.clone();
    let db = state.db.clone();
    let semantic = state.semantic.clone();
    let project_id = request.project_id;
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
            &semantic,
            project_id,
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

    // Convert broadcast receiver to SSE stream
    let stream = async_stream::stream! {
        let mut rx = broadcast_rx;

        // Send initial event with session ID
        let init_event = serde_json::json!({
            "type": "session_created",
            "session_id": session_id,
        });
        yield Ok(Event::default().data(serde_json::to_string(&init_event).unwrap_or_default()));

        // Stream progress events
        loop {
            match rx.recv().await {
                Ok(event) => {
                    let data = serde_json::to_string(&event).unwrap_or_default();
                    yield Ok(Event::default().data(data));

                    // Stop streaming after completion events
                    if matches!(event, CouncilProgress::DeliberationComplete { .. } | CouncilProgress::DeliberationFailed { .. }) {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Closed) => break,
                Err(broadcast::error::RecvError::Lagged(_)) => continue, // Skip missed events
            }
        }
    };

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

/// SSE stream for an existing deliberation session
///
/// Subscribe to an ongoing deliberation's progress. Returns 404 if session doesn't exist,
/// or streams completed result if session already finished.
async fn stream_session(
    State(state): State<AdvisoryState>,
    Path(session_id): Path<String>,
) -> Result<
    Sse<axum::response::sse::KeepAliveStream<std::pin::Pin<Box<dyn Stream<Item = Result<Event, Infallible>> + Send>>>>,
    (StatusCode, Json<ErrorResponse>),
> {
    use crate::advisory::session::{get_session, get_deliberation_progress, SessionStatus};

    // Check if session exists
    let session = get_session(&state.db, &session_id).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse::new(e.to_string()))))?
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(ErrorResponse::new("Session not found"))))?;

    // If session is actively deliberating, try to subscribe to broadcast
    if session.status == SessionStatus::Deliberating {
        if let Some(rx) = state.broadcaster.subscribe(&session_id).await {
            let stream = async_stream::stream! {
                let mut rx = rx;

                // Send connection event
                let connect_event = serde_json::json!({
                    "type": "stream_connected",
                    "session_id": session_id,
                    "status": "deliberating",
                });
                yield Ok(Event::default().data(serde_json::to_string(&connect_event).unwrap_or_default()));

                // Stream progress events
                loop {
                    match rx.recv().await {
                        Ok(event) => {
                            let data = serde_json::to_string(&event).unwrap_or_default();
                            yield Ok(Event::default().data(data));

                            if matches!(event, CouncilProgress::DeliberationComplete { .. } | CouncilProgress::DeliberationFailed { .. }) {
                                break;
                            }
                        }
                        Err(broadcast::error::RecvError::Closed) => break,
                        Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    }
                }
            };
            let boxed: std::pin::Pin<Box<dyn Stream<Item = Result<Event, Infallible>> + Send>> =
                Box::pin(stream);
            return Ok(Sse::new(boxed).keep_alive(KeepAlive::default()));
        }
    }

    // Session not actively streaming - return current progress from database
    let db = state.db.clone();
    let session_status = session.status.as_str().to_string();
    let stream = async_stream::stream! {
        // Check for stored progress
        if let Ok(Some(progress)) = get_deliberation_progress(&db, &session_id).await {
            if let Some(result) = progress.result {
                // Session completed - send the result
                let complete_event = serde_json::json!({
                    "type": "deliberation_complete",
                    "result": result,
                });
                yield Ok(Event::default().data(serde_json::to_string(&complete_event).unwrap_or_default()));
            } else {
                // Session in progress but we missed the broadcast - send current state
                let status_event = serde_json::json!({
                    "type": "progress_snapshot",
                    "session_id": session_id,
                    "current_round": progress.current_round,
                    "max_rounds": progress.max_rounds,
                    "status": format!("{:?}", progress.status),
                    "models_responded": progress.models_responded,
                });
                yield Ok(Event::default().data(serde_json::to_string(&status_event).unwrap_or_default()));
            }
        } else {
            // No progress info - session may have completed normally
            let status_event = serde_json::json!({
                "type": "session_status",
                "session_id": session_id,
                "status": session_status,
            });
            yield Ok(Event::default().data(serde_json::to_string(&status_event).unwrap_or_default()));
        }
    };

    let boxed: std::pin::Pin<Box<dyn Stream<Item = Result<Event, Infallible>> + Send>> =
        Box::pin(stream);
    Ok(Sse::new(boxed).keep_alive(KeepAlive::default()))
}

/// Create advisory router
pub fn create_router(db: Arc<SqlitePool>, semantic: Arc<SemanticSearch>) -> Router {
    let state = AdvisoryState {
        db,
        semantic,
        broadcaster: SessionBroadcaster::new(),
    };

    Router::new()
        .route("/api/advisory/sessions", get(list_sessions))
        .route("/api/advisory/sessions/{id}", get(get_session))
        .route("/api/advisory/sessions/{id}/stream", get(stream_session))
        .route("/api/advisory/sessions/{id}/close", axum::routing::post(close_session))
        .route("/api/advisory/deliberate", axum::routing::post(deliberate_stream))
        .with_state(state)
}
