// src/api/http/memory.rs
use std::sync::Arc;

use axum::{
    extract::{Path, State},
    response::IntoResponse,
    Json,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use tracing::info;

use crate::{
    api::error::{ApiResult, IntoApiError},
    memory::traits::MemoryStore,   // ⬅️ bring trait into scope so .save() resolves
    memory::types::MemoryEntry,
    state::AppState,
};

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
    // Optional: echo created ids for client-side reconciliation
    created_ids: Vec<i64>,
}

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

    info!(%id, "📌 pinned memory");
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

    info!(%id, "📍 unpinned memory");
    Ok(Json(OkWithId { ok: true, id }))
}

/// POST /memory/import
///
/// Minimal importer: persists provided messages under the given session_id.
/// Sets last_accessed=now(); other Phase-4 fields pass through as provided.
pub async fn import_memories(
    State(app): State<Arc<AppState>>,
    Json(payload): Json<ImportPayload>,
) -> ApiResult<impl IntoResponse> {
    let mut created_ids = Vec::with_capacity(payload.messages.len());

    for mut m in payload.messages {
        // normalize/sessionize
        m.session_id = payload.session_id.clone();
        m.last_accessed = Some(Utc::now());

        // persist via SqliteMemoryStore (returns MemoryEntry with id)
        let saved = app
            .sqlite_store
            .save(&m)
            .await
            .into_api_error("Failed to import memory")?;

        if let Some(id) = saved.id {
            created_ids.push(id);
        }
    }

    info!(count = created_ids.len(), "📥 imported memories");
    Ok(Json(ImportOk {
        ok: true,
        count: created_ids.len(),
        created_ids,
    }))
}
