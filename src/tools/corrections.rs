// src/tools/corrections.rs
// Correction tracking - thin wrapper delegating to core::ops::mira
//
// Keeps MCP-specific types separate from the shared core.

use sqlx::sqlite::SqlitePool;
use std::sync::Arc;

use crate::core::ops::mira as core_mira;
use crate::core::OpContext;
use crate::core::SemanticSearch;

// Re-export param structs for backwards compatibility
pub struct RecordCorrectionParams {
    pub correction_type: String,
    pub what_was_wrong: String,
    pub what_is_right: String,
    pub rationale: Option<String>,
    pub scope: Option<String>,
    pub keywords: Option<String>,
}

pub struct GetCorrectionsParams {
    pub file_path: Option<String>,
    pub topic: Option<String>,
    pub correction_type: Option<String>,
    pub context: Option<String>,
    pub limit: Option<i64>,
}

pub struct ListCorrectionsParams {
    pub correction_type: Option<String>,
    pub scope: Option<String>,
    pub status: Option<String>,
    pub limit: Option<i64>,
}

/// Record a new correction when user corrects Claude's approach
pub async fn record_correction(
    db: &SqlitePool,
    semantic: &Arc<SemanticSearch>,
    req: RecordCorrectionParams,
    project_id: Option<i64>,
) -> anyhow::Result<serde_json::Value> {
    let ctx = OpContext::new(std::env::current_dir().unwrap_or_default())
        .with_db(db.clone())
        .with_semantic(semantic.clone());

    let input = core_mira::RecordCorrectionInput {
        correction_type: req.correction_type,
        what_was_wrong: req.what_was_wrong,
        what_is_right: req.what_is_right,
        rationale: req.rationale,
        scope: req.scope,
        keywords: req.keywords,
        project_id,
    };

    let output = core_mira::record_correction(&ctx, input).await?;

    Ok(serde_json::json!({
        "status": "recorded",
        "correction_id": output.correction_id,
        "correction_type": output.correction_type,
        "scope": output.scope,
    }))
}

/// Get corrections relevant to current context (file, topic, keywords)
/// Note: This is a simplified version - semantic matching happens in list_corrections
pub async fn get_corrections(
    db: &SqlitePool,
    semantic: &Arc<SemanticSearch>,
    req: GetCorrectionsParams,
    project_id: Option<i64>,
) -> anyhow::Result<Vec<serde_json::Value>> {
    let ctx = OpContext::new(std::env::current_dir().unwrap_or_default())
        .with_db(db.clone())
        .with_semantic(semantic.clone());

    let input = core_mira::ListCorrectionsInput {
        correction_type: req.correction_type,
        scope: None,
        status: Some("active".to_string()),
        limit: req.limit.unwrap_or(10),
        project_id,
    };

    let corrections = core_mira::list_corrections(&ctx, input).await?;

    Ok(corrections
        .into_iter()
        .map(|c| {
            serde_json::json!({
                "id": c.id,
                "correction_type": c.correction_type,
                "what_was_wrong": c.what_was_wrong,
                "what_is_right": c.what_is_right,
                "rationale": c.rationale,
                "scope": c.scope,
                "confidence": c.confidence,
                "times_applied": c.times_applied,
                "times_validated": c.times_validated,
            })
        })
        .collect())
}

/// Validate a correction (mark as helpful or not)
pub async fn validate_correction(
    db: &SqlitePool,
    correction_id: &str,
    outcome: &str,
) -> anyhow::Result<serde_json::Value> {
    let ctx = OpContext::new(std::env::current_dir().unwrap_or_default()).with_db(db.clone());

    core_mira::validate_correction(&ctx, correction_id, outcome).await?;

    Ok(serde_json::json!({
        "status": "recorded",
        "correction_id": correction_id,
        "outcome": outcome,
    }))
}

/// List all corrections for a project
pub async fn list_corrections(
    db: &SqlitePool,
    req: ListCorrectionsParams,
    project_id: Option<i64>,
) -> anyhow::Result<Vec<serde_json::Value>> {
    let ctx = OpContext::new(std::env::current_dir().unwrap_or_default()).with_db(db.clone());

    let input = core_mira::ListCorrectionsInput {
        correction_type: req.correction_type,
        scope: req.scope,
        status: req.status,
        limit: req.limit.unwrap_or(20),
        project_id,
    };

    let corrections = core_mira::list_corrections(&ctx, input).await?;

    Ok(corrections
        .into_iter()
        .map(|c| {
            serde_json::json!({
                "id": c.id,
                "correction_type": c.correction_type,
                "what_was_wrong": c.what_was_wrong,
                "what_is_right": c.what_is_right,
                "rationale": c.rationale,
                "scope": c.scope,
                "confidence": c.confidence,
                "times_applied": c.times_applied,
                "times_validated": c.times_validated,
                "created_at": c.created_at,
            })
        })
        .collect())
}
