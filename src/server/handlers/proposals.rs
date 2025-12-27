//! Proposal management handler
//!
//! Handles the proactive organization system - extracting goals/tasks/decisions
//! from conversation text and managing their lifecycle.

use anyhow::Result;
use sqlx::SqlitePool;
use serde_json::{json, Value};

use crate::core::ops::proposals;
use crate::core::OpContext;
use crate::tools::types::ProposalRequest;

/// Extract proposals from text using pattern matching
pub async fn extract(db: &SqlitePool, text: &str, base_confidence: f64) -> Result<Value> {
    let ctx = OpContext::new(std::env::current_dir().unwrap_or_default())
        .with_db(db.clone());

    // Pattern-based extraction
    let matches = proposals::extract_from_text(&ctx, text, base_confidence).await?;

    let mut created = Vec::new();
    let mut skipped_dupes = 0;

    if matches.is_empty() {
        return Ok(json!({ "message": "No proposals detected in text." }));
    }

    // Process pattern matches
    for m in matches {
        // Check for duplicate before creating
        if let Ok(Some(existing_id)) = proposals::find_duplicate(&ctx, &m.full_context).await {
            tracing::debug!("Skipping duplicate proposal, matches: {}", existing_id);
            skipped_dupes += 1;
            continue;
        }

        let evidence = json!({
            "pattern_id": m.pattern_id,
            "matched_text": m.matched_text,
            "context": m.full_context,
        });

        let proposal = proposals::create_proposal(
            &ctx,
            m.proposal_type,
            &m.full_context,
            None,
            m.confidence,
            Some(&evidence.to_string()),
            Some("extract"),
            None,
        ).await?;

        created.push(json!({
            "id": proposal.id,
            "type": proposal.proposal_type.to_string(),
            "content": proposal.content,
            "confidence": proposal.confidence,
            "status": proposal.status.to_string(),
        }));
    }

    let mut response = json!({
        "extracted": created.len(),
        "method": "pattern",
        "proposals": created,
    });
    if skipped_dupes > 0 {
        response["skipped_duplicates"] = json!(skipped_dupes);
    }
    Ok(response)
}

/// List proposals with optional filters
pub async fn list(
    db: &SqlitePool,
    status: Option<&str>,
    proposal_type: Option<&str>,
    limit: i64,
) -> Result<Value> {
    let ctx = OpContext::new(std::env::current_dir().unwrap_or_default())
        .with_db(db.clone());

    let status = status.and_then(|s| s.parse().ok());
    let ptype = proposal_type.and_then(|t| t.parse().ok());

    let props = proposals::list_proposals(&ctx, status, ptype, limit).await?;

    if props.is_empty() {
        return Ok(json!({ "message": "No proposals found." }));
    }

    let results: Vec<_> = props.iter().map(|p| json!({
        "id": p.id,
        "type": p.proposal_type.to_string(),
        "content": if p.content.len() > 100 { format!("{}...", &p.content[..100]) } else { p.content.clone() },
        "confidence": p.confidence,
        "status": p.status.to_string(),
    })).collect();

    Ok(json!({ "proposals": results }))
}

/// Confirm a proposal (convert to goal/task/decision)
pub async fn confirm(db: &SqlitePool, proposal_id: &str) -> Result<Value> {
    let ctx = OpContext::new(std::env::current_dir().unwrap_or_default())
        .with_db(db.clone());

    match proposals::confirm_proposal(&ctx, proposal_id).await? {
        Some(msg) => Ok(json!({ "message": msg })),
        None => Ok(json!({ "message": format!("Proposal {} not found", proposal_id) })),
    }
}

/// Reject a proposal
pub async fn reject(db: &SqlitePool, proposal_id: &str) -> Result<Value> {
    let ctx = OpContext::new(std::env::current_dir().unwrap_or_default())
        .with_db(db.clone());

    match proposals::reject_proposal(&ctx, proposal_id).await? {
        Some(msg) => Ok(json!({ "message": msg })),
        None => Ok(json!({ "message": format!("Proposal {} not found or already processed", proposal_id) })),
    }
}

/// Get pending proposals for batch review
pub async fn review(db: &SqlitePool, limit: i64) -> Result<Value> {
    let ctx = OpContext::new(std::env::current_dir().unwrap_or_default())
        .with_db(db.clone());

    let pending = proposals::get_pending_review(&ctx, limit).await?;

    if pending.is_empty() {
        return Ok(json!({ "message": "No pending proposals to review." }));
    }

    let results: Vec<_> = pending.iter().map(|p| json!({
        "id": p.id,
        "type": p.proposal_type.to_string(),
        "content": if p.content.len() > 80 { format!("{}...", &p.content[..80]) } else { p.content.clone() },
        "confidence": p.confidence,
    })).collect();

    Ok(json!({
        "count": pending.len(),
        "pending": results,
        "hint": "Use confirm/reject with proposal_id to process."
    }))
}

/// Route a proposal request to the appropriate handler
pub async fn handle(db: &SqlitePool, req: ProposalRequest) -> Result<Value> {
    match req.action.as_str() {
        "extract" => {
            let text = req.text.ok_or_else(|| anyhow::anyhow!("text required for extract"))?;
            let base_confidence = req.base_confidence.unwrap_or(0.5);
            extract(db, &text, base_confidence).await
        }
        "list" => {
            let limit = req.limit.unwrap_or(20);
            list(db, req.status.as_deref(), req.proposal_type.as_deref(), limit).await
        }
        "confirm" => {
            let proposal_id = req.proposal_id.ok_or_else(|| anyhow::anyhow!("proposal_id required"))?;
            confirm(db, &proposal_id).await
        }
        "reject" => {
            let proposal_id = req.proposal_id.ok_or_else(|| anyhow::anyhow!("proposal_id required"))?;
            reject(db, &proposal_id).await
        }
        "review" => {
            let limit = req.limit.unwrap_or(10);
            review(db, limit).await
        }
        action => Ok(json!({
            "error": format!("Unknown action: {}. Use extract/list/confirm/reject/review", action)
        })),
    }
}
