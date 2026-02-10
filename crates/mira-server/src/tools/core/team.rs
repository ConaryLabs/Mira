// crates/mira-server/src/tools/core/team.rs
// Team intelligence tool — status and review for Agent Teams

use super::ToolContext;
use crate::mcp::requests::{TeamAction, TeamRequest};
use crate::mcp::responses::team::*;
use crate::mcp::responses::{Json, TeamOutput, ToolOutput};
use std::collections::HashMap;

/// Handle team tool actions.
pub async fn handle_team<C: ToolContext + ?Sized>(
    ctx: &C,
    req: TeamRequest,
) -> Result<Json<TeamOutput>, String> {
    match req.action {
        TeamAction::Status => team_status(ctx).await,
        TeamAction::Review => team_review(ctx, req.teammate).await,
        TeamAction::Distill => team_distill(ctx).await,
    }
}

/// Get team status: members, files, conflicts.
async fn team_status<C: ToolContext + ?Sized>(ctx: &C) -> Result<Json<TeamOutput>, String> {
    let membership = ctx.get_team_membership().ok_or_else(|| {
        "Not in a team. Team tools require an active Agent Teams session.".to_string()
    })?;

    let pool = ctx.pool().clone();
    let tid = membership.team_id;

    // Get active members and their files
    let (members_info, all_conflicts) = pool
        .interact(move |conn| {
            let members = crate::db::get_active_team_members_sync(conn, tid);
            let mut member_summaries: Vec<(crate::db::TeamMemberInfo, Vec<String>)> = Vec::new();

            for m in &members {
                let files = crate::db::get_member_files_sync(conn, tid, &m.session_id);
                member_summaries.push((m.clone(), files));
            }

            // Collect all conflicts across sessions
            let mut conflict_map: HashMap<String, Vec<String>> = HashMap::new();
            for m in &members {
                let conflicts = crate::db::get_file_conflicts_sync(conn, tid, &m.session_id);
                for c in conflicts {
                    conflict_map
                        .entry(c.file_path.clone())
                        .or_default()
                        .push(c.other_member_name.clone());
                    // Also add the current member
                    conflict_map
                        .entry(c.file_path)
                        .or_default()
                        .push(m.member_name.clone());
                }
            }

            // Deduplicate conflict editors
            for editors in conflict_map.values_mut() {
                editors.sort();
                editors.dedup();
            }

            Ok::<_, anyhow::Error>((member_summaries, conflict_map))
        })
        .await
        .map_err(|e| format!("Failed to get team status: {}", e))?;

    let active_count = members_info.len();
    let members: Vec<TeamMemberSummary> = members_info
        .into_iter()
        .map(|(m, files)| TeamMemberSummary {
            name: m.member_name,
            role: m.role,
            status: m.status,
            last_heartbeat: m.last_heartbeat,
            files,
        })
        .collect();

    let file_conflicts: Vec<FileConflictSummary> = all_conflicts
        .into_iter()
        .map(|(file_path, edited_by)| FileConflictSummary {
            file_path,
            edited_by,
        })
        .collect();

    let conflict_note = if file_conflicts.is_empty() {
        String::new()
    } else {
        format!(" {} file conflict(s) detected.", file_conflicts.len())
    };

    let message = format!(
        "Team '{}': {} active member(s).{}",
        membership.team_name, active_count, conflict_note
    );

    Ok(Json(ToolOutput {
        action: "status".to_string(),
        message,
        data: Some(TeamData::Status(TeamStatusData {
            team_name: membership.team_name,
            team_id: membership.team_id,
            members,
            active_count,
            file_conflicts,
        })),
    }))
}

/// Review a teammate's work: files modified.
async fn team_review<C: ToolContext + ?Sized>(
    ctx: &C,
    teammate: Option<String>,
) -> Result<Json<TeamOutput>, String> {
    let membership = ctx.get_team_membership().ok_or_else(|| {
        "Not in a team. Team tools require an active Agent Teams session.".to_string()
    })?;

    let pool = ctx.pool().clone();
    let tid = membership.team_id;
    let my_name = membership.member_name.clone();

    // Find the target teammate
    let target_name = teammate.unwrap_or_else(|| my_name.clone());

    let (member_name, files) = pool
        .interact(move |conn| {
            let members = crate::db::get_active_team_members_sync(conn, tid);

            // Find matching member
            let member = members
                .iter()
                .find(|m| m.member_name == target_name)
                .ok_or_else(|| {
                    let available: Vec<&str> =
                        members.iter().map(|m| m.member_name.as_str()).collect();
                    anyhow::anyhow!(
                        "Teammate '{}' not found. Active members: {}",
                        target_name,
                        available.join(", ")
                    )
                })?;

            let files = crate::db::get_member_files_sync(conn, tid, &member.session_id);

            Ok::<_, anyhow::Error>((member.member_name.clone(), files))
        })
        .await
        .map_err(|e| format!("Failed to review team work: {}", e))?;

    let file_count = files.len();
    let message = format!("{} has modified {} file(s).", member_name, file_count);

    Ok(Json(ToolOutput {
        action: "review".to_string(),
        message,
        data: Some(TeamData::Review(TeamReviewData {
            member_name,
            files_modified: files,
            file_count,
        })),
    }))
}

/// Distill key findings/decisions from team work into team-scoped memories.
async fn team_distill<C: ToolContext + ?Sized>(ctx: &C) -> Result<Json<TeamOutput>, String> {
    let membership = ctx.get_team_membership().ok_or_else(|| {
        "Not in a team. Team tools require an active Agent Teams session.".to_string()
    })?;

    let pool = ctx.pool().clone();
    let tid = membership.team_id;
    let project_id = ctx.project_id().await;

    let result =
        crate::background::knowledge_distillation::distill_team_session(&pool, tid, project_id)
            .await?;

    match result {
        Some(result) => {
            let message =
                crate::background::knowledge_distillation::format_distillation_result(&result);
            let findings: Vec<DistilledFindingSummary> = result
                .findings
                .iter()
                .map(|f| DistilledFindingSummary {
                    category: f.category.clone(),
                    content: f.content.clone(),
                    source_count: f.source_count,
                })
                .collect();

            Ok(Json(ToolOutput {
                action: "distill".to_string(),
                message,
                data: Some(TeamData::Distill(TeamDistillData {
                    team_name: result.team_name,
                    findings_count: findings.len(),
                    memories_processed: result.total_memories_processed,
                    files_touched: result.total_files_touched,
                    findings,
                })),
            }))
        }
        None => Ok(Json(ToolOutput {
            action: "distill".to_string(),
            message: format!(
                "No findings to distill for team '{}'. Insufficient data (need at least {} memories or file activity).",
                membership.team_name, 2,
            ),
            data: None,
        })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_team_action_variants() {
        // Verify the action enum deserializes correctly
        let status: TeamAction = serde_json::from_str(r#""status""#).unwrap();
        assert!(matches!(status, TeamAction::Status));

        let review: TeamAction = serde_json::from_str(r#""review""#).unwrap();
        assert!(matches!(review, TeamAction::Review));

        let distill: TeamAction = serde_json::from_str(r#""distill""#).unwrap();
        assert!(matches!(distill, TeamAction::Distill));
    }
}
