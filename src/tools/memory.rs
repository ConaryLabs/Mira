// src/tools/memory.rs
// Memory tools - persistent facts, decisions, preferences across sessions

use chrono::Utc;
use sqlx::sqlite::SqlitePool;
use std::collections::HashMap;
use uuid::Uuid;

use super::semantic::{SemanticSearch, COLLECTION_CONVERSATION};
use super::types::*;

/// Remember a fact, decision, or preference
pub async fn remember(
    db: &SqlitePool,
    semantic: &SemanticSearch,
    req: RememberRequest,
) -> anyhow::Result<serde_json::Value> {
    let now = Utc::now().timestamp();
    let id = Uuid::new_v4().to_string();
    let fact_type = req.fact_type.clone().unwrap_or_else(|| "general".to_string());

    // Generate key from content if not provided (first 50 chars, normalized)
    let key = req.key.clone().unwrap_or_else(|| {
        req.content
            .chars()
            .take(50)
            .collect::<String>()
            .to_lowercase()
            .replace(|c: char| !c.is_alphanumeric() && c != ' ', "")
            .trim()
            .to_string()
    });

    // Upsert: update if key exists, insert if not
    sqlx::query(r#"
        INSERT INTO memory_facts (id, fact_type, key, value, category, source, confidence, times_used, created_at, updated_at)
        VALUES ($1, $2, $3, $4, $5, 'claude-code', 1.0, 0, $6, $6)
        ON CONFLICT(key) DO UPDATE SET
            value = excluded.value,
            fact_type = excluded.fact_type,
            category = COALESCE(excluded.category, memory_facts.category),
            updated_at = excluded.updated_at
    "#)
    .bind(&id)
    .bind(&fact_type)
    .bind(&key)
    .bind(&req.content)
    .bind(&req.category)
    .bind(now)
    .execute(db)
    .await?;

    // Also store in Qdrant for semantic search
    if semantic.is_available() {
        let mut metadata = HashMap::new();
        metadata.insert("type".to_string(), serde_json::Value::String("memory_fact".to_string()));
        metadata.insert("fact_type".to_string(), serde_json::Value::String(fact_type.clone()));
        metadata.insert("key".to_string(), serde_json::Value::String(key.clone()));
        if let Some(ref cat) = req.category {
            metadata.insert("category".to_string(), serde_json::Value::String(cat.clone()));
        }

        if let Err(e) = semantic.ensure_collection(COLLECTION_CONVERSATION).await {
            tracing::warn!("Failed to ensure conversation collection: {}", e);
        }

        if let Err(e) = semantic.store(COLLECTION_CONVERSATION, &id, &req.content, metadata).await {
            tracing::warn!("Failed to store memory in Qdrant: {}", e);
        }
    }

    Ok(serde_json::json!({
        "status": "remembered",
        "key": key,
        "fact_type": fact_type,
        "category": req.category,
        "semantic_search": semantic.is_available(),
    }))
}

/// Recall memories matching a query - uses semantic search if available
pub async fn recall(
    db: &SqlitePool,
    semantic: &SemanticSearch,
    req: RecallRequest,
) -> anyhow::Result<Vec<serde_json::Value>> {
    let limit = req.limit.unwrap_or(10) as usize;
    let now = Utc::now().timestamp();

    // Try semantic search first if available
    if semantic.is_available() {
        let filter = if let Some(ref fact_type) = req.fact_type {
            Some(qdrant_client::qdrant::Filter::must([
                qdrant_client::qdrant::Condition::matches("fact_type", fact_type.clone())
            ]))
        } else {
            None
        };

        match semantic.search(COLLECTION_CONVERSATION, &req.query, limit, filter).await {
            Ok(results) if !results.is_empty() => {
                // Update times_used for semantic results
                for result in &results {
                    if let Some(serde_json::Value::String(key)) = result.metadata.get("key") {
                        let _ = sqlx::query(
                            "UPDATE memory_facts SET times_used = times_used + 1, last_used_at = $1 WHERE key = $2"
                        )
                        .bind(now)
                        .bind(key)
                        .execute(db)
                        .await;
                    }
                }

                return Ok(results.into_iter().map(|r| {
                    serde_json::json!({
                        "value": r.content,
                        "score": r.score,
                        "search_type": "semantic",
                        "fact_type": r.metadata.get("fact_type"),
                        "key": r.metadata.get("key"),
                        "category": r.metadata.get("category"),
                    })
                }).collect());
            }
            Ok(_) => {
                // No semantic results, fall through to text search
                tracing::debug!("No semantic results for query: {}", req.query);
            }
            Err(e) => {
                tracing::warn!("Semantic search failed, falling back to text: {}", e);
            }
        }
    }

    // Fallback to SQLite text search
    let search_pattern = format!("%{}%", req.query);

    let query = r#"
        SELECT id, fact_type, key, value, category, confidence, times_used,
               datetime(created_at, 'unixepoch', 'localtime') as created_at
        FROM memory_facts
        WHERE (value LIKE $1 OR key LIKE $1 OR category LIKE $1)
          AND ($2 IS NULL OR fact_type = $2)
          AND ($3 IS NULL OR category = $3)
        ORDER BY times_used DESC, updated_at DESC
        LIMIT $4
    "#;

    let rows = sqlx::query_as::<_, (String, String, String, String, Option<String>, f64, i64, String)>(query)
        .bind(&search_pattern)
        .bind(&req.fact_type)
        .bind(&req.category)
        .bind(limit as i64)
        .fetch_all(db)
        .await?;

    // Update times_used and last_used_at for returned results
    for (id, _, _, _, _, _, _, _) in &rows {
        let _ = sqlx::query("UPDATE memory_facts SET times_used = times_used + 1, last_used_at = $1 WHERE id = $2")
            .bind(now)
            .bind(id)
            .execute(db)
            .await;
    }

    Ok(rows
        .into_iter()
        .map(|(id, fact_type, key, value, category, confidence, times_used, created_at)| {
            serde_json::json!({
                "id": id,
                "fact_type": fact_type,
                "key": key,
                "value": value,
                "category": category,
                "confidence": confidence,
                "times_used": times_used,
                "created_at": created_at,
                "search_type": "text",
            })
        })
        .collect())
}

/// Forget (delete) a memory
pub async fn forget(
    db: &SqlitePool,
    semantic: &SemanticSearch,
    req: ForgetRequest,
) -> anyhow::Result<serde_json::Value> {
    let result = sqlx::query("DELETE FROM memory_facts WHERE id = $1")
        .bind(&req.id)
        .execute(db)
        .await?;

    // Also delete from Qdrant if available
    if semantic.is_available() {
        if let Err(e) = semantic.delete(COLLECTION_CONVERSATION, &req.id).await {
            tracing::warn!("Failed to delete from Qdrant: {}", e);
        }
    }

    if result.rows_affected() > 0 {
        Ok(serde_json::json!({
            "status": "forgotten",
            "id": req.id,
        }))
    } else {
        Ok(serde_json::json!({
            "status": "not_found",
            "id": req.id,
        }))
    }
}
