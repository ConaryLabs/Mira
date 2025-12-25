//! Rejected approach operations - record and retrieve failed approaches

use crate::core::{CoreError, CoreResult, OpContext};
use chrono::Utc;
use uuid::Uuid;

use super::mira::normalize_json_array;

/// Input for recording a rejected approach
#[derive(Debug, Clone)]
pub struct RecordRejectedApproachInput {
    pub problem_context: String,
    pub approach: String,
    pub rejection_reason: String,
    pub related_files: Option<String>,
    pub related_topics: Option<String>,
    pub project_id: Option<i64>,
}

/// Result of recording a rejected approach
#[derive(Debug, Clone)]
pub struct RecordRejectedApproachOutput {
    pub id: String,
    pub problem_context: String,
    pub approach: String,
}

/// Input for getting rejected approaches
#[derive(Debug, Clone, Default)]
pub struct GetRejectedApproachesInput {
    pub task_context: Option<String>,
    pub project_id: Option<i64>,
    pub limit: i64,
}

/// A rejected approach to avoid
#[derive(Debug, Clone)]
pub struct RejectedApproach {
    pub id: String,
    pub problem_context: String,
    pub approach: String,
    pub rejection_reason: String,
    pub related_files: Option<String>,
    pub related_topics: Option<String>,
    pub created_at: String,
}

/// Get rejected approaches, optionally filtered by task context
pub async fn get_rejected_approaches(
    ctx: &OpContext,
    input: GetRejectedApproachesInput,
) -> CoreResult<Vec<RejectedApproach>> {
    let db = ctx.require_db()?;
    let project_filter = input.project_id.unwrap_or(0);

    let query = r#"
        SELECT id, problem_context, approach, rejection_reason, related_files, related_topics,
               datetime(created_at, 'unixepoch', 'localtime') as created_at
        FROM rejected_approaches
        WHERE project_id = $1 OR project_id IS NULL
        ORDER BY created_at DESC
        LIMIT $2
    "#;

    let rows = sqlx::query_as::<_, (String, String, String, String, Option<String>, Option<String>, String)>(query)
        .bind(project_filter)
        .bind(input.limit)
        .fetch_all(db)
        .await?;

    let approaches: Vec<RejectedApproach> = rows.into_iter()
        .map(|(id, problem_context, approach, rejection_reason, related_files, related_topics, created_at)| {
            RejectedApproach {
                id,
                problem_context,
                approach,
                rejection_reason,
                related_files,
                related_topics,
                created_at,
            }
        })
        .collect();

    // Filter to relevant ones if task context provided
    if let Some(ref task_context) = input.task_context {
        let task_lower = task_context.to_lowercase();
        let filtered: Vec<RejectedApproach> = approaches.into_iter()
            .filter(|r| {
                let ctx_lower = r.problem_context.to_lowercase();
                // Check for any word overlap (words > 3 chars)
                task_lower.split_whitespace()
                    .any(|word| word.len() > 3 && ctx_lower.contains(word))
            })
            .collect();
        return Ok(filtered);
    }

    Ok(approaches)
}

/// Record a rejected approach
pub async fn record_rejected_approach(
    ctx: &OpContext,
    input: RecordRejectedApproachInput,
) -> CoreResult<RecordRejectedApproachOutput> {
    if input.problem_context.is_empty() {
        return Err(CoreError::MissingField("problem_context"));
    }
    if input.approach.is_empty() {
        return Err(CoreError::MissingField("approach"));
    }
    if input.rejection_reason.is_empty() {
        return Err(CoreError::MissingField("rejection_reason"));
    }

    let db = ctx.require_db()?;
    let now = Utc::now().timestamp();
    let id = format!(
        "rej-{}",
        Uuid::new_v4().to_string().split('-').next().unwrap()
    );

    let related_files = normalize_json_array(&input.related_files);
    let related_topics = normalize_json_array(&input.related_topics);

    sqlx::query(
        r#"
        INSERT INTO rejected_approaches (id, project_id, problem_context, approach, rejection_reason,
                                        related_files, related_topics, created_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        "#,
    )
    .bind(&id)
    .bind(input.project_id)
    .bind(&input.problem_context)
    .bind(&input.approach)
    .bind(&input.rejection_reason)
    .bind(&related_files)
    .bind(&related_topics)
    .bind(now)
    .execute(db)
    .await?;

    // Store in semantic search
    if let Some(semantic) = &ctx.semantic {
        if semantic.is_available() {
            use crate::core::primitives::semantic::COLLECTION_CONVERSATION;
            use crate::core::primitives::semantic_helpers::{store_with_logging, MetadataBuilder};

            let content = format!(
                "Rejected approach for {}: {} - Reason: {}",
                input.problem_context, input.approach, input.rejection_reason
            );
            let metadata = MetadataBuilder::new("rejected_approach")
                .string("id", &id)
                .project_id(input.project_id)
                .build();
            store_with_logging(semantic, COLLECTION_CONVERSATION, &id, &content, metadata).await;
        }
    }

    Ok(RecordRejectedApproachOutput {
        id,
        problem_context: input.problem_context,
        approach: input.approach,
    })
}
