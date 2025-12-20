//! Memory operations - remember, recall, forget
//!
//! Unified implementation for storing and retrieving facts, decisions,
//! and preferences. Both MCP and Chat tools call into this module.
//!
//! Uses mira_core::memory for the heavy lifting (DB operations, semantic search).

use crate::core::{CoreError, CoreResult, OpContext};
use mira_core::semantic::COLLECTION_CONVERSATION;
use mira_core::semantic_helpers::{store_with_logging, MetadataBuilder};
use mira_core::{make_memory_key, forget_memory_fact, recall_memory_facts, upsert_memory_fact};
use mira_core::{MemoryFact, MemoryScope, RecallConfig};

// ============================================================================
// Input Types
// ============================================================================

/// Input for remember operation
#[derive(Debug, Clone, Default)]
pub struct RememberInput {
    /// Content to remember (required)
    pub content: String,
    /// Type: preference, decision, context, general
    pub fact_type: Option<String>,
    /// Optional category for organization
    pub category: Option<String>,
    /// Optional key for upsert (auto-generated if not provided)
    pub key: Option<String>,
    /// Project ID for scoping (None = global)
    pub project_id: Option<i64>,
    /// Source identifier (e.g., "claude-code", "mira-chat")
    pub source: String,
}

/// Input for recall operation
#[derive(Debug, Clone, Default)]
pub struct RecallInput {
    /// Search query (required)
    pub query: String,
    /// Max results to return
    pub limit: Option<usize>,
    /// Filter by fact_type
    pub fact_type: Option<String>,
    /// Filter by category
    pub category: Option<String>,
    /// Project ID for scoping (None = search global + all projects)
    pub project_id: Option<i64>,
}

/// Input for forget operation
#[derive(Debug, Clone)]
pub struct ForgetInput {
    /// Memory ID to delete
    pub id: String,
}

// ============================================================================
// Output Types
// ============================================================================

/// Result of remember operation
#[derive(Debug, Clone)]
pub struct RememberOutput {
    pub key: String,
    pub fact_type: String,
    pub category: Option<String>,
    pub project_id: Option<i64>,
    pub semantic_stored: bool,
}

/// Result of recall operation
#[derive(Debug, Clone)]
pub struct RecallOutput {
    pub facts: Vec<MemoryFact>,
    pub search_type: String,
}

/// Result of forget operation
#[derive(Debug, Clone)]
pub struct ForgetOutput {
    pub deleted: bool,
    pub id: String,
}

// ============================================================================
// Operations
// ============================================================================

/// Remember a fact, decision, or preference
///
/// Smart scoping: preferences are always global, other types use project_id if provided.
pub async fn remember(ctx: &OpContext, input: RememberInput) -> CoreResult<RememberOutput> {
    if input.content.is_empty() {
        return Err(CoreError::MissingField("content"));
    }

    let db = ctx.require_db()?;
    let fact_type = input.fact_type.unwrap_or_else(|| "general".to_string());

    // Smart scoping: preferences are always global
    let effective_project_id = if fact_type == "preference" {
        None
    } else {
        input.project_id
    };

    // Generate key from content if not provided
    let key = input.key.unwrap_or_else(|| make_memory_key(&input.content));

    // Determine scope
    let scope = match effective_project_id {
        Some(pid) => MemoryScope::ProjectId(pid),
        None => MemoryScope::Global,
    };

    // Store in database
    let id = upsert_memory_fact(
        db,
        scope,
        &key,
        &input.content,
        &fact_type,
        input.category.as_deref(),
        &input.source,
    )
    .await?;

    // Also store in Qdrant for semantic search
    let mut semantic_stored = false;
    if let Some(semantic) = &ctx.semantic {
        if semantic.is_available() {
            let metadata = MetadataBuilder::new("memory_fact")
                .string("fact_type", &fact_type)
                .string("key", &key)
                .string_opt("category", input.category.as_ref())
                .project_id(effective_project_id)
                .build();

            store_with_logging(
                semantic,
                COLLECTION_CONVERSATION,
                &id,
                &input.content,
                metadata,
            )
            .await;
            semantic_stored = true;
        }
    }

    Ok(RememberOutput {
        key,
        fact_type,
        category: input.category,
        project_id: effective_project_id,
        semantic_stored,
    })
}

/// Recall memories matching a query
///
/// Uses semantic search first (if available), falls back to text search.
/// Returns both project-scoped and global memories.
pub async fn recall(ctx: &OpContext, input: RecallInput) -> CoreResult<RecallOutput> {
    if input.query.is_empty() {
        return Err(CoreError::MissingField("query"));
    }

    let db = ctx.require_db()?;
    let limit = input.limit.unwrap_or(10);

    let cfg = RecallConfig {
        collection: COLLECTION_CONVERSATION,
        fact_type: input.fact_type.as_deref(),
        category: input.category.as_deref(),
    };

    // Get semantic reference if available
    let semantic_ref = ctx.semantic.as_ref().map(|arc| arc.as_ref());

    let facts = recall_memory_facts(db, semantic_ref, cfg, &input.query, limit, input.project_id)
        .await
        .map_err(|e| CoreError::Internal(e.to_string()))?;

    let search_type = facts
        .first()
        .map(|f| f.search_type.as_str())
        .unwrap_or("none")
        .to_string();

    Ok(RecallOutput { facts, search_type })
}

/// Forget (delete) a memory by ID
pub async fn forget(ctx: &OpContext, input: ForgetInput) -> CoreResult<ForgetOutput> {
    if input.id.is_empty() {
        return Err(CoreError::MissingField("id"));
    }

    let db = ctx.require_db()?;
    let semantic_ref = ctx.semantic.as_ref().map(|arc| arc.as_ref());

    let deleted = forget_memory_fact(db, semantic_ref, COLLECTION_CONVERSATION, &input.id)
        .await
        .map_err(|e| CoreError::Internal(e.to_string()))?;

    Ok(ForgetOutput {
        deleted,
        id: input.id,
    })
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_remember_input_defaults() {
        let input = RememberInput {
            content: "test".into(),
            source: "test".into(),
            ..Default::default()
        };
        assert!(input.fact_type.is_none());
        assert!(input.category.is_none());
        assert!(input.project_id.is_none());
    }

    #[test]
    fn test_recall_input_defaults() {
        let input = RecallInput {
            query: "test".into(),
            ..Default::default()
        };
        assert_eq!(input.limit, None);
        assert!(input.fact_type.is_none());
    }
}
