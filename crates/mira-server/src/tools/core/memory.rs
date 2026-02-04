//! Unified memory tools (recall, remember, forget)

use crate::db::{StoreMemoryParams, store_embedding_sync, store_memory_sync};
use crate::mcp::responses::Json;
use crate::mcp::responses::{MemoryData, MemoryItem, MemoryOutput, RecallData, RememberData};
use crate::search::{embedding_to_bytes, format_project_header};
use crate::tools::core::ToolContext;
use mira_types::MemoryFact;
use regex::Regex;
use std::sync::LazyLock;

/// Patterns that look like secrets/credentials.
/// Each tuple is (description, regex).
#[allow(clippy::expect_used)] // Static regex patterns are compile-time known; panic on invalid regex is correct
static SECRET_PATTERNS: LazyLock<Vec<(&str, Regex)>> = LazyLock::new(|| {
    vec![
        (
            "API key",
            Regex::new(r"(?i)(sk-[a-zA-Z0-9]{20,}|api[_-]?key\s*[:=]\s*\S{10,})")
                .expect("valid regex"),
        ),
        (
            "AWS key",
            Regex::new(r"AKIA[0-9A-Z]{16}").expect("valid regex"),
        ),
        (
            "Private key",
            Regex::new(r"-----BEGIN (RSA |EC |OPENSSH )?PRIVATE KEY-----").expect("valid regex"),
        ),
        (
            "Bearer token",
            Regex::new(r"(?i)bearer\s+[a-zA-Z0-9_\-.]{20,}").expect("valid regex"),
        ),
        (
            "Password assignment",
            Regex::new(r"(?i)(password|passwd|pwd)\s*[:=]\s*\S{6,}").expect("valid regex"),
        ),
        (
            "GitHub token",
            Regex::new(r"gh[pousr]_[A-Za-z0-9_]{36,}").expect("valid regex"),
        ),
        (
            "Generic secret",
            Regex::new(r#"(?i)(secret|token)\s*[:=]\s*['"]?[a-zA-Z0-9_\-/.]{20,}"#)
                .expect("valid regex"),
        ),
    ]
});

/// Check if content looks like it contains secrets.
/// Returns the name of the first matched pattern, or None.
fn detect_secret(content: &str) -> Option<&'static str> {
    for (name, pattern) in SECRET_PATTERNS.iter() {
        if pattern.is_match(content) {
            return Some(name);
        }
    }
    None
}

/// Fire-and-forget recording of memory access for evidence-based tracking.
/// Spawns a background task that records each memory ID as accessed in the given session.
fn spawn_record_access(
    pool: std::sync::Arc<crate::db::pool::DatabasePool>,
    ids: Vec<i64>,
    session_id: String,
) {
    use crate::db::record_memory_access_sync;
    tokio::spawn(async move {
        if let Err(e) = pool
            .interact(move |conn| {
                for id in ids {
                    if let Err(e) = record_memory_access_sync(conn, id, &session_id) {
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

/// Unified memory tool dispatcher
pub async fn handle_memory<C: ToolContext>(
    ctx: &C,
    req: crate::mcp::requests::MemoryRequest,
) -> Result<Json<MemoryOutput>, String> {
    use crate::mcp::requests::MemoryAction;
    match req.action {
        MemoryAction::Remember => {
            let content = req
                .content
                .ok_or("content is required for action 'remember'")?;
            remember(
                ctx,
                content,
                req.key,
                req.fact_type,
                req.category,
                req.confidence,
                req.scope,
            )
            .await
        }
        MemoryAction::Recall => {
            let query = req.query.ok_or("query is required for action 'recall'")?;
            recall(ctx, query, req.limit, req.category, req.fact_type).await
        }
        MemoryAction::Forget => {
            let id = req.id.ok_or("id is required for action 'forget'")?;
            forget(ctx, id).await
        }
    }
}

/// Store a memory fact
pub async fn remember<C: ToolContext>(
    ctx: &C,
    content: String,
    key: Option<String>,
    fact_type: Option<String>,
    category: Option<String>,
    confidence: Option<f64>,
    scope: Option<String>,
) -> Result<Json<MemoryOutput>, String> {
    // Security: warn if content looks like it contains secrets
    if let Some(pattern_name) = detect_secret(&content) {
        return Err(format!(
            "Content appears to contain a secret ({pattern_name}). \
             Secrets should be stored in ~/.mira/.env, not in memories. \
             If this is a false positive, rephrase the content to avoid secret-like patterns."
        ));
    }

    let project_id = ctx.project_id().await;
    let session_id = ctx.get_session_id().await;
    let user_id = ctx.get_user_identity();

    let fact_type = fact_type.unwrap_or_else(|| "general".to_string());
    let confidence = confidence.unwrap_or(0.5); // Start with lower confidence for evidence-based system
    // Default scope is "project" for backward compatibility
    let scope = scope.unwrap_or_else(|| "project".to_string());

    // Validate scope
    if !["personal", "project", "team"].contains(&scope.as_str()) {
        return Err(format!(
            "Invalid scope '{}'. Must be one of: personal, project, team",
            scope
        ));
    }

    // Personal scope requires user_id
    if scope == "personal" && user_id.is_none() {
        return Err("Cannot create personal memory: user identity not available".to_string());
    }

    // Get current branch for branch-aware memory
    let branch = ctx.get_branch().await;

    // Store in SQL with session tracking via connection pool
    let content_for_store = content.clone();
    let key_for_store = key.clone();
    let category_for_store = category.clone();
    let fact_type_for_store = fact_type.clone();
    let session_id_for_store = session_id.clone();
    let user_id_for_store = user_id.clone();
    let scope_for_store = scope.clone();
    let branch_for_store = branch.clone();
    let id: i64 = ctx
        .pool()
        .run(move |conn| {
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
                branch: branch_for_store.as_deref(),
            };
            store_memory_sync(conn, params)
        })
        .await?;

    // Store embedding if available (using RETRIEVAL_DOCUMENT task type for storage)
    if let Some(embeddings) = ctx.embeddings() {
        match embeddings.embed_for_storage(&content).await {
            Ok(embedding) => {
                let embedding_bytes = embedding_to_bytes(&embedding);
                let content_clone = content.clone();
                let result = ctx
                    .pool()
                    .run(move |conn| {
                        store_embedding_sync(conn, id, &content_clone, &embedding_bytes)
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

    // Extract and link entities in a separate transaction
    // If this fails, the fact is still stored — backfill will pick it up later
    {
        use crate::db::entities::{
            link_entity_to_fact_sync, mark_fact_has_entities_sync, upsert_entity_sync,
        };
        use crate::entities::extract_entities_heuristic;

        let entities = extract_entities_heuristic(&content);
        let pool_for_entities = ctx.pool().clone();
        if let Err(e) = pool_for_entities
            .run(move |conn| {
                let tx = conn.unchecked_transaction()?;
                for entity in &entities {
                    let entity_id = upsert_entity_sync(
                        &tx,
                        project_id,
                        &entity.canonical_name,
                        entity.entity_type.as_str(),
                        &entity.name,
                    )?;
                    link_entity_to_fact_sync(&tx, id, entity_id)?;
                }
                // Always mark as processed, even if zero entities found
                mark_fact_has_entities_sync(&tx, id)?;
                tx.commit()?;
                Ok::<(), rusqlite::Error>(())
            })
            .await
        {
            tracing::warn!("Entity extraction failed for fact {}: {}", id, e);
        }
    }

    if let Some(cache) = ctx.fuzzy_cache() {
        cache.invalidate_memory(project_id).await;
    }

    Ok(Json(MemoryOutput {
        action: "remember".into(),
        message: format!(
            "Stored memory (id: {}){}",
            id,
            if key.is_some() { " with key" } else { "" }
        ),
        data: Some(MemoryData::Remember(RememberData { id })),
    }))
}

/// Search memories using semantic similarity or keyword fallback
pub async fn recall<C: ToolContext>(
    ctx: &C,
    query: String,
    limit: Option<i64>,
    _category: Option<String>,
    _fact_type: Option<String>,
) -> Result<Json<MemoryOutput>, String> {
    use crate::db::search_memories_sync;

    let project_id = ctx.project_id().await;
    let session_id = ctx.get_session_id().await;
    let project = ctx.get_project().await;
    let user_id = ctx.get_user_identity();
    let current_branch = ctx.get_branch().await;
    let context_header = format_project_header(project.as_ref());

    let limit = limit.unwrap_or(10) as usize;

    // Extract entities from query for entity-based recall boost
    let query_entity_names: Vec<String> = {
        use crate::entities::extract_entities_heuristic;
        extract_entities_heuristic(&query)
            .into_iter()
            .map(|e| e.canonical_name)
            .collect()
    };

    // Try semantic search first if embeddings available (with branch-aware + entity boosting)
    // Uses RETRIEVAL_QUERY task type for optimal search results
    if let Some(embeddings) = ctx.embeddings() {
        if let Ok(query_embedding) = embeddings.embed_for_query(&query).await {
            let embedding_bytes = embedding_to_bytes(&query_embedding);
            let user_id_for_query = user_id.clone();
            let branch_for_query = current_branch.clone();
            let entity_names_for_query = query_entity_names.clone();

            // Run vector search via connection pool with branch + entity boosting
            let results: Vec<(i64, String, f32, Option<String>)> = ctx
                .pool()
                .run(move |conn| {
                    crate::db::recall_semantic_with_entity_boost_sync(
                        conn,
                        &embedding_bytes,
                        project_id,
                        user_id_for_query.as_deref(),
                        branch_for_query.as_deref(),
                        &entity_names_for_query,
                        limit,
                    )
                })
                .await?;

            if !results.is_empty() {
                // Record memory access for evidence-based tracking
                if let Some(ref sid) = session_id {
                    let ids: Vec<i64> = results.iter().map(|(id, _, _, _)| *id).collect();
                    spawn_record_access(ctx.pool().clone(), ids, sid.clone());
                }

                let items: Vec<MemoryItem> = results
                    .iter()
                    .map(|(id, content, distance, branch)| MemoryItem {
                        id: *id,
                        content: content.clone(),
                        score: Some(1.0 - distance),
                        fact_type: None,
                        branch: branch.clone(),
                    })
                    .collect();
                let total = items.len();
                let mut response = format!("{}Found {} memories:\n", context_header, total);
                for (id, content, distance, branch) in &results {
                    let score = 1.0 - distance;
                    let preview = if content.len() > 100 {
                        format!("{}...", &content[..100])
                    } else {
                        content.clone()
                    };
                    let branch_tag = branch
                        .as_ref()
                        .map(|b| format!(" [{}]", b))
                        .unwrap_or_default();
                    response.push_str(&format!(
                        "  [{}] (score: {:.2}){} {}\n",
                        id, score, branch_tag, preview
                    ));
                }
                return Ok(Json(MemoryOutput {
                    action: "recall".into(),
                    message: response,
                    data: Some(MemoryData::Recall(RecallData {
                        memories: items,
                        total,
                    })),
                }));
            }
        }
    }

    // Fall back to fuzzy search if enabled
    if let Some(cache) = ctx.fuzzy_cache() {
        if let Ok(results) = cache
            .search_memories(ctx.pool(), project_id, user_id.as_deref(), &query, limit)
            .await
        {
            if !results.is_empty() {
                // Record memory access for evidence-based tracking
                if let Some(ref sid) = session_id {
                    let ids: Vec<i64> = results.iter().map(|m| m.id).collect();
                    spawn_record_access(ctx.pool().clone(), ids, sid.clone());
                }

                let items: Vec<MemoryItem> = results
                    .iter()
                    .map(|mem| MemoryItem {
                        id: mem.id,
                        content: mem.content.clone(),
                        score: None,
                        fact_type: Some(mem.fact_type.clone()),
                        branch: None,
                    })
                    .collect();
                let total = items.len();
                let mut response = format!("{}Found {} memories (fuzzy):\n", context_header, total);
                for mem in &results {
                    let preview = if mem.content.len() > 100 {
                        format!("{}...", &mem.content[..100])
                    } else {
                        mem.content.clone()
                    };
                    response.push_str(&format!("  [{}] ({}) {}\n", mem.id, mem.fact_type, preview));
                }
                return Ok(Json(MemoryOutput {
                    action: "recall".into(),
                    message: response,
                    data: Some(MemoryData::Recall(RecallData {
                        memories: items,
                        total,
                    })),
                }));
            }
        }
    }

    // Fall back to SQL search via connection pool
    let query_clone = query.clone();
    let user_id_clone = user_id.clone();
    let results: Vec<MemoryFact> = ctx
        .pool()
        .run(move |conn| {
            search_memories_sync(
                conn,
                project_id,
                &query_clone,
                user_id_clone.as_deref(),
                limit,
            )
        })
        .await?;

    if results.is_empty() {
        return Ok(Json(MemoryOutput {
            action: "recall".into(),
            message: format!("{}No memories found.", context_header),
            data: Some(MemoryData::Recall(RecallData {
                memories: vec![],
                total: 0,
            })),
        }));
    }

    // Record memory access for evidence-based tracking
    if let Some(ref sid) = session_id {
        let ids: Vec<i64> = results.iter().map(|m| m.id).collect();
        spawn_record_access(ctx.pool().clone(), ids, sid.clone());
    }

    let items: Vec<MemoryItem> = results
        .iter()
        .map(|mem| MemoryItem {
            id: mem.id,
            content: mem.content.clone(),
            score: None,
            fact_type: Some(mem.fact_type.clone()),
            branch: None,
        })
        .collect();
    let total = items.len();
    let mut response = format!("{}Found {} memories:\n", context_header, total);
    for mem in &results {
        let preview = if mem.content.len() > 100 {
            format!("{}...", &mem.content[..100])
        } else {
            mem.content.clone()
        };
        response.push_str(&format!("  [{}] ({}) {}\n", mem.id, mem.fact_type, preview));
    }

    Ok(Json(MemoryOutput {
        action: "recall".into(),
        message: response,
        data: Some(MemoryData::Recall(RecallData {
            memories: items,
            total,
        })),
    }))
}

/// Delete a memory
pub async fn forget<C: ToolContext>(ctx: &C, id: String) -> Result<Json<MemoryOutput>, String> {
    use crate::db::delete_memory_sync;

    let id: i64 = id.parse().map_err(|_| "Invalid ID format".to_string())?;
    if id <= 0 {
        return Err("Invalid memory ID: must be positive".to_string());
    }

    // Delete from both SQL and vector table via connection pool
    let deleted = ctx
        .pool()
        .run(move |conn| delete_memory_sync(conn, id))
        .await?;

    if deleted {
        if let Some(cache) = ctx.fuzzy_cache() {
            let project_id = ctx.project_id().await;
            cache.invalidate_memory(project_id).await;
        }
        Ok(Json(MemoryOutput {
            action: "forget".into(),
            message: format!("Memory {} deleted.", id),
            data: None,
        }))
    } else {
        Ok(Json(MemoryOutput {
            action: "forget".into(),
            message: format!("Memory {} not found.", id),
            data: None,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_secret_catches_api_key_prefix() {
        assert_eq!(
            detect_secret("sk-abcdefghijklmnopqrstuvwxyz"),
            Some("API key")
        );
    }

    #[test]
    fn detect_secret_catches_api_key_assignment() {
        assert_eq!(
            detect_secret("api_key = supersecretvalue123"),
            Some("API key")
        );
    }

    #[test]
    fn detect_secret_catches_aws_key() {
        assert_eq!(detect_secret("AKIAIOSFODNN7EXAMPLE"), Some("AWS key"));
    }

    #[test]
    fn detect_secret_catches_private_key() {
        assert_eq!(
            detect_secret("-----BEGIN RSA PRIVATE KEY-----"),
            Some("Private key")
        );
        assert_eq!(
            detect_secret("-----BEGIN PRIVATE KEY-----"),
            Some("Private key")
        );
    }

    #[test]
    fn detect_secret_catches_bearer_token() {
        assert_eq!(
            detect_secret("Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9"),
            Some("Bearer token")
        );
    }

    #[test]
    fn detect_secret_catches_password_assignment() {
        assert_eq!(
            detect_secret("password = hunter2abc"),
            Some("Password assignment")
        );
    }

    #[test]
    fn detect_secret_catches_github_token() {
        assert_eq!(
            detect_secret("ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijkl"),
            Some("GitHub token")
        );
    }

    #[test]
    fn detect_secret_catches_generic_secret() {
        assert_eq!(
            detect_secret("secret = abcdefghijklmnopqrstuvwxyz"),
            Some("Generic secret")
        );
    }

    #[test]
    fn detect_secret_allows_normal_content() {
        assert_eq!(detect_secret("Use the builder pattern for Config"), None);
        assert_eq!(detect_secret("API design uses REST conventions"), None);
        assert_eq!(detect_secret("Remember to check the password field"), None);
    }

    #[test]
    fn detect_secret_allows_short_values() {
        // Too short to trigger password pattern (< 6 chars)
        assert_eq!(detect_secret("pwd = abc"), None);
    }

    #[test]
    fn secret_patterns_static_initializes() {
        // Verify all regex patterns compile without panic
        assert!(!SECRET_PATTERNS.is_empty());
    }
}
