//! Correction operations - record, list, validate

use crate::core::{CoreError, CoreResult, OpContext};
use chrono::Utc;
use uuid::Uuid;

use super::mira::normalize_json_array;

/// Input for recording a correction
#[derive(Debug, Clone)]
pub struct RecordCorrectionInput {
    pub correction_type: String,
    pub what_was_wrong: String,
    pub what_is_right: String,
    pub rationale: Option<String>,
    pub scope: Option<String>,
    pub keywords: Option<String>,
    pub project_id: Option<i64>,
}

/// Input for listing corrections
#[derive(Debug, Clone, Default)]
pub struct ListCorrectionsInput {
    pub correction_type: Option<String>,
    pub scope: Option<String>,
    pub status: Option<String>,
    pub limit: i64,
    pub project_id: Option<i64>,
}

/// Correction data
#[derive(Debug, Clone)]
pub struct Correction {
    pub id: String,
    pub correction_type: String,
    pub what_was_wrong: String,
    pub what_is_right: String,
    pub rationale: Option<String>,
    pub scope: String,
    pub confidence: f64,
    pub times_applied: i64,
    pub times_validated: i64,
    pub created_at: Option<String>,
}

/// Result of recording a correction
#[derive(Debug, Clone)]
pub struct RecordCorrectionOutput {
    pub correction_id: String,
    pub correction_type: String,
    pub scope: String,
}

/// Record a new correction
pub async fn record_correction(
    ctx: &OpContext,
    input: RecordCorrectionInput,
) -> CoreResult<RecordCorrectionOutput> {
    if input.what_was_wrong.is_empty() {
        return Err(CoreError::MissingField("what_was_wrong"));
    }
    if input.what_is_right.is_empty() {
        return Err(CoreError::MissingField("what_is_right"));
    }

    let db = ctx.require_db()?;
    let now = Utc::now().timestamp();
    let id = Uuid::new_v4().to_string();
    let scope = input.scope.as_deref().unwrap_or("project");
    let keywords = normalize_json_array(&input.keywords);

    sqlx::query(
        r#"
        INSERT INTO corrections (id, correction_type, what_was_wrong, what_is_right, rationale,
                                scope, project_id, keywords, created_at, updated_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $9)
        "#,
    )
    .bind(&id)
    .bind(&input.correction_type)
    .bind(&input.what_was_wrong)
    .bind(&input.what_is_right)
    .bind(&input.rationale)
    .bind(scope)
    .bind(if scope == "global" {
        None
    } else {
        input.project_id
    })
    .bind(&keywords)
    .bind(now)
    .execute(db)
    .await?;

    // Store in semantic search for fuzzy matching
    if let Some(semantic) = &ctx.semantic {
        if semantic.is_available() {
            use crate::core::primitives::semantic::COLLECTION_CONVERSATION;
            use crate::core::primitives::semantic_helpers::{store_with_logging, MetadataBuilder};

            let content = format!(
                "Correction: {} -> {}. Rationale: {}",
                input.what_was_wrong,
                input.what_is_right,
                input.rationale.as_deref().unwrap_or("")
            );
            let metadata = MetadataBuilder::new("correction")
                .string("correction_type", &input.correction_type)
                .string("scope", scope)
                .string("id", &id)
                .project_id(input.project_id)
                .build();
            store_with_logging(semantic, COLLECTION_CONVERSATION, &id, &content, metadata).await;
        }
    }

    Ok(RecordCorrectionOutput {
        correction_id: id,
        correction_type: input.correction_type,
        scope: scope.to_string(),
    })
}

/// List corrections
pub async fn list_corrections(
    ctx: &OpContext,
    input: ListCorrectionsInput,
) -> CoreResult<Vec<Correction>> {
    let db = ctx.require_db()?;
    let status = input.status.as_deref().unwrap_or("active");

    let results = sqlx::query_as::<
        _,
        (
            String,
            String,
            String,
            String,
            Option<String>,
            String,
            f64,
            i64,
            i64,
            String,
        ),
    >(
        r#"
        SELECT id, correction_type, what_was_wrong, what_is_right, rationale, scope,
               confidence, times_applied, times_validated,
               datetime(created_at, 'unixepoch', 'localtime') as created
        FROM corrections
        WHERE status = $1
          AND (project_id IS NULL OR project_id = $2)
          AND ($3 IS NULL OR correction_type = $3)
          AND ($4 IS NULL OR scope = $4)
        ORDER BY created_at DESC
        LIMIT $5
        "#,
    )
    .bind(status)
    .bind(input.project_id)
    .bind(&input.correction_type)
    .bind(&input.scope)
    .bind(input.limit)
    .fetch_all(db)
    .await?;

    Ok(results
        .into_iter()
        .map(
            |(id, ctype, wrong, right, rationale, scope, confidence, applied, validated, created)| {
                Correction {
                    id,
                    correction_type: ctype,
                    what_was_wrong: wrong,
                    what_is_right: right,
                    rationale,
                    scope,
                    confidence,
                    times_applied: applied,
                    times_validated: validated,
                    created_at: Some(created),
                }
            },
        )
        .collect())
}

/// Validate a correction
pub async fn validate_correction(
    ctx: &OpContext,
    correction_id: &str,
    outcome: &str,
) -> CoreResult<bool> {
    if correction_id.is_empty() {
        return Err(CoreError::MissingField("correction_id"));
    }

    let db = ctx.require_db()?;
    let now = Utc::now().timestamp();

    match outcome {
        "validated" => {
            sqlx::query(
                r#"
                UPDATE corrections
                SET times_validated = times_validated + 1,
                    confidence = MIN(1.0, confidence + 0.05),
                    updated_at = $2
                WHERE id = $1
                "#,
            )
            .bind(correction_id)
            .bind(now)
            .execute(db)
            .await?;
        }
        "overridden" => {
            sqlx::query("UPDATE corrections SET updated_at = $2 WHERE id = $1")
                .bind(correction_id)
                .bind(now)
                .execute(db)
                .await?;
        }
        "deprecated" => {
            sqlx::query(
                r#"
                UPDATE corrections
                SET status = 'deprecated', updated_at = $2
                WHERE id = $1
                "#,
            )
            .bind(correction_id)
            .bind(now)
            .execute(db)
            .await?;
        }
        _ => {
            return Err(CoreError::InvalidArgument(format!(
                "Invalid outcome: {}. Use 'validated', 'overridden', or 'deprecated'",
                outcome
            )));
        }
    }

    sqlx::query(
        r#"
        INSERT INTO correction_applications (correction_id, outcome, applied_at)
        VALUES ($1, $2, $3)
        "#,
    )
    .bind(correction_id)
    .bind(outcome)
    .bind(now)
    .execute(db)
    .await?;

    Ok(true)
}
