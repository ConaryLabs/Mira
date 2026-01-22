//! Unified memory tools (recall, remember, forget)

use crate::search::{embedding_to_bytes, format_project_header};
use crate::tools::core::ToolContext;
use mira_types::MemoryFact;

/// Store a memory fact
pub async fn remember<C: ToolContext>(
    ctx: &C,
    content: String,
    key: Option<String>,
    fact_type: Option<String>,
    category: Option<String>,
    confidence: Option<f64>,
    scope: Option<String>,
) -> Result<String, String> {
    let project_id = ctx.project_id().await;
    let session_id = ctx.get_session_id().await;
    let user_id = ctx.get_user_identity();

    let fact_type = fact_type.unwrap_or_else(|| "general".to_string());
    let confidence = confidence.unwrap_or(0.5); // Start with lower confidence for evidence-based system
    // Default scope is "project" for backward compatibility
    let scope = scope.unwrap_or_else(|| "project".to_string());

    // Validate scope
    if !["personal", "project", "team"].contains(&scope.as_str()) {
        return Err(format!("Invalid scope '{}'. Must be one of: personal, project, team", scope));
    }

    // Personal scope requires user_id
    if scope == "personal" && user_id.is_none() {
        return Err("Cannot create personal memory: user identity not available".to_string());
    }

    // Store in SQL with session tracking via connection pool
    let content_for_store = content.clone();
    let key_for_store = key.clone();
    let category_for_store = category.clone();
    let fact_type_for_store = fact_type.clone();
    let session_id_for_store = session_id.clone();
    let user_id_for_store = user_id.clone();
    let scope_for_store = scope.clone();
    let id: i64 = ctx
        .pool()
        .interact(move |conn| {
            use rusqlite::params;

            // Upsert by key if provided
            if let Some(ref k) = key_for_store {
                let existing: Option<(i64, Option<String>)> = conn
                    .query_row(
                        "SELECT id, last_session_id FROM memory_facts WHERE key = ? AND (project_id = ? OR project_id IS NULL)",
                        params![k, project_id],
                        |row| Ok((row.get(0)?, row.get(1)?)),
                    )
                    .ok();

                if let Some((id, last_session)) = existing {
                    let is_new_session = session_id_for_store
                        .as_ref()
                        .map(|s| last_session.as_deref() != Some(s))
                        .unwrap_or(false);

                    if is_new_session {
                        conn.execute(
                            "UPDATE memory_facts SET content = ?, fact_type = ?, category = ?, confidence = ?,
                             session_count = session_count + 1, last_session_id = ?, user_id = COALESCE(user_id, ?),
                             scope = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
                            params![content_for_store, fact_type_for_store, category_for_store, confidence,
                                    session_id_for_store, user_id_for_store, scope_for_store, id],
                        )?;
                        // Check for promotion
                        conn.execute(
                            "UPDATE memory_facts SET status = 'confirmed', confidence = MIN(confidence + 0.2, 1.0)
                             WHERE id = ? AND status = 'candidate' AND session_count >= 3",
                            [id],
                        )?;
                    } else {
                        conn.execute(
                            "UPDATE memory_facts SET content = ?, fact_type = ?, category = ?, confidence = ?,
                             user_id = COALESCE(user_id, ?), scope = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
                            params![content_for_store, fact_type_for_store, category_for_store, confidence,
                                    user_id_for_store, scope_for_store, id],
                        )?;
                    }
                    return Ok(id);
                }
            }

            // New memory - starts as candidate with low confidence
            let initial_confidence = if confidence < 1.0 { confidence } else { 0.5 };
            conn.execute(
                "INSERT INTO memory_facts (project_id, key, content, fact_type, category, confidence,
                 session_count, first_session_id, last_session_id, status, user_id, scope)
                 VALUES (?, ?, ?, ?, ?, ?, 1, ?, ?, 'candidate', ?, ?)",
                params![project_id, key_for_store, content_for_store, fact_type_for_store, category_for_store,
                        initial_confidence, session_id_for_store, session_id_for_store, user_id_for_store, scope_for_store],
            )?;
            Ok(conn.last_insert_rowid())
        })
        .await
        .map_err(|e| e.to_string())?;

    // Store embedding if available
    if let Some(embeddings) = ctx.embeddings() {
        match embeddings.embed(&content).await {
            Ok(embedding) => {
                // Insert into vec_memory via connection pool
                let embedding_bytes = embedding_to_bytes(&embedding);
                let content_clone = content.clone();
                let result = ctx
                    .pool()
                    .interact(move |conn| {
                        use rusqlite::params;
                        conn.execute(
                            "INSERT INTO vec_memory (rowid, embedding, fact_id, content) VALUES (?, ?, ?, ?)",
                            params![id, embedding_bytes, id, &content_clone],
                        )?;
                        Ok(())
                    })
                    .await;
                if let Err(e) = result {
                    tracing::warn!("Failed to store embedding: {}", e);
                }
            }
            Err(e) => {
                tracing::warn!("Failed to generate embedding: {}", e);
            }
        }
    }

    Ok(format!(
        "Stored memory (id: {}){}",
        id,
        if key.is_some() { " with key" } else { "" }
    ))
}

/// Search memories using semantic similarity or keyword fallback
pub async fn recall<C: ToolContext>(
    ctx: &C,
    query: String,
    limit: Option<i64>,
    _category: Option<String>,
    _fact_type: Option<String>,
) -> Result<String, String> {
    let project_id = ctx.project_id().await;
    let session_id = ctx.get_session_id().await;
    let project = ctx.get_project().await;
    let user_id = ctx.get_user_identity();
    let context_header = format_project_header(project.as_ref());

    let limit = limit.unwrap_or(10) as usize;

    // Try semantic search first if embeddings available
    if let Some(embeddings) = ctx.embeddings() {
        if let Ok(query_embedding) = embeddings.embed(&query).await {
            let embedding_bytes = embedding_to_bytes(&query_embedding);
            let user_id_for_query = user_id.clone();

            // Run vector search via connection pool with scope filtering
            let results: Result<Vec<(i64, String, f32)>, String> = ctx
                .pool()
                .interact(move |conn| {
                    use rusqlite::params;
                    // Scope filtering logic:
                    // - project scope: visible to all with project access
                    // - personal scope: only visible to creator (user_id match)
                    // - team scope: TODO - requires team membership check
                    // - legacy memories (user_id IS NULL): treated as project scope
                    let mut stmt = conn.prepare(
                        "SELECT v.fact_id, v.content, vec_distance_cosine(v.embedding, ?1) as distance
                         FROM vec_memory v
                         JOIN memory_facts f ON v.fact_id = f.id
                         WHERE (f.project_id = ?2 OR f.project_id IS NULL OR ?2 IS NULL)
                           AND (
                             f.scope = 'project'
                             OR f.scope IS NULL
                             OR (f.scope = 'personal' AND f.user_id = ?4)
                             OR f.user_id IS NULL
                           )
                         ORDER BY distance
                         LIMIT ?3",
                    )?;

                    let results: Vec<(i64, String, f32)> = stmt
                        .query_map(params![embedding_bytes, project_id, limit as i64, user_id_for_query], |row| {
                            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
                        })?
                        .filter_map(|r| r.ok())
                        .collect();

                    Ok(results)
                })
                .await
                .map_err(|e| e.to_string());

            if let Ok(results) = results {
                if !results.is_empty() {
                    // Record memory access for evidence-based tracking
                    if let Some(ref sid) = session_id {
                        let ids: Vec<i64> = results.iter().map(|(id, _, _)| *id).collect();
                        let pool_clone = ctx.pool().clone();
                        let sid_clone = sid.clone();
                        // Fire and forget - don't block on this
                        tokio::spawn(async move {
                            let _ = pool_clone
                                .interact(move |conn| {
                                    for id in ids {
                                        if let Err(e) = record_memory_access_sync(conn, id, &sid_clone) {
                                            tracing::debug!("Failed to record memory access: {}", e);
                                        }
                                    }
                                    Ok::<_, anyhow::Error>(())
                                })
                                .await;
                        });
                    }

                    let mut response =
                        format!("{}Found {} memories:\n", context_header, results.len());
                    for (id, content, distance) in results {
                        let score = 1.0 - distance; // Convert distance to similarity
                        let preview = if content.len() > 100 {
                            format!("{}...", &content[..100])
                        } else {
                            content
                        };
                        response
                            .push_str(&format!("  [{}] (score: {:.2}) {}\n", id, score, preview));
                    }
                    return Ok(response);
                }
            }
        }
    }

    // Fall back to SQL search via connection pool
    let query_clone = query.clone();
    let user_id_clone = user_id.clone();
    let results: Vec<MemoryFact> = ctx
        .pool()
        .interact(move |conn| {
            search_memories_sync(conn, project_id, &query_clone, limit, user_id_clone.as_deref())
                .map_err(|e| anyhow::anyhow!(e))
        })
        .await
        .map_err(|e| e.to_string())?;

    if results.is_empty() {
        return Ok(format!("{}No memories found.", context_header));
    }

    // Record memory access for evidence-based tracking
    if let Some(ref sid) = session_id {
        let ids: Vec<i64> = results.iter().map(|m| m.id).collect();
        let pool_clone = ctx.pool().clone();
        let sid_clone = sid.clone();
        // Fire and forget - don't block on this
        tokio::spawn(async move {
            let _ = pool_clone
                .interact(move |conn| {
                    for id in ids {
                        if let Err(e) = record_memory_access_sync(conn, id, &sid_clone) {
                            tracing::debug!("Failed to record memory access: {}", e);
                        }
                    }
                    Ok::<_, anyhow::Error>(())
                })
                .await;
        });
    }

    let mut response = format!("{}Found {} memories:\n", context_header, results.len());
    for mem in results {
        let preview = if mem.content.len() > 100 {
            format!("{}...", &mem.content[..100])
        } else {
            mem.content.clone()
        };
        response.push_str(&format!(
            "  [{}] ({}) {}\n",
            mem.id,
            mem.fact_type,
            preview
        ));
    }

    Ok(response)
}

/// Sync helper: search memories by text (basic SQL LIKE) with scope filtering
/// This version takes a Connection reference for use inside run_blocking
fn search_memories_sync(
    conn: &rusqlite::Connection,
    project_id: Option<i64>,
    query: &str,
    limit: usize,
    user_id: Option<&str>,
) -> Result<Vec<MemoryFact>, String> {
    use rusqlite::params;

    // Escape SQL LIKE wildcards to prevent injection
    let escaped = query
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    let pattern = format!("%{}%", escaped);

    // Scope filtering: project and legacy memories visible to all, personal only to creator
    let mut stmt = conn
        .prepare(
            "SELECT id, project_id, key, content, fact_type, category, confidence, created_at,
                    session_count, first_session_id, last_session_id, status,
                    user_id, scope, team_id
             FROM memory_facts
             WHERE (project_id = ? OR project_id IS NULL)
               AND content LIKE ? ESCAPE '\\'
               AND (
                 scope = 'project'
                 OR scope IS NULL
                 OR (scope = 'personal' AND user_id = ?)
                 OR user_id IS NULL
               )
             ORDER BY updated_at DESC
             LIMIT ?",
        )
        .map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map(params![project_id, pattern, user_id, limit as i64], |row| {
            Ok(MemoryFact {
                id: row.get(0)?,
                project_id: row.get(1)?,
                key: row.get(2)?,
                content: row.get(3)?,
                fact_type: row.get(4)?,
                category: row.get(5)?,
                confidence: row.get(6)?,
                created_at: row.get(7)?,
                session_count: row.get(8).unwrap_or(1),
                first_session_id: row.get(9).ok(),
                last_session_id: row.get(10).ok(),
                status: row.get(11).unwrap_or_else(|_| "candidate".to_string()),
                user_id: row.get(12).ok(),
                scope: row.get(13).unwrap_or_else(|_| "project".to_string()),
                team_id: row.get(14).ok(),
            })
        })
        .map_err(|e| e.to_string())?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

/// Sync helper: record that a memory was accessed in a session
/// This version takes a Connection reference for use inside run_blocking
fn record_memory_access_sync(
    conn: &rusqlite::Connection,
    memory_id: i64,
    session_id: &str,
) -> Result<(), String> {
    use rusqlite::params;

    // Get current session info
    let current: Option<String> = conn
        .query_row(
            "SELECT last_session_id FROM memory_facts WHERE id = ?",
            [memory_id],
            |row| row.get(0),
        )
        .ok()
        .flatten();

    // Only increment if this is a new session
    if current.as_deref() != Some(session_id) {
        conn.execute(
            "UPDATE memory_facts SET session_count = session_count + 1, last_session_id = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
            params![session_id, memory_id],
        )
        .map_err(|e| e.to_string())?;

        // Check for promotion
        conn.execute(
            "UPDATE memory_facts SET status = 'confirmed', confidence = MIN(confidence + 0.2, 1.0)
             WHERE id = ? AND status = 'candidate' AND session_count >= 3",
            [memory_id],
        )
        .map_err(|e| e.to_string())?;
    }

    Ok(())
}

/// Delete a memory
pub async fn forget<C: ToolContext>(ctx: &C, id: String) -> Result<String, String> {
    let id: i64 = id.parse().map_err(|_| "Invalid ID format".to_string())?;
    if id <= 0 {
        return Err("Invalid memory ID: must be positive".to_string());
    }

    // Delete from both SQL and vector table via connection pool
    let deleted = ctx
        .pool()
        .interact(move |conn| {
            use rusqlite::params;
            // Delete from vector table first
            conn.execute("DELETE FROM vec_memory WHERE fact_id = ?", params![id])?;
            // Delete from facts table
            let deleted = conn.execute("DELETE FROM memory_facts WHERE id = ?", params![id])? > 0;
            Ok(deleted)
        })
        .await
        .map_err(|e| e.to_string())?;

    if deleted {
        Ok(format!("Memory {} deleted.", id))
    } else {
        Ok(format!("Memory {} not found.", id))
    }
}
