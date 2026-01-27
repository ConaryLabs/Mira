// tools/core/reviews.rs
// MCP tool implementations for code review findings and feedback

use super::ToolContext;
use crate::db::{
    get_findings_sync, get_finding_sync, get_finding_stats_sync,
    update_finding_status_sync, bulk_update_finding_status_sync,
    get_relevant_corrections_sync, extract_patterns_from_findings_sync,
};
use crate::mcp::requests::FindingAction;
use crate::utils::truncate;

/// List review findings with optional filters
pub async fn list_findings<C: ToolContext>(
    ctx: &C,
    status: Option<String>,
    file_path: Option<String>,
    expert_role: Option<String>,
    limit: Option<i64>,
) -> Result<String, String> {
    let project_id = ctx.project_id().await;
    let limit = limit.unwrap_or(20) as usize;

    let status_clone = status.clone();
    let file_path_clone = file_path.clone();
    let expert_role_clone = expert_role.clone();
    let findings = ctx
        .pool()
        .run(move |conn| {
            get_findings_sync(
                conn,
                project_id,
                status_clone.as_deref(),
                expert_role_clone.as_deref(),
                file_path_clone.as_deref(),
                limit,
            )
        })
        .await?;

    if findings.is_empty() {
        let mut msg = "No findings found".to_string();
        if let Some(s) = &status {
            msg.push_str(&format!(" with status '{}'", s));
        }
        if let Some(f) = &file_path {
            msg.push_str(&format!(" for file '{}'", f));
        }
        if let Some(r) = &expert_role {
            msg.push_str(&format!(" from '{}'", r));
        }
        return Ok(msg);
    }

    // Get stats for context
    let (pending, accepted, rejected, fixed) = ctx
        .pool()
        .run(move |conn| get_finding_stats_sync(conn, project_id))
        .await
        .unwrap_or((0, 0, 0, 0));

    let mut output = format!(
        "{} findings (pending: {}, accepted: {}, rejected: {}, fixed: {}):\n\n",
        findings.len(),
        pending,
        accepted,
        rejected,
        fixed
    );

    for f in findings {
        let file_info = f.file_path.as_deref().unwrap_or("(no file)");
        let severity_icon = match f.severity.as_str() {
            "critical" => "[!!]",
            "major" => "[!]",
            "minor" => "[-]",
            _ => "[.]",
        };

        output.push_str(&format!(
            "  {} [{}] {} - {} ({})\n",
            severity_icon, f.id, f.finding_type, file_info, f.status
        ));
        output.push_str(&format!("     {}\n", truncate(&f.content, 100)));

        if let Some(suggestion) = &f.suggestion {
            output.push_str(&format!("     Suggestion: {}\n", truncate(suggestion, 80)));
        }

        if let Some(feedback) = &f.feedback {
            output.push_str(&format!("     Feedback: {}\n", truncate(feedback, 80)));
        }

        output.push('\n');
    }

    Ok(output)
}

/// Review a single finding (accept/reject/fixed)
pub async fn review_finding<C: ToolContext>(
    ctx: &C,
    finding_id: i64,
    status: String,
    feedback: Option<String>,
) -> Result<String, String> {
    // Validate status
    let valid_statuses = ["accepted", "rejected", "fixed"];
    if !valid_statuses.contains(&status.as_str()) {
        return Err(format!(
            "Invalid status '{}'. Valid values: accepted, rejected, fixed",
            status
        ));
    }

    // Get the finding first to verify it exists
    let finding = ctx
        .pool()
        .run(move |conn| get_finding_sync(conn, finding_id))
        .await?
        .ok_or_else(|| format!("Finding {} not found", finding_id))?;

    if finding.status != "pending" {
        return Err(format!(
            "Finding {} is already '{}', cannot change to '{}'",
            finding_id, finding.status, status
        ));
    }

    // Get reviewer identity
    let reviewed_by = ctx.get_user_identity();

    // Update the status
    let status_clone = status.clone();
    let feedback_clone = feedback.clone();
    let reviewed_by_clone = reviewed_by.clone();
    let updated = ctx
        .pool()
        .run(move |conn| {
            update_finding_status_sync(
                conn,
                finding_id,
                &status_clone,
                feedback_clone.as_deref(),
                reviewed_by_clone.as_deref(),
            )
        })
        .await?;

    if !updated {
        return Err(format!("Failed to update finding {}", finding_id));
    }

    // If accepted with a suggestion, consider creating/updating a correction pattern
    if status == "accepted" && finding.suggestion.is_some() {
        // We could trigger pattern learning here, but for now just log it
        tracing::debug!(
            finding_id,
            finding_type = %finding.finding_type,
            "Accepted finding may contribute to learned patterns"
        );
    }

    let mut response = format!("Finding {} marked as '{}'", finding_id, status);
    if let Some(fb) = &feedback {
        response.push_str(&format!(" with feedback: {}", truncate(fb, 50)));
    }

    Ok(response)
}

/// Bulk review multiple findings
pub async fn bulk_review_findings<C: ToolContext>(
    ctx: &C,
    finding_ids: Vec<i64>,
    status: String,
) -> Result<String, String> {
    // Validate status
    let valid_statuses = ["accepted", "rejected", "fixed"];
    if !valid_statuses.contains(&status.as_str()) {
        return Err(format!(
            "Invalid status '{}'. Valid values: accepted, rejected, fixed",
            status
        ));
    }

    if finding_ids.is_empty() {
        return Err("No finding IDs provided".to_string());
    }

    let reviewed_by = ctx.get_user_identity();
    let finding_ids_len = finding_ids.len();
    let status_clone = status.clone();

    let updated = ctx
        .pool()
        .run(move |conn| {
            bulk_update_finding_status_sync(conn, &finding_ids, &status_clone, reviewed_by.as_deref())
        })
        .await?;

    Ok(format!(
        "Updated {} of {} findings to '{}'",
        updated,
        finding_ids_len,
        status
    ))
}

/// Get details of a specific finding
pub async fn get_finding<C: ToolContext>(
    ctx: &C,
    finding_id: i64,
) -> Result<String, String> {
    let finding = ctx
        .pool()
        .run(move |conn| get_finding_sync(conn, finding_id))
        .await?
        .ok_or_else(|| format!("Finding {} not found", finding_id))?;

    let mut output = format!("Finding #{} ({})\n", finding.id, finding.status);
    output.push_str(&format!("Expert: {}\n", finding.expert_role));
    output.push_str(&format!("Type: {} | Severity: {}\n", finding.finding_type, finding.severity));

    if let Some(file) = &finding.file_path {
        output.push_str(&format!("File: {}\n", file));
    }

    output.push_str(&format!("Confidence: {:.0}%\n", finding.confidence * 100.0));
    output.push_str(&format!("\nContent:\n{}\n", finding.content));

    if let Some(snippet) = &finding.code_snippet {
        output.push_str(&format!("\nCode:\n```\n{}\n```\n", snippet));
    }

    if let Some(suggestion) = &finding.suggestion {
        output.push_str(&format!("\nSuggestion:\n{}\n", suggestion));
    }

    if let Some(feedback) = &finding.feedback {
        output.push_str(&format!("\nFeedback: {}\n", feedback));
    }

    if let Some(reviewer) = &finding.reviewed_by {
        output.push_str(&format!("Reviewed by: {} at {}\n", reviewer, finding.reviewed_at.as_deref().unwrap_or("?")));
    }

    output.push_str(&format!("\nCreated: {}", finding.created_at));
    if let Some(session) = &finding.session_id {
        output.push_str(&format!(" (session: {})", &session[..8.min(session.len())]));
    }

    Ok(output)
}

/// Get learned correction patterns
pub async fn get_learned_patterns<C: ToolContext>(
    ctx: &C,
    correction_type: Option<String>,
    limit: Option<i64>,
) -> Result<String, String> {
    let project_id = ctx.project_id().await;
    let limit = limit.unwrap_or(20) as usize;

    let corrections = ctx
        .pool()
        .run(move |conn| {
            get_relevant_corrections_sync(conn, project_id, correction_type.as_deref(), limit)
        })
        .await?;

    if corrections.is_empty() {
        return Ok("No learned patterns yet. Review findings to build up patterns.".to_string());
    }

    let mut output = format!("{} learned patterns:\n\n", corrections.len());

    for c in corrections {
        output.push_str(&format!(
            "[{}] {} (confidence: {:.0}%, seen: {}x, acceptance: {:.0}%)\n",
            c.id,
            c.correction_type,
            c.confidence * 100.0,
            c.occurrence_count,
            c.acceptance_rate * 100.0
        ));
        output.push_str(&format!("  Problem: {}\n", truncate(&c.what_was_wrong, 80)));
        output.push_str(&format!("  Fix: {}\n\n", truncate(&c.what_is_right, 80)));
    }

    Ok(output)
}

/// Trigger pattern extraction from accepted findings
pub async fn extract_patterns<C: ToolContext>(ctx: &C) -> Result<String, String> {
    let project_id = ctx.project_id().await;

    let created = ctx
        .pool()
        .run(move |conn| extract_patterns_from_findings_sync(conn, project_id))
        .await?;

    if created == 0 {
        Ok("No new patterns extracted. Need more accepted findings with suggestions.".to_string())
    } else {
        Ok(format!("Extracted {} new patterns from accepted findings", created))
    }
}

/// Get finding statistics
pub async fn get_finding_stats<C: ToolContext>(ctx: &C) -> Result<String, String> {
    let project_id = ctx.project_id().await;

    let (pending, accepted, rejected, fixed) = ctx
        .pool()
        .run(move |conn| get_finding_stats_sync(conn, project_id))
        .await?;

    let total = pending + accepted + rejected + fixed;
    if total == 0 {
        return Ok("No review findings yet.".to_string());
    }

    let acceptance_rate = if accepted + rejected > 0 {
        (accepted as f64 / (accepted + rejected) as f64) * 100.0
    } else {
        0.0
    };

    Ok(format!(
        "Review Finding Statistics:\n  Total: {}\n  Pending: {}\n  Accepted: {}\n  Rejected: {}\n  Fixed: {}\n  Acceptance rate: {:.1}%",
        total, pending, accepted, rejected, fixed, acceptance_rate
    ))
}

/// Unified finding tool with action parameter
/// Actions: list, get, review, stats, patterns, extract
#[allow(clippy::too_many_arguments)]
pub async fn finding<C: ToolContext>(
    ctx: &C,
    action: FindingAction,
    finding_id: Option<i64>,
    finding_ids: Option<Vec<i64>>,
    status: Option<String>,
    feedback: Option<String>,
    file_path: Option<String>,
    expert_role: Option<String>,
    correction_type: Option<String>,
    limit: Option<i64>,
) -> Result<String, String> {
    match action {
        FindingAction::List => list_findings(ctx, status, file_path, expert_role, limit).await,
        FindingAction::Get => {
            let id = finding_id.ok_or("finding_id is required for action 'get'")?;
            get_finding(ctx, id).await
        }
        FindingAction::Review => {
            let new_status = status.ok_or("status is required for action 'review'")?;
            // Check if bulk review (finding_ids) or single review (finding_id)
            if let Some(ids) = finding_ids {
                if !ids.is_empty() {
                    return bulk_review_findings(ctx, ids, new_status).await;
                }
            }
            let id = finding_id.ok_or("finding_id (or finding_ids for bulk) is required for action 'review'")?;
            review_finding(ctx, id, new_status, feedback).await
        }
        FindingAction::Stats => get_finding_stats(ctx).await,
        FindingAction::Patterns => get_learned_patterns(ctx, correction_type, limit).await,
        FindingAction::Extract => extract_patterns(ctx).await,
    }
}
