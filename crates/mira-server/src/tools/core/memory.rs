//! Unified memory tools (recall, remember, forget)

use crate::db::{store_memory_sync, store_embedding_sync, StoreMemoryParams};
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
            let params = StoreMemoryParams {
                project_id,
                key: key_for_store.as_deref(),
                content: &content_for_store,
                fact_type: &fact_type_for_store,
                category: category_for_store.as_deref(),
                confidence,
                session_id: session_id_for_store.as_deref(),
                user_id: user_id_for_store.as_deref(),
                scope: &scope_for_store,
            };
            store_memory_sync(conn, params).map_err(|e| anyhow::anyhow!(e))
        })
        .await
        .map_err(|e| e.to_string())?;

    // Store embedding if available
    if let Some(embeddings) = ctx.embeddings() {
        match embeddings.embed(&content).await {
            Ok(embedding) => {
                let embedding_bytes = embedding_to_bytes(&embedding);
                let content_clone = content.clone();
                let result = ctx
                    .pool()
                    .interact(move |conn| {
                        store_embedding_sync(conn, id, &content_clone, &embedding_bytes)
                            .map_err(|e| anyhow::anyhow!(e))
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
    use crate::db::{recall_semantic_sync, search_memories_sync, record_memory_access_sync};

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
            let results: Vec<(i64, String, f32)> = ctx
                .pool()
                .interact(move |conn| {
                    recall_semantic_sync(conn, &embedding_bytes, project_id, user_id_for_query.as_deref(), limit)
                        .map_err(|e| anyhow::anyhow!(e))
                })
                .await
                .map_err(|e| e.to_string())?;

            if !results.is_empty() {
                // Record memory access for evidence-based tracking
                if let Some(ref sid) = session_id {
                    let ids: Vec<i64> = results.iter().map(|(id, _, _)| *id).collect();
                    let pool_clone = ctx.pool().clone();
                    let sid_clone = sid.clone();
                    // Fire and forget - don't block on this
                    tokio::spawn(async move {
                        if let Err(e) = pool_clone
                            .interact(move |conn| {
                                for id in ids {
                                    if let Err(e) = record_memory_access_sync(conn, id, &sid_clone) {
                                        tracing::debug!("Failed to record memory access: {}", e);
                                    }
                                }
                                Ok::<_, anyhow::Error>(())
                            })
                            .await
                        {
                            tracing::debug!("Failed to record memory access (pool error): {}", e);
                        }
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
                    response.push_str(&format!("  [{}] (score: {:.2}) {}\n", id, score, preview));
                }
                return Ok(response);
            }
        }
    }

    // Fall back to SQL search via connection pool
    let query_clone = query.clone();
    let user_id_clone = user_id.clone();
    let results: Vec<MemoryFact> = ctx
        .pool()
        .interact(move |conn| {
            search_memories_sync(conn, project_id, &query_clone, user_id_clone.as_deref(), limit)
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
            if let Err(e) = pool_clone
                .interact(move |conn| {
                    for id in ids {
                        if let Err(e) = record_memory_access_sync(conn, id, &sid_clone) {
                            tracing::debug!("Failed to record memory access: {}", e);
                        }
                    }
                    Ok::<_, anyhow::Error>(())
                })
                .await
            {
                tracing::debug!("Failed to record memory access (pool error): {}", e);
            }
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

/// Delete a memory
pub async fn forget<C: ToolContext>(ctx: &C, id: String) -> Result<String, String> {
    use crate::db::delete_memory_sync;

    let id: i64 = id.parse().map_err(|_| "Invalid ID format".to_string())?;
    if id <= 0 {
        return Err("Invalid memory ID: must be positive".to_string());
    }

    // Delete from both SQL and vector table via connection pool
    let deleted = ctx
        .pool()
        .interact(move |conn| {
            delete_memory_sync(conn, id).map_err(|e| anyhow::anyhow!(e))
        })
        .await
        .map_err(|e| e.to_string())?;

    if deleted {
        Ok(format!("Memory {} deleted.", id))
    } else {
        Ok(format!("Memory {} not found.", id))
    }
}
