//! Decision storage operations

use crate::core::{CoreError, CoreResult, OpContext};
use chrono::Utc;
use uuid::Uuid;

/// Input for storing a decision
#[derive(Debug, Clone)]
pub struct StoreDecisionInput {
    pub key: String,
    pub decision: String,
    pub category: Option<String>,
    pub context: Option<String>,
    pub project_id: Option<i64>,
}

/// Store an important decision
pub async fn store_decision(ctx: &OpContext, input: StoreDecisionInput) -> CoreResult<()> {
    if input.key.is_empty() {
        return Err(CoreError::MissingField("key"));
    }
    if input.decision.is_empty() {
        return Err(CoreError::MissingField("decision"));
    }

    let db = ctx.require_db()?;
    let now = Utc::now().timestamp();
    let id = Uuid::new_v4().to_string();

    sqlx::query(
        r#"
        INSERT INTO memory_facts (id, fact_type, key, value, category, source, confidence, created_at, updated_at, project_id)
        VALUES ($1, 'decision', $2, $3, $4, $5, 1.0, $6, $6, $7)
        ON CONFLICT(key) DO UPDATE SET
            value = excluded.value,
            project_id = COALESCE(excluded.project_id, memory_facts.project_id),
            updated_at = excluded.updated_at
        "#,
    )
    .bind(&id)
    .bind(&input.key)
    .bind(&input.decision)
    .bind(&input.category)
    .bind(&input.context)
    .bind(now)
    .bind(input.project_id)
    .execute(db)
    .await?;

    // Store in semantic search
    if let Some(semantic) = &ctx.semantic {
        if semantic.is_available() {
            use crate::core::primitives::semantic::COLLECTION_CONVERSATION;
            use crate::core::primitives::semantic_helpers::{store_with_logging, MetadataBuilder};

            let metadata = MetadataBuilder::new("decision")
                .string("key", &input.key)
                .string_opt("category", input.category.as_ref())
                .project_id(input.project_id)
                .build();
            store_with_logging(semantic, COLLECTION_CONVERSATION, &id, &input.decision, metadata)
                .await;
        }
    }

    Ok(())
}
