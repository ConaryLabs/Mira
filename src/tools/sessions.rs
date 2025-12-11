// src/tools/sessions.rs
// Cross-session memory tools - remember and search across Claude Code sessions

use chrono::Utc;
use sqlx::sqlite::SqlitePool;
use std::collections::HashMap;

use super::semantic::{SemanticSearch, COLLECTION_CONVERSATION};
use super::types::*;

/// Store a session summary for cross-session recall
pub async fn store_session(
    db: &SqlitePool,
    semantic: &SemanticSearch,
    req: StoreSessionRequest,
) -> anyhow::Result<serde_json::Value> {
    let now = Utc::now().timestamp();
    let session_id = req.session_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    // Store in SQLite for persistence
    sqlx::query(r#"
        INSERT INTO memory_entries (id, session_id, role, content, created_at)
        VALUES ($1, $2, 'session_summary', $3, $4)
    "#)
    .bind(&session_id)
    .bind(&session_id)
    .bind(&req.summary)
    .bind(now)
    .execute(db)
    .await?;

    // Store in Qdrant for semantic search (if available)
    if semantic.is_available() {
        let mut metadata = HashMap::new();
        metadata.insert("session_id".to_string(), serde_json::Value::String(session_id.clone()));
        metadata.insert("type".to_string(), serde_json::Value::String("session_summary".to_string()));
        metadata.insert("timestamp".to_string(), serde_json::Value::Number(now.into()));

        if let Some(ref project) = req.project_path {
            metadata.insert("project_path".to_string(), serde_json::Value::String(project.clone()));
        }

        if let Some(ref topics) = req.topics {
            metadata.insert("topics".to_string(), serde_json::Value::String(topics.join(",")));
        }

        if let Err(e) = semantic.ensure_collection(COLLECTION_CONVERSATION).await {
            tracing::warn!("Failed to ensure conversation collection: {}", e);
        }

        if let Err(e) = semantic.store(
            COLLECTION_CONVERSATION,
            &session_id,
            &req.summary,
            metadata,
        ).await {
            tracing::warn!("Failed to store session in Qdrant: {}", e);
        }
    }

    Ok(serde_json::json!({
        "status": "stored",
        "session_id": session_id,
        "semantic_search": semantic.is_available(),
    }))
}

/// Search across past sessions using semantic similarity
pub async fn search_sessions(
    db: &SqlitePool,
    semantic: &SemanticSearch,
    req: SearchSessionsRequest,
) -> anyhow::Result<Vec<serde_json::Value>> {
    let limit = req.limit.unwrap_or(10) as usize;

    // If semantic search is available, use it
    if semantic.is_available() {
        let results = semantic.search(
            COLLECTION_CONVERSATION,
            &req.query,
            limit,
            None,
        ).await?;

        return Ok(results.into_iter().map(|r| {
            let mut result = serde_json::json!({
                "content": r.content,
                "score": r.score,
            });

            // Add metadata fields
            if let Some(obj) = result.as_object_mut() {
                for (key, value) in r.metadata {
                    obj.insert(key, value);
                }
            }

            result
        }).collect());
    }

    // Fallback to SQLite text search
    let query = r#"
        SELECT id, session_id, content,
               datetime(created_at, 'unixepoch', 'localtime') as created_at
        FROM memory_entries
        WHERE role = 'session_summary'
          AND content LIKE '%' || $1 || '%'
        ORDER BY created_at DESC
        LIMIT $2
    "#;

    let rows = sqlx::query_as::<_, (String, String, String, String)>(query)
        .bind(&req.query)
        .bind(limit as i64)
        .fetch_all(db)
        .await?;

    Ok(rows.into_iter().map(|(id, session_id, content, created_at)| {
        serde_json::json!({
            "id": id,
            "session_id": session_id,
            "content": content,
            "created_at": created_at,
            "search_type": "text",
        })
    }).collect())
}

/// Store a key decision or important context from a session
pub async fn store_decision(
    db: &SqlitePool,
    semantic: &SemanticSearch,
    req: StoreDecisionRequest,
) -> anyhow::Result<serde_json::Value> {
    let now = Utc::now().timestamp();
    let id = uuid::Uuid::new_v4().to_string();

    // Store in memory_facts for structured recall
    sqlx::query(r#"
        INSERT INTO memory_facts (id, fact_type, key, value, category, source, confidence, created_at, updated_at)
        VALUES ($1, 'decision', $2, $3, $4, $5, 1.0, $6, $6)
        ON CONFLICT(key) DO UPDATE SET
            value = excluded.value,
            updated_at = excluded.updated_at
    "#)
    .bind(&id)
    .bind(&req.key)
    .bind(&req.decision)
    .bind(&req.category)
    .bind(&req.context)
    .bind(now)
    .execute(db)
    .await?;

    // Store in Qdrant for semantic search
    if semantic.is_available() {
        let mut metadata = HashMap::new();
        metadata.insert("type".to_string(), serde_json::Value::String("decision".to_string()));
        metadata.insert("key".to_string(), serde_json::Value::String(req.key.clone()));

        if let Some(ref category) = req.category {
            metadata.insert("category".to_string(), serde_json::Value::String(category.clone()));
        }

        if let Some(ref context) = req.context {
            metadata.insert("context".to_string(), serde_json::Value::String(context.clone()));
        }

        if let Err(e) = semantic.ensure_collection(COLLECTION_CONVERSATION).await {
            tracing::warn!("Failed to ensure conversation collection: {}", e);
        }

        if let Err(e) = semantic.store(
            COLLECTION_CONVERSATION,
            &id,
            &req.decision,
            metadata,
        ).await {
            tracing::warn!("Failed to store decision in Qdrant: {}", e);
        }
    }

    Ok(serde_json::json!({
        "status": "stored",
        "id": id,
        "key": req.key,
    }))
}
