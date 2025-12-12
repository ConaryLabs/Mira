// src/tools/sessions.rs
// Cross-session memory tools - remember and search across Claude Code sessions

use chrono::Utc;
use sqlx::sqlite::SqlitePool;
use std::collections::HashMap;

use super::semantic::{SemanticSearch, COLLECTION_CONVERSATION};
use super::types::*;

/// Get session context - combines recent sessions, memories, and pending tasks
/// This is the "where did we leave off?" tool for session startup
pub async fn get_session_context(
    db: &SqlitePool,
    req: GetSessionContextRequest,
    project_id: Option<i64>,
) -> anyhow::Result<serde_json::Value> {
    let limit = req.limit.unwrap_or(5) as i64;
    let include_memories = req.include_memories.unwrap_or(true);
    let include_tasks = req.include_tasks.unwrap_or(true);
    let include_sessions = req.include_sessions.unwrap_or(true);

    let mut context = serde_json::json!({
        "project_id": project_id,
    });

    // Get recent memories (decisions, context, preferences)
    if include_memories {
        let memories = sqlx::query_as::<_, (String, String, String, Option<String>, Option<String>, String)>(r#"
            SELECT id, key, value, fact_type, category,
                   datetime(updated_at, 'unixepoch', 'localtime') as updated
            FROM memory_facts
            WHERE (project_id IS NULL OR project_id = $1)
            ORDER BY updated_at DESC
            LIMIT $2
        "#)
        .bind(project_id)
        .bind(limit)
        .fetch_all(db)
        .await?;

        let memories_json: Vec<serde_json::Value> = memories.into_iter().map(|(id, key, value, fact_type, category, updated)| {
            serde_json::json!({
                "id": id,
                "key": key,
                "value": value,
                "fact_type": fact_type,
                "category": category,
                "updated": updated,
            })
        }).collect();

        context["recent_memories"] = serde_json::json!(memories_json);
    }

    // Get pending/in-progress tasks
    if include_tasks {
        let tasks = sqlx::query_as::<_, (String, String, Option<String>, String, Option<String>, String)>(r#"
            SELECT id, title, description, status, priority,
                   datetime(updated_at, 'unixepoch', 'localtime') as updated
            FROM tasks
            WHERE status IN ('pending', 'in_progress', 'blocked')
              AND (project_path IS NULL OR $1 IS NULL OR project_path LIKE '%' || $1 || '%')
            ORDER BY
                CASE status
                    WHEN 'in_progress' THEN 1
                    WHEN 'blocked' THEN 2
                    WHEN 'pending' THEN 3
                END,
                CASE priority
                    WHEN 'urgent' THEN 1
                    WHEN 'high' THEN 2
                    WHEN 'medium' THEN 3
                    WHEN 'low' THEN 4
                END,
                updated_at DESC
            LIMIT $2
        "#)
        .bind(project_id.map(|_| "")) // Just checking if project is set
        .bind(limit)
        .fetch_all(db)
        .await?;

        let tasks_json: Vec<serde_json::Value> = tasks.into_iter().map(|(id, title, description, status, priority, updated)| {
            serde_json::json!({
                "id": id,
                "title": title,
                "description": description,
                "status": status,
                "priority": priority,
                "updated": updated,
            })
        }).collect();

        context["pending_tasks"] = serde_json::json!(tasks_json);
    }

    // Get recent session summaries
    if include_sessions {
        let sessions = sqlx::query_as::<_, (String, String, String)>(r#"
            SELECT session_id, content,
                   datetime(created_at, 'unixepoch', 'localtime') as created
            FROM memory_entries
            WHERE role = 'session_summary'
              AND (project_id IS NULL OR project_id = $1)
            ORDER BY created_at DESC
            LIMIT $2
        "#)
        .bind(project_id)
        .bind(limit)
        .fetch_all(db)
        .await?;

        let sessions_json: Vec<serde_json::Value> = sessions.into_iter().map(|(session_id, content, created)| {
            serde_json::json!({
                "session_id": session_id,
                "summary": content,
                "created": created,
            })
        }).collect();

        context["recent_sessions"] = serde_json::json!(sessions_json);
    }

    Ok(context)
}

/// Store a session summary for cross-session recall
/// Session is scoped to the active project if project_id is provided
pub async fn store_session(
    db: &SqlitePool,
    semantic: &SemanticSearch,
    req: StoreSessionRequest,
    project_id: Option<i64>,
) -> anyhow::Result<serde_json::Value> {
    let now = Utc::now().timestamp();
    let session_id = req.session_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    // Store in SQLite for persistence
    sqlx::query(r#"
        INSERT INTO memory_entries (id, session_id, role, content, created_at, project_id)
        VALUES ($1, $2, 'session_summary', $3, $4, $5)
    "#)
    .bind(&session_id)
    .bind(&session_id)
    .bind(&req.summary)
    .bind(now)
    .bind(project_id)
    .execute(db)
    .await?;

    // Store in Qdrant for semantic search (if available)
    if semantic.is_available() {
        let mut metadata = HashMap::new();
        metadata.insert("session_id".to_string(), serde_json::Value::String(session_id.clone()));
        metadata.insert("type".to_string(), serde_json::Value::String("session_summary".to_string()));
        metadata.insert("timestamp".to_string(), serde_json::Value::Number(now.into()));

        if let Some(pid) = project_id {
            metadata.insert("project_id".to_string(), serde_json::Value::Number(pid.into()));
        }

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
        "project_id": project_id,
        "semantic_search": semantic.is_available(),
    }))
}

/// Search across past sessions using semantic similarity
/// Returns sessions from the active project AND global sessions
pub async fn search_sessions(
    db: &SqlitePool,
    semantic: &SemanticSearch,
    req: SearchSessionsRequest,
    project_id: Option<i64>,
) -> anyhow::Result<Vec<serde_json::Value>> {
    let limit = req.limit.unwrap_or(10) as usize;

    // If semantic search is available, use it
    if semantic.is_available() {
        // Filter for session summaries
        // Note: Qdrant doesn't support OR conditions with is_null easily,
        // so we filter by type only and rely on SQLite for strict project filtering.
        let filter = Some(qdrant_client::qdrant::Filter::must([
            qdrant_client::qdrant::Condition::matches("type", "session_summary".to_string()),
        ]));
        // If we have a specific project, also add that filter
        let filter = if let Some(pid) = project_id {
            Some(qdrant_client::qdrant::Filter::must([
                qdrant_client::qdrant::Condition::matches("type", "session_summary".to_string()),
                qdrant_client::qdrant::Condition::matches("project_id", pid),
            ]))
        } else {
            filter
        };

        let results = semantic.search(
            COLLECTION_CONVERSATION,
            &req.query,
            limit,
            filter,
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
    // Include sessions from this project AND global sessions
    let query = r#"
        SELECT id, session_id, content,
               datetime(created_at, 'unixepoch', 'localtime') as created_at,
               project_id
        FROM memory_entries
        WHERE role = 'session_summary'
          AND content LIKE '%' || $1 || '%'
          AND (project_id IS NULL OR $2 IS NULL OR project_id = $2)
        ORDER BY created_at DESC
        LIMIT $3
    "#;

    let rows = sqlx::query_as::<_, (String, String, String, String, Option<i64>)>(query)
        .bind(&req.query)
        .bind(project_id)
        .bind(limit as i64)
        .fetch_all(db)
        .await?;

    Ok(rows.into_iter().map(|(id, session_id, content, created_at, proj_id)| {
        serde_json::json!({
            "id": id,
            "session_id": session_id,
            "content": content,
            "created_at": created_at,
            "project_id": proj_id,
            "search_type": "text",
        })
    }).collect())
}

/// Store a key decision or important context from a session
/// Decisions are project-scoped by default (unlike preferences)
pub async fn store_decision(
    db: &SqlitePool,
    semantic: &SemanticSearch,
    req: StoreDecisionRequest,
    project_id: Option<i64>,
) -> anyhow::Result<serde_json::Value> {
    let now = Utc::now().timestamp();
    let id = uuid::Uuid::new_v4().to_string();

    // Store in memory_facts for structured recall
    sqlx::query(r#"
        INSERT INTO memory_facts (id, fact_type, key, value, category, source, confidence, created_at, updated_at, project_id)
        VALUES ($1, 'decision', $2, $3, $4, $5, 1.0, $6, $6, $7)
        ON CONFLICT(key) DO UPDATE SET
            value = excluded.value,
            project_id = COALESCE(excluded.project_id, memory_facts.project_id),
            updated_at = excluded.updated_at
    "#)
    .bind(&id)
    .bind(&req.key)
    .bind(&req.decision)
    .bind(&req.category)
    .bind(&req.context)
    .bind(now)
    .bind(project_id)
    .execute(db)
    .await?;

    // Store in Qdrant for semantic search
    if semantic.is_available() {
        let mut metadata = HashMap::new();
        metadata.insert("type".to_string(), serde_json::Value::String("decision".to_string()));
        metadata.insert("key".to_string(), serde_json::Value::String(req.key.clone()));
        metadata.insert("fact_type".to_string(), serde_json::Value::String("decision".to_string()));

        if let Some(pid) = project_id {
            metadata.insert("project_id".to_string(), serde_json::Value::Number(pid.into()));
        }

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
        "project_id": project_id,
    }))
}
