//! Unified memory tools (recall, remember, forget)

use crate::search::{embedding_to_bytes, format_project_header};
use crate::tools::core::ToolContext;

/// Store a memory fact
pub async fn remember<C: ToolContext>(
    ctx: &C,
    content: String,
    key: Option<String>,
    fact_type: Option<String>,
    category: Option<String>,
    confidence: Option<f64>,
) -> Result<String, String> {
    let project_id = ctx.project_id().await;

    let fact_type = fact_type.unwrap_or_else(|| "general".to_string());
    let confidence = confidence.unwrap_or(1.0);

    // Store in SQL
    let id = ctx
        .db()
        .store_memory(
            project_id,
            key.as_deref(),
            &content,
            &fact_type,
            category.as_deref(),
            confidence,
        )
        .map_err(|e| e.to_string())?;

    // Store embedding if available
    if let Some(embeddings) = ctx.embeddings() {
        match embeddings.embed(&content).await {
            Ok(embedding) => {
                // Insert into vec_memory on blocking thread pool
                let db_clone = ctx.db().clone();
                let embedding_bytes = embedding_to_bytes(&embedding);
                let content_clone = content.clone();
                let result = crate::db::Database::run_blocking(db_clone, move |conn| {
                    use rusqlite::params;
                    conn.execute(
                        "INSERT INTO vec_memory (rowid, embedding, fact_id, content) VALUES (?, ?, ?, ?)",
                        params![id, embedding_bytes, id, &content_clone],
                    )
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
    let project = ctx.get_project().await;
    let context_header = format_project_header(project.as_ref());

    let limit = limit.unwrap_or(10) as usize;

    // Try semantic search first if embeddings available
    if let Some(embeddings) = ctx.embeddings() {
        if let Ok(query_embedding) = embeddings.embed(&query).await {
            let embedding_bytes = embedding_to_bytes(&query_embedding);

            // Run vector search on blocking thread pool
            let db_clone = ctx.db().clone();
            let results: Result<Vec<(i64, String, f32)>, String> =
                crate::db::Database::run_blocking(db_clone, move |conn| {
                    use rusqlite::params;
                    let mut stmt = conn
                        .prepare(
                            "SELECT v.fact_id, v.content, vec_distance_cosine(v.embedding, ?1) as distance
                             FROM vec_memory v
                             JOIN memory_facts f ON v.fact_id = f.id
                             WHERE (f.project_id = ?2 OR f.project_id IS NULL OR ?2 IS NULL)
                             ORDER BY distance
                             LIMIT ?3",
                        )
                        .map_err(|e| e.to_string())?;

                    let results: Vec<(i64, String, f32)> = stmt
                        .query_map(params![embedding_bytes, project_id, limit as i64], |row| {
                            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
                        })
                        .map_err(|e| e.to_string())?
                        .filter_map(|r| r.ok())
                        .collect();

                    Ok(results)
                })
                .await;

            if let Ok(results) = results {
                if !results.is_empty() {
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

    // Fall back to SQL search
    let results = ctx
        .db()
        .search_memories(project_id, &query, limit)
        .map_err(|e| e.to_string())?;

    if results.is_empty() {
        return Ok(format!("{}No memories found.", context_header));
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

/// Delete a memory
pub async fn forget<C: ToolContext>(ctx: &C, id: String) -> Result<String, String> {
    let id: i64 = id.parse().map_err(|_| "Invalid ID".to_string())?;

    // Delete from both SQL and vector table on blocking thread pool
    let db_clone = ctx.db().clone();
    let deleted = crate::db::Database::run_blocking(db_clone, move |conn| {
        use rusqlite::params;
        // Delete from vector table first
        let _ = conn.execute("DELETE FROM vec_memory WHERE fact_id = ?", params![id]);
        // Delete from facts table
        conn.execute("DELETE FROM memory_facts WHERE id = ?", params![id])
            .map(|n| n > 0)
            .unwrap_or(false)
    })
    .await;

    if deleted {
        Ok(format!("Memory {} deleted.", id))
    } else {
        Ok(format!("Memory {} not found.", id))
    }
}
