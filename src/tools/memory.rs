// src/tools/memory.rs
// Memory tools - persistent facts, decisions, preferences across sessions
//
// Uses mira_core::memory for shared logic, keeps only MCP-specific wrappers.

use sqlx::sqlite::SqlitePool;

use super::semantic::{SemanticSearch, COLLECTION_CONVERSATION};
use super::semantic_helpers::{MetadataBuilder, store_with_logging};
use super::types::*;

use mira_core::{make_memory_key, upsert_memory_fact, MemoryScope};

/// Remember a fact, decision, or preference
/// project_id is used for smart scoping:
/// - "preference" fact_type -> always global (project_id = NULL)
/// - Other fact_types -> scoped to project if provided
pub async fn remember(
    db: &SqlitePool,
    semantic: &SemanticSearch,
    req: RememberRequest,
    project_id: Option<i64>,
) -> anyhow::Result<serde_json::Value> {
    let fact_type = req.fact_type.clone().unwrap_or_else(|| "general".to_string());

    // Smart scoping: preferences are always global
    let effective_project_id = if fact_type == "preference" {
        None
    } else {
        project_id
    };

    // Generate key from content if not provided
    let key = req.key.clone().unwrap_or_else(|| make_memory_key(&req.content));

    // Use shared upsert logic
    let scope = match effective_project_id {
        Some(pid) => MemoryScope::ProjectId(pid),
        None => MemoryScope::Global,
    };

    let id = upsert_memory_fact(
        db,
        scope,
        &key,
        &req.content,
        &fact_type,
        req.category.as_deref(),
        "claude-code",
    )
    .await?;

    // Also store in Qdrant for semantic search
    let metadata = MetadataBuilder::new("memory_fact")
        .string("fact_type", &fact_type)
        .string("key", &key)
        .string_opt("category", req.category.as_ref())
        .project_id(effective_project_id)
        .build();
    store_with_logging(semantic, COLLECTION_CONVERSATION, &id, &req.content, metadata).await;

    Ok(serde_json::json!({
        "status": "remembered",
        "key": key,
        "fact_type": fact_type,
        "category": req.category,
        "project_id": effective_project_id,
        "project_scoped": effective_project_id.is_some(),
        "semantic_search": semantic.is_available(),
    }))
}

/// Recall memories matching a query - uses semantic search if available
/// Returns both project-scoped (if project_id provided) AND global memories
///
/// Uses mira_core::recall_memory_facts for shared logic with batch times_used updates.
pub async fn recall(
    db: &SqlitePool,
    semantic: &SemanticSearch,
    req: RecallRequest,
    project_id: Option<i64>,
) -> anyhow::Result<Vec<serde_json::Value>> {
    use mira_core::{recall_memory_facts, RecallConfig};

    let limit = req.limit.unwrap_or(10) as usize;

    let cfg = RecallConfig {
        collection: COLLECTION_CONVERSATION,
        fact_type: req.fact_type.as_deref(),
        category: req.category.as_deref(),
    };

    let facts = recall_memory_facts(db, Some(semantic), cfg, &req.query, limit, project_id).await?;

    Ok(facts
        .into_iter()
        .map(|f| {
            serde_json::json!({
                "id": f.id,
                "key": f.key,
                "value": f.value,
                "fact_type": f.fact_type,
                "category": f.category,
                "project_id": f.project_id,
                "score": f.score,
                "search_type": f.search_type.as_str(),
            })
        })
        .collect())
}

/// Forget (delete) a memory
///
/// Uses mira_core::forget_memory_fact for shared logic.
pub async fn forget(
    db: &SqlitePool,
    semantic: &SemanticSearch,
    req: ForgetRequest,
) -> anyhow::Result<serde_json::Value> {
    use mira_core::forget_memory_fact;

    let deleted = forget_memory_fact(db, Some(semantic), COLLECTION_CONVERSATION, &req.id).await?;

    if deleted {
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
