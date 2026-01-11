// crates/mira-server/src/web/api/memory.rs
// Memory CRUD and recall API handlers

use axum::{
    extract::{Path, State},
    response::IntoResponse,
    Json,
};
use mira_types::{ApiResponse, MemoryFact, RecallRequest, RecallResponse, RememberRequest};

use crate::db::parse_memory_fact_row;
use crate::web::state::AppState;

pub async fn list_memories(State(state): State<AppState>) -> impl IntoResponse {
    let project_id = state.project_id().await;
    let conn = state.db.conn();

    let result: Result<Vec<MemoryFact>, _> = (|| {
        let mut stmt = conn.prepare(
            "SELECT id, project_id, key, content, fact_type, category, confidence, created_at
             FROM memory_facts
             WHERE project_id = ?1 OR ?1 IS NULL
             ORDER BY created_at DESC
             LIMIT 100",
        )?;

        let rows = stmt.query_map([project_id], parse_memory_fact_row)?;

        rows.collect::<Result<Vec<_>, _>>()
    })();

    match result {
        Ok(memories) => Json(ApiResponse::ok(memories)),
        Err(e) => Json(ApiResponse::err(e.to_string())),
    }
}

pub async fn create_memory(
    State(state): State<AppState>,
    Json(req): Json<RememberRequest>,
) -> impl IntoResponse {
    let project_id = state.project_id().await;
    let conn = state.db.conn();

    let result: Result<i64, _> = conn
        .execute(
            "INSERT INTO memory_facts (project_id, key, content, fact_type, category, confidence, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, datetime('now'))",
            rusqlite::params![
                project_id,
                req.key,
                req.content,
                req.fact_type.unwrap_or_else(|| "general".to_string()),
                req.category,
                req.confidence.unwrap_or(1.0),
            ],
        )
        .map(|_| conn.last_insert_rowid());

    match result {
        Ok(id) => Json(ApiResponse::ok(serde_json::json!({ "id": id }))),
        Err(e) => Json(ApiResponse::err(e.to_string())),
    }
}

pub async fn get_memory(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    let conn = state.db.conn();

    let result: Result<MemoryFact, _> = conn.query_row(
        "SELECT id, project_id, key, content, fact_type, category, confidence, created_at
         FROM memory_facts WHERE id = ?1",
        [id],
        parse_memory_fact_row,
    );

    match result {
        Ok(memory) => Json(ApiResponse::ok(memory)),
        Err(e) => Json(ApiResponse::err(e.to_string())),
    }
}

pub async fn delete_memory(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    let conn = state.db.conn();

    match conn.execute("DELETE FROM memory_facts WHERE id = ?1", [id]) {
        Ok(deleted) => Json(ApiResponse::ok(serde_json::json!({ "deleted": deleted }))),
        Err(e) => Json(ApiResponse::err(e.to_string())),
    }
}

pub async fn recall(
    State(state): State<AppState>,
    Json(req): Json<RecallRequest>,
) -> impl IntoResponse {
    let project_id = state.project_id().await;

    // Try semantic search first if embeddings available
    if let Some(ref embeddings) = state.embeddings {
        if let Ok(query_embedding) = embeddings.embed(&req.query).await {
            let conn = state.db.conn();
            let limit = req.limit.unwrap_or(10);

            // Convert embedding to bytes for sqlite-vec
            let embedding_bytes: Vec<u8> = query_embedding
                .iter()
                .flat_map(|f| f.to_le_bytes())
                .collect();

            let result: Result<Vec<MemoryFact>, _> = (|| {
                let mut stmt = conn.prepare(
                    "SELECT f.id, f.project_id, f.key, f.content, f.fact_type, f.category, f.confidence, f.created_at
                     FROM memory_facts f
                     JOIN vec_memory v ON f.id = v.fact_id
                     WHERE (f.project_id = ?1 OR ?1 IS NULL)
                     ORDER BY vec_distance_cosine(v.embedding, ?2)
                     LIMIT ?3"
                )?;

                let rows = stmt.query_map(
                    rusqlite::params![project_id, embedding_bytes, limit],
                    parse_memory_fact_row,
                )?;

                rows.collect::<Result<Vec<_>, _>>()
            })();

            if let Ok(memories) = result {
                if !memories.is_empty() {
                    return Json(ApiResponse::ok(RecallResponse { memories }));
                }
            }
        }
    }

    // Fallback to SQL LIKE search
    let conn = state.db.conn();
    let limit = req.limit.unwrap_or(10);
    let pattern = format!("%{}%", req.query);

    let result: Result<Vec<MemoryFact>, _> = (|| {
        let mut stmt = conn.prepare(
            "SELECT id, project_id, key, content, fact_type, category, confidence, created_at
             FROM memory_facts
             WHERE (project_id = ?1 OR ?1 IS NULL)
               AND content LIKE ?2
             ORDER BY created_at DESC
             LIMIT ?3",
        )?;

        let rows = stmt.query_map(
            rusqlite::params![project_id, pattern, limit],
            parse_memory_fact_row,
        )?;

        rows.collect::<Result<Vec<_>, _>>()
    })();

    match result {
        Ok(memories) => Json(ApiResponse::ok(RecallResponse { memories })),
        Err(e) => Json(ApiResponse::err(e.to_string())),
    }
}
