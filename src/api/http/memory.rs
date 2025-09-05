// src/api/http/memory.rs
// PHASE 4: Complete memory API with rolling summaries and snapshot support
// FIXED: Removed duplicate method definitions - these are already in MemoryService

use std::sync::Arc;

use axum::{
    extract::{Path, State, Query},
    response::IntoResponse,
    Json,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::{
    api::error::{ApiResult, IntoApiError},
    config::CONFIG,
    memory::traits::MemoryStore,
    memory::types::MemoryEntry,
    state::AppState,
};

// ═══════════════════════════════════════════════════════════════
// ─── REQUEST/RESPONSE TYPES ────────────────────────────────────
// ═══════════════════════════════════════════════════════════════

#[derive(Debug, Deserialize)]
pub struct ImportPayload {
    pub session_id: String,
    pub messages: Vec<MemoryEntry>,
}

#[derive(Debug, Serialize)]
struct OkWithId {
    ok: bool,
    id: i64,
}

#[derive(Debug, Serialize)]
struct ImportOk {
    ok: bool,
    count: usize,
    created_ids: Vec<i64>,
}

/// Request body for snapshot summary creation
#[derive(Debug, Deserialize)]
pub struct SnapshotSummaryRequest {
    /// Number of messages to summarize (default: all messages)
    pub message_count: Option<usize>,
    /// Optional tag to apply to the summary
    pub tag: Option<String>,
}

/// Response for summary operations
#[derive(Debug, Serialize)]
pub struct SummaryResponse {
    pub ok: bool,
    pub session_id: String,
    pub summary_type: String,
    pub message_count: usize,
    pub summary_id: Option<i64>,
}

/// Request for rolling summary creation
#[derive(Debug, Deserialize)]
pub struct RollingSummaryRequest {
    /// Window size (10 or 100 typically)
    pub window_size: Option<usize>,
}

/// Query params for summary status
#[derive(Debug, Deserialize)]
pub struct SummaryStatusQuery {
    pub include_content: Option<bool>,
}

/// Summary status response
#[derive(Debug, Serialize)]
pub struct SummaryStatusResponse {
    pub session_id: String,
    pub total_messages: usize,
    pub rolling_10_count: usize,
    pub rolling_100_count: usize,
    pub snapshot_count: usize,
    pub last_summary_at: Option<chrono::DateTime<Utc>>,
    pub summaries_enabled: bool,
    pub use_in_context: bool,
}

// ═══════════════════════════════════════════════════════════════
// ─── EXISTING ENDPOINTS ────────────────────────────────────────
// ═══════════════════════════════════════════════════════════════

/// POST /memory/:id/pin
pub async fn pin_memory(
    State(app): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> ApiResult<impl IntoResponse> {
    sqlx::query("UPDATE chat_history SET pinned = 1 WHERE id = ?")
        .bind(id)
        .execute(&app.sqlite_store.pool)
        .await
        .into_api_error("Failed to pin memory")?;

    info!(%id, "📌 Pinned memory");
    Ok(Json(OkWithId { ok: true, id }))
}

/// POST /memory/:id/unpin
pub async fn unpin_memory(
    State(app): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> ApiResult<impl IntoResponse> {
    sqlx::query("UPDATE chat_history SET pinned = 0 WHERE id = ?")
        .bind(id)
        .execute(&app.sqlite_store.pool)
        .await
        .into_api_error("Failed to unpin memory")?;

    info!(%id, "📍 Unpinned memory");
    Ok(Json(OkWithId { ok: true, id }))
}

/// POST /memory/import
pub async fn import_memories(
    State(app): State<Arc<AppState>>,
    Json(payload): Json<ImportPayload>,
) -> ApiResult<impl IntoResponse> {
    let mut created_ids = Vec::with_capacity(payload.messages.len());

    for mut m in payload.messages {
        // Normalize/sessionize
        m.session_id = payload.session_id.clone();
        m.last_accessed = Some(Utc::now());

        // Persist via SqliteMemoryStore (returns MemoryEntry with id)
        let saved = app
            .sqlite_store
            .save(&m)
            .await
            .into_api_error("Failed to import memory")?;

        if let Some(id) = saved.id {
            created_ids.push(id);
        }
    }

    info!(count = created_ids.len(), "📥 Imported memories");
    Ok(Json(ImportOk {
        ok: true,
        count: created_ids.len(),
        created_ids,
    }))
}

// ═══════════════════════════════════════════════════════════════
// ─── NEW ROLLING SUMMARY ENDPOINTS (CRITICAL FIX #3) ───────────
// ═══════════════════════════════════════════════════════════════

/// POST /memory/:session_id/snapshot_summary
/// Creates a manual snapshot summary for the specified session
pub async fn create_snapshot_summary(
    State(app): State<Arc<AppState>>,
    Path(session_id): Path<String>,
    Json(req): Json<SnapshotSummaryRequest>,
) -> ApiResult<impl IntoResponse> {
    // Check if snapshot summaries are enabled
    if !CONFIG.snapshot_summaries_enabled() {
        warn!("Snapshot summary requested but feature is disabled");
        return Err(crate::api::error::ApiError::bad_request(
            "Snapshot summaries are not enabled. Set MIRA_SUMMARY_PHASE_SNAPSHOTS=true"
        ));
    }

    info!("📸 Creating snapshot summary for session: {}", session_id);

    // Get message count (default to all messages)
    let message_count = if let Some(count) = req.message_count {
        count
    } else {
        // Get total message count for session - use the existing method
        app.memory_service
            .get_session_message_count(&session_id)
            .await
    };

    // Create the snapshot summary - use the existing method from MemoryService
    app.memory_service
        .create_snapshot_summary(&session_id, message_count)
        .await
        .into_api_error("Failed to create snapshot summary")?;

    info!("✅ Snapshot summary created for {} messages", message_count);

    Ok(Json(SummaryResponse {
        ok: true,
        session_id: session_id.clone(),
        summary_type: "snapshot".to_string(),
        message_count,
        summary_id: None,
    }))
}

/// POST /memory/:session_id/rolling_summary
/// Manually triggers a rolling summary for the specified session
pub async fn create_rolling_summary(
    State(app): State<Arc<AppState>>,
    Path(session_id): Path<String>,
    Json(req): Json<RollingSummaryRequest>,
) -> ApiResult<impl IntoResponse> {
    // Check if rolling summaries are enabled
    if !CONFIG.rolling_summaries_enabled() {
        warn!("Rolling summary requested but feature is disabled");
        return Err(crate::api::error::ApiError::bad_request(
            "Rolling summaries are not enabled. Set MIRA_AGGRESSIVE_METADATA_ENABLED=true"
        ));
    }

    let window_size = req.window_size.unwrap_or(10);

    // Validate window size
    if window_size != 10 && window_size != 100 {
        return Err(crate::api::error::ApiError::bad_request(
            "Window size must be 10 or 100"
        ));
    }

    info!("🔄 Creating {}-message rolling summary for session: {}", window_size, session_id);

    // Create the rolling summary - use the existing method from MemoryService
    app.memory_service
        .create_rolling_summary(&session_id, window_size)
        .await
        .into_api_error("Failed to create rolling summary")?;

    info!("✅ Rolling summary created for {} messages", window_size);

    Ok(Json(SummaryResponse {
        ok: true,
        session_id: session_id.clone(),
        summary_type: format!("rolling_{}", window_size),
        message_count: window_size,
        summary_id: None,
    }))
}

/// GET /memory/:session_id/summary_status
/// Returns the current summarization status for a session
pub async fn get_summary_status(
    State(app): State<Arc<AppState>>,
    Path(session_id): Path<String>,
    Query(_params): Query<SummaryStatusQuery>,
) -> ApiResult<impl IntoResponse> {
    info!("📊 Getting summary status for session: {}", session_id);

    // Get message count using existing method
    let total_messages = app.memory_service
        .get_session_message_count(&session_id)
        .await;

    // Get summary counts from database
    let rolling_10_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM chat_history 
         WHERE session_id = ? AND tags LIKE '%summary:rolling:10%'"
    )
    .bind(&session_id)
    .fetch_one(&app.sqlite_store.pool)
    .await
    .unwrap_or(0);

    let rolling_100_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM chat_history 
         WHERE session_id = ? AND tags LIKE '%summary:rolling:100%'"
    )
    .bind(&session_id)
    .fetch_one(&app.sqlite_store.pool)
    .await
    .unwrap_or(0);

    let snapshot_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM chat_history 
         WHERE session_id = ? AND tags LIKE '%summary:snapshot:%'"
    )
    .bind(&session_id)
    .fetch_one(&app.sqlite_store.pool)
    .await
    .unwrap_or(0);

    // Get last summary timestamp
    let last_summary_at: Option<chrono::DateTime<Utc>> = sqlx::query_scalar(
        "SELECT MAX(timestamp) FROM chat_history 
         WHERE session_id = ? AND tags LIKE '%summary:%'"
    )
    .bind(&session_id)
    .fetch_optional(&app.sqlite_store.pool)
    .await
    .unwrap_or(None);

    Ok(Json(SummaryStatusResponse {
        session_id,
        total_messages,
        rolling_10_count: rolling_10_count as usize,
        rolling_100_count: rolling_100_count as usize,
        snapshot_count: snapshot_count as usize,
        last_summary_at,
        summaries_enabled: CONFIG.rolling_summaries_enabled(),
        use_in_context: CONFIG.should_use_rolling_summaries_in_context(),
    }))
}

/// DELETE /memory/:session_id/summaries
/// Clears all summaries for a session (useful for testing/reset)
pub async fn clear_summaries(
    State(app): State<Arc<AppState>>,
    Path(session_id): Path<String>,
) -> ApiResult<impl IntoResponse> {
    info!("🗑️ Clearing all summaries for session: {}", session_id);

    let result = sqlx::query(
        "DELETE FROM chat_history 
         WHERE session_id = ? AND tags LIKE '%summary:%'"
    )
    .bind(&session_id)
    .execute(&app.sqlite_store.pool)
    .await
    .into_api_error("Failed to clear summaries")?;

    let deleted_count = result.rows_affected();
    info!("✅ Cleared {} summaries for session", deleted_count);

    Ok(Json(serde_json::json!({
        "ok": true,
        "session_id": session_id,
        "deleted_count": deleted_count,
    })))
}

/// GET /memory/:session_id/summaries
/// Lists all summaries for a session
pub async fn list_summaries(
    State(app): State<Arc<AppState>>,
    Path(session_id): Path<String>,
) -> ApiResult<impl IntoResponse> {
    info!("📋 Listing summaries for session: {}", session_id);

    #[derive(sqlx::FromRow, Serialize)]
    struct SummaryInfo {
        id: i64,
        timestamp: chrono::DateTime<Utc>,
        tags: Option<String>,
        summary: Option<String>,
    }

    let summaries: Vec<SummaryInfo> = sqlx::query_as(
        "SELECT id, timestamp, tags, summary FROM chat_history 
         WHERE session_id = ? AND tags LIKE '%summary:%'
         ORDER BY timestamp DESC
         LIMIT 100"
    )
    .bind(&session_id)
    .fetch_all(&app.sqlite_store.pool)
    .await
    .into_api_error("Failed to fetch summaries")?;

    Ok(Json(serde_json::json!({
        "ok": true,
        "session_id": session_id,
        "count": summaries.len(),
        "summaries": summaries,
    })))
}
