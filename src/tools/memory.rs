// src/tools/memory.rs
// Memory tools - persistent facts, decisions, preferences across sessions
//
// Thin wrapper that delegates to core::ops::memory for the actual implementation.
// This keeps MCP-specific types separate from the shared core.

use sqlx::sqlite::SqlitePool;
use std::sync::Arc;

use super::semantic::SemanticSearch;
use super::types::*;
use crate::core::ops::memory as core_memory;
use crate::core::OpContext;

/// Remember a fact, decision, or preference
///
/// Delegates to core::ops::memory::remember
pub async fn remember(
    db: &SqlitePool,
    semantic: &Arc<SemanticSearch>,
    req: RememberRequest,
    project_id: Option<i64>,
) -> anyhow::Result<serde_json::Value> {
    // Build OpContext from MCP server state
    let ctx = OpContext::new(std::env::current_dir().unwrap_or_default())
        .with_db(db.clone())
        .with_semantic(semantic.clone());

    // Convert MCP request to core input
    let input = core_memory::RememberInput {
        content: req.content,
        fact_type: req.fact_type,
        category: req.category,
        key: req.key,
        project_id,
        source: "claude-code".to_string(),
    };

    // Call core operation
    let output = core_memory::remember(&ctx, input).await?;

    // Convert to JSON response
    Ok(serde_json::json!({
        "status": "remembered",
        "key": output.key,
        "fact_type": output.fact_type,
        "category": output.category,
        "project_id": output.project_id,
        "project_scoped": output.project_id.is_some(),
        "semantic_search": output.semantic_stored,
    }))
}

/// Recall memories matching a query
///
/// Delegates to core::ops::memory::recall
pub async fn recall(
    db: &SqlitePool,
    semantic: &Arc<SemanticSearch>,
    req: RecallRequest,
    project_id: Option<i64>,
) -> anyhow::Result<Vec<serde_json::Value>> {
    // Build OpContext from MCP server state
    let ctx = OpContext::new(std::env::current_dir().unwrap_or_default())
        .with_db(db.clone())
        .with_semantic(semantic.clone());

    // Convert MCP request to core input
    let input = core_memory::RecallInput {
        query: req.query,
        limit: req.limit.map(|l| l as usize),
        fact_type: req.fact_type,
        category: req.category,
        project_id,
    };

    // Call core operation
    let output = core_memory::recall(&ctx, input).await?;

    // Convert to JSON response
    Ok(output
        .facts
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
/// Delegates to core::ops::memory::forget
pub async fn forget(
    db: &SqlitePool,
    semantic: &Arc<SemanticSearch>,
    req: ForgetRequest,
) -> anyhow::Result<serde_json::Value> {
    // Build OpContext from MCP server state
    let ctx = OpContext::new(std::env::current_dir().unwrap_or_default())
        .with_db(db.clone())
        .with_semantic(semantic.clone());

    // Convert MCP request to core input
    let input = core_memory::ForgetInput { id: req.id };

    // Call core operation
    let output = core_memory::forget(&ctx, input).await?;

    // Convert to JSON response
    if output.deleted {
        Ok(serde_json::json!({
            "status": "forgotten",
            "id": output.id,
        }))
    } else {
        Ok(serde_json::json!({
            "status": "not_found",
            "id": output.id,
        }))
    }
}
