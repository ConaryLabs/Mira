// src/mcp/tools/memory.rs
// Memory tools: remember, recall, forget

use crate::mcp::MiraServer;
use rusqlite::params;
use zerocopy::AsBytes;

/// Store a memory fact
pub async fn remember(
    server: &MiraServer,
    content: String,
    key: Option<String>,
    fact_type: Option<String>,
    category: Option<String>,
    confidence: Option<f64>,
) -> Result<String, String> {
    let project_id = server
        .project
        .read()
        .await
        .as_ref()
        .map(|p| p.id);

    let fact_type = fact_type.unwrap_or_else(|| "general".to_string());
    let confidence = confidence.unwrap_or(1.0);

    // Store in SQL
    let id = server
        .db
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
    if let Some(embeddings) = &server.embeddings {
        match embeddings.embed(&content).await {
            Ok(embedding) => {
                let conn = server.db.conn();
                // Insert into vec_memory
                let result = conn.execute(
                    "INSERT INTO vec_memory (rowid, embedding, fact_id, content) VALUES (?, ?, ?, ?)",
                    params![
                        id,
                        embedding.as_bytes(),
                        id,
                        &content
                    ],
                );
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

/// Search memories
pub async fn recall(
    server: &MiraServer,
    query: String,
    limit: Option<i64>,
    _category: Option<String>,
    _fact_type: Option<String>,
) -> Result<String, String> {
    let project_id = server
        .project
        .read()
        .await
        .as_ref()
        .map(|p| p.id);

    let limit = limit.unwrap_or(10) as usize;

    // Try semantic search first if embeddings available
    if let Some(embeddings) = &server.embeddings {
        if let Ok(query_embedding) = embeddings.embed(&query).await {
            let conn = server.db.conn();

            // Search vec_memory with project scoping
            // Join with memory_facts to filter by project_id
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
                .query_map(
                    params![query_embedding.as_bytes(), project_id, limit as i64],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .map_err(|e| e.to_string())?
                .filter_map(|r| r.ok())
                .collect();

            if !results.is_empty() {
                let mut response = format!("{} results:\n", results.len());
                for (id, content, distance) in results {
                    let score = 1.0 - distance; // Convert distance to similarity
                    let preview = if content.len() > 100 {
                        format!("{}...", &content[..100])
                    } else {
                        content
                    };
                    response.push_str(&format!("  [{}] (score: {:.2}) {}\n", id, score, preview));
                }
                return Ok(response);
            }
        }
    }

    // Fall back to SQL search
    let results = server
        .db
        .search_memories(project_id, &query, limit)
        .map_err(|e| e.to_string())?;

    if results.is_empty() {
        return Ok("No memories found.".to_string());
    }

    let mut response = format!("{} results:\n", results.len());
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
pub async fn forget(server: &MiraServer, id: String) -> Result<String, String> {
    let id: i64 = id.parse().map_err(|_| "Invalid ID".to_string())?;

    // Delete from SQL
    let deleted = server.db.delete_memory(id).map_err(|e| e.to_string())?;

    // Delete from vector table
    let conn = server.db.conn();
    let _ = conn.execute("DELETE FROM vec_memory WHERE fact_id = ?", [id]);

    if deleted {
        Ok(format!("Memory {} deleted.", id))
    } else {
        Ok(format!("Memory {} not found.", id))
    }
}
