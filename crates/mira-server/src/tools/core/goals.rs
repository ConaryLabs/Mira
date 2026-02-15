//! crates/mira-server/src/tools/core/goals.rs
//! Goal and milestone tools - split into focused action functions

use crate::db::{
    complete_milestone_sync, count_sessions_for_goal_sync, create_goal_sync, create_milestone_sync,
    delete_goal_sync, delete_milestone_sync, get_active_goals_sync, get_goal_by_id_sync,
    get_goals_sync, get_milestone_by_id_sync, get_milestones_for_goal_sync,
    get_sessions_for_goal_sync, record_session_goal_sync,
    update_goal_progress_from_milestones_sync, update_goal_sync,
};
use crate::mcp::requests::{GoalAction, GoalRequest};
use crate::mcp::responses::Json;
use crate::mcp::responses::{
    GoalBulkCreatedData, GoalCreatedData, GoalCreatedEntry, GoalData, GoalGetData, GoalListData,
    GoalOutput, GoalSessionEntry, GoalSessionsData, GoalSummary, MilestoneInfo,
    MilestoneProgressData,
};
use crate::tools::core::ToolContext;
use serde::Deserialize;

/// Goal definition for bulk creation
#[derive(Debug, Deserialize)]
struct BulkGoal {
    title: String,
    description: Option<String>,
    priority: Option<String>,
    status: Option<String>,
}

// ============================================================================
// Authorization helpers
// ============================================================================

/// Verify a goal belongs to the current project context.
/// Returns error if both have project IDs that don't match.
fn verify_goal_project(
    goal_project_id: Option<i64>,
    ctx_project_id: Option<i64>,
) -> Result<(), String> {
    match (goal_project_id, ctx_project_id) {
        // Both have project IDs — must match
        (Some(goal_pid), Some(ctx_pid)) if goal_pid != ctx_pid => {
            Err("Access denied: goal belongs to a different project".to_string())
        }
        // Goal has a project but no context project — deny access
        (Some(_), None) => Err("Access denied: goal belongs to a different project".to_string()),
        // Both match, goal is global, or both are None — allow
        _ => Ok(()),
    }
}

/// Fetch a goal by ID and verify project authorization.
async fn get_authorized_goal<C: ToolContext>(ctx: &C, id: i64) -> Result<crate::db::Goal, String> {
    let goal = ctx
        .pool()
        .run(move |conn| get_goal_by_id_sync(conn, id))
        .await?
        .ok_or_else(|| format!("Goal not found (id: {})", id))?;

    let ctx_project_id = ctx.project_id().await;
    verify_goal_project(goal.project_id, ctx_project_id)?;

    Ok(goal)
}

/// Look up a milestone's parent goal_id and verify project authorization.
async fn verify_milestone_project<C: ToolContext>(
    ctx: &C,
    milestone_id: i64,
) -> Result<(), String> {
    let milestone = ctx
        .pool()
        .run(move |conn| get_milestone_by_id_sync(conn, milestone_id))
        .await?
        .ok_or_else(|| format!("Milestone not found (id: {})", milestone_id))?;

    let goal_id = milestone
        .goal_id
        .ok_or_else(|| "Milestone has no associated goal".to_string())?;

    let goal = ctx
        .pool()
        .run(move |conn| get_goal_by_id_sync(conn, goal_id))
        .await?
        .ok_or_else(|| format!("Goal not found (id: {})", goal_id))?;

    let ctx_project_id = ctx.project_id().await;
    verify_goal_project(goal.project_id, ctx_project_id)?;

    Ok(())
}

/// Valid goal statuses.
const VALID_STATUSES: &[&str] = &[
    "planning",
    "in_progress",
    "blocked",
    "completed",
    "abandoned",
];

/// Valid goal priorities.
const VALID_PRIORITIES: &[&str] = &["low", "medium", "high", "critical"];

/// Validate a status value, if provided.
fn validate_status(status: &Option<String>) -> Result<(), String> {
    if let Some(s) = status
        && !VALID_STATUSES.contains(&s.as_str())
    {
        return Err(format!(
            "Invalid status '{}'. Must be one of: {}",
            s,
            VALID_STATUSES.join(", ")
        ));
    }
    Ok(())
}

/// Validate a priority value, if provided.
fn validate_priority(priority: &Option<String>) -> Result<(), String> {
    if let Some(p) = priority
        && !VALID_PRIORITIES.contains(&p.as_str())
    {
        return Err(format!(
            "Invalid priority '{}'. Must be one of: {}",
            p,
            VALID_PRIORITIES.join(", ")
        ));
    }
    Ok(())
}

/// Validate that an ID is positive (non-zero, non-negative).
fn validate_positive_id(id: i64, field: &str) -> Result<i64, String> {
    if id <= 0 {
        Err(format!("Invalid {}: must be positive", field))
    } else {
        Ok(id)
    }
}

/// Record a session-goal link (fire-and-forget, never fails the parent operation).
async fn record_goal_interaction<C: ToolContext>(ctx: &C, goal_id: i64, interaction_type: &str) {
    let session_id = match ctx.get_session_id().await {
        Some(id) => id,
        None => return, // No active session, skip recording
    };
    let itype = interaction_type.to_string();
    let _ = ctx
        .pool()
        .run(move |conn| record_session_goal_sync(conn, &session_id, goal_id, &itype))
        .await;
    // Silently ignore errors — this is best-effort tracking
}

// ============================================================================
// Action-specific functions
// ============================================================================

/// Get a goal by ID with its milestones
async fn action_get<C: ToolContext>(ctx: &C, goal_id: i64) -> Result<Json<GoalOutput>, String> {
    let goal = get_authorized_goal(ctx, goal_id).await?;

    let mut response = format!("Goal [{}]: {}\n", goal.id, goal.title);
    response.push_str(&format!("  Status: {}\n", goal.status));
    response.push_str(&format!("  Priority: {}\n", goal.priority));
    response.push_str(&format!("  Progress: {}%\n", goal.progress_percent));
    if let Some(desc) = &goal.description {
        response.push_str(&format!("  Description: {}\n", desc));
    }
    response.push_str(&format!("  Created: {}\n", goal.created_at));

    // Show milestones
    let milestones = ctx
        .pool()
        .run(move |conn| get_milestones_for_goal_sync(conn, goal_id))
        .await?;

    let mut milestone_items = Vec::new();
    if !milestones.is_empty() {
        response.push_str(&format!("\n  Milestones ({}):\n", milestones.len()));
        for m in &milestones {
            let icon = if m.completed { "[x]" } else { "[ ]" };
            response.push_str(&format!(
                "    {} [{}] {} (weight: {})\n",
                icon, m.id, m.title, m.weight
            ));
            milestone_items.push(MilestoneInfo {
                id: m.id,
                title: m.title.clone(),
                weight: m.weight,
                completed: m.completed,
            });
        }
    }

    Ok(Json(GoalOutput {
        action: "get".into(),
        message: response,
        data: Some(GoalData::Get(GoalGetData {
            id: goal.id,
            title: goal.title,
            status: goal.status,
            priority: goal.priority,
            progress_percent: goal.progress_percent,
            description: goal.description,
            created_at: goal.created_at,
            milestones: milestone_items,
        })),
    }))
}

/// Create a new goal
async fn action_create<C: ToolContext>(
    ctx: &C,
    project_id: Option<i64>,
    title: String,
    description: Option<String>,
    status: Option<String>,
    priority: Option<String>,
    progress_percent: Option<i32>,
) -> Result<Json<GoalOutput>, String> {
    validate_status(&status)?;
    validate_priority(&priority)?;

    let title_for_result = title.clone();

    let id = ctx
        .pool()
        .run(move |conn| {
            create_goal_sync(
                conn,
                project_id,
                &title,
                description.as_deref(),
                status.as_deref(),
                priority.as_deref(),
                progress_percent.map(|p| p as i64),
            )
        })
        .await
        .map_err(|e| format!("Failed to create goal: {}", e))?;

    // Record session-goal link
    record_goal_interaction(ctx, id, "created").await;

    Ok(Json(GoalOutput {
        action: "create".into(),
        message: format!("Created goal '{}' (id: {})", title_for_result, id),
        data: Some(GoalData::Created(GoalCreatedData { goal_id: id })),
    }))
}

/// Bulk create multiple goals
async fn action_bulk_create<C: ToolContext>(
    ctx: &C,
    project_id: Option<i64>,
    goals_json: &str,
) -> Result<Json<GoalOutput>, String> {
    let bulk_goals: Vec<BulkGoal> = serde_json::from_str(goals_json).map_err(|e| {
        format!(
            "Invalid goals JSON: {}. Expected: [{{\"title\": \"...\", \"description?\": \"...\", \"priority?\": \"...\"}}]",
            e
        )
    })?;

    if bulk_goals.is_empty() {
        return Err("goals array cannot be empty".to_string());
    }

    if bulk_goals.len() > 100 {
        return Err("Too many goals: maximum 100 per bulk_create call".into());
    }

    // Validate status/priority for all goals before writing any
    for g in &bulk_goals {
        validate_status(&g.status)?;
        validate_priority(&g.priority)?;
    }

    let results = ctx
        .pool()
        .run(move |conn| {
            let tx = conn.unchecked_transaction()?;
            let mut entries = Vec::new();
            for g in &bulk_goals {
                let id = create_goal_sync(
                    &tx,
                    project_id,
                    &g.title,
                    g.description.as_deref(),
                    g.status.as_deref(),
                    g.priority.as_deref(),
                    None,
                )?;
                entries.push((id, g.title.clone()));
            }
            tx.commit()?;
            Ok::<_, rusqlite::Error>(entries)
        })
        .await?;

    // Record session-goal links for all created goals
    for &(id, _) in &results {
        record_goal_interaction(ctx, id, "created").await;
    }

    let created: Vec<String> = results
        .iter()
        .map(|(id, t)| format!("[{}] {}", id, t))
        .collect();
    let entries: Vec<GoalCreatedEntry> = results
        .into_iter()
        .map(|(id, title)| GoalCreatedEntry { id, title })
        .collect();

    Ok(Json(GoalOutput {
        action: "bulk_create".into(),
        message: format!(
            "Created {} goals:\n  {}",
            created.len(),
            created.join("\n  ")
        ),
        data: Some(GoalData::BulkCreated(GoalBulkCreatedData {
            goals: entries,
        })),
    }))
}

/// List goals with optional filters
async fn action_list<C: ToolContext>(
    ctx: &C,
    project_id: Option<i64>,
    include_finished: bool,
    limit: i64,
) -> Result<Json<GoalOutput>, String> {
    let limit_usize = limit.max(0) as usize;
    let incl = include_finished;
    // Get true total count before applying limit
    let total_count = {
        let pid = project_id;
        ctx.pool()
            .run(move |conn| crate::db::count_goals_sync(conn, pid, incl))
            .await
            .map_err(|e| {
                tracing::warn!("Failed to count goals: {}", e);
                e
            })
            .unwrap_or(0)
    };
    let goals = if include_finished {
        let mut all = ctx
            .pool()
            .run(move |conn| get_goals_sync(conn, project_id, None))
            .await
            .map_err(|e| format!("Failed to list goals: {}", e))?;
        if limit_usize > 0 {
            all.truncate(limit_usize);
        }
        all
    } else {
        ctx.pool()
            .run(move |conn| get_active_goals_sync(conn, project_id, limit_usize))
            .await
            .map_err(|e| format!("Failed to list active goals: {}", e))?
    };

    if goals.is_empty() {
        let message = if total_count > 0 {
            format!("{} goals (showing 0):\n", total_count)
        } else {
            "No goals found.".into()
        };
        return Ok(Json(GoalOutput {
            action: "list".into(),
            message,
            data: Some(GoalData::List(GoalListData {
                goals: vec![],
                total: total_count,
            })),
        }));
    }

    // Fetch milestones for all goals in one pass
    let goal_ids: Vec<i64> = goals.iter().map(|g| g.id).collect();
    let milestones_by_goal = {
        let ids = goal_ids.clone();
        ctx.pool()
            .run(move |conn| -> rusqlite::Result<std::collections::HashMap<i64, Vec<MilestoneInfo>>> {
                let mut map = std::collections::HashMap::new();
                for gid in ids {
                    let milestones = get_milestones_for_goal_sync(conn, gid)?;
                    if !milestones.is_empty() {
                        map.insert(
                            gid,
                            milestones
                                .into_iter()
                                .map(|m| MilestoneInfo {
                                    id: m.id,
                                    title: m.title,
                                    weight: m.weight,
                                    completed: m.completed,
                                })
                                .collect(),
                        );
                    }
                }
                Ok(map)
            })
            .await?
    };

    let display_total = if total_count > 0 && total_count > goals.len() {
        format!("{} goals (showing {}):\n", total_count, goals.len())
    } else {
        format!("{} goals:\n", goals.len())
    };
    let mut response = display_total;
    let items: Vec<GoalSummary> = goals
        .into_iter()
        .map(|goal| {
            let icon = match goal.status.as_str() {
                "completed" => "[x]",
                "in_progress" => "[>]",
                "abandoned" => "[-]",
                _ => "[ ]",
            };
            let ms = milestones_by_goal
                .get(&goal.id)
                .cloned()
                .unwrap_or_default();
            response.push_str(&format!(
                "  {} {} ({}%) - {} [{}]\n",
                icon, goal.title, goal.progress_percent, goal.priority, goal.id
            ));
            if !ms.is_empty() {
                for m in &ms {
                    let mi = if m.completed { "[x]" } else { "[ ]" };
                    response.push_str(&format!("    {} {} (w:{})\n", mi, m.title, m.weight));
                }
            }
            GoalSummary {
                id: goal.id,
                title: goal.title,
                status: goal.status,
                priority: goal.priority,
                progress_percent: goal.progress_percent,
                milestones: ms,
            }
        })
        .collect();
    let total = if total_count > 0 {
        total_count
    } else {
        items.len()
    }; // fallback only if count query failed
    Ok(Json(GoalOutput {
        action: "list".into(),
        message: response,
        data: Some(GoalData::List(GoalListData {
            goals: items,
            total,
        })),
    }))
}

/// Update a goal
async fn action_update<C: ToolContext>(
    ctx: &C,
    goal_id: i64,
    title: Option<String>,
    description: Option<String>,
    status: Option<String>,
    priority: Option<String>,
    progress_percent: Option<i32>,
) -> Result<Json<GoalOutput>, String> {
    validate_status(&status)?;
    validate_priority(&priority)?;

    get_authorized_goal(ctx, goal_id).await?;

    ctx.pool()
        .run(move |conn| {
            update_goal_sync(
                conn,
                goal_id,
                title.as_deref(),
                description.as_deref(),
                status.as_deref(),
                priority.as_deref(),
                progress_percent.map(|p| p as i64),
            )
        })
        .await?;

    // Record session-goal link
    record_goal_interaction(ctx, goal_id, "updated").await;

    Ok(Json(GoalOutput {
        action: "update".into(),
        message: format!("Updated goal {}", goal_id),
        data: None,
    }))
}

/// Delete a goal
async fn action_delete<C: ToolContext>(ctx: &C, goal_id: i64) -> Result<Json<GoalOutput>, String> {
    get_authorized_goal(ctx, goal_id).await?;

    ctx.pool()
        .run(move |conn| delete_goal_sync(conn, goal_id))
        .await?;

    Ok(Json(GoalOutput {
        action: "delete".into(),
        message: format!("Deleted goal {}", goal_id),
        data: None,
    }))
}

/// Add a milestone to a goal
async fn action_add_milestone<C: ToolContext>(
    ctx: &C,
    goal_id: i64,
    milestone_title: String,
    weight: Option<i32>,
) -> Result<Json<GoalOutput>, String> {
    get_authorized_goal(ctx, goal_id).await?;

    let mtitle_for_result = milestone_title.clone();

    let mid = ctx
        .pool()
        .run(move |conn| create_milestone_sync(conn, goal_id, &milestone_title, weight))
        .await?;

    // Record session-goal link
    record_goal_interaction(ctx, goal_id, "milestone_added").await;

    Ok(Json(GoalOutput {
        action: "add_milestone".into(),
        message: format!(
            "Added milestone '{}' to goal {} (milestone id: {})",
            mtitle_for_result, goal_id, mid
        ),
        data: Some(GoalData::MilestoneProgress(MilestoneProgressData {
            milestone_id: mid,
            goal_id: Some(goal_id),
            progress_percent: None,
        })),
    }))
}

/// Complete a milestone and update goal progress
async fn action_complete_milestone<C: ToolContext>(
    ctx: &C,
    milestone_id: i64,
) -> Result<Json<GoalOutput>, String> {
    verify_milestone_project(ctx, milestone_id).await?;

    let goal_id_result = ctx
        .pool()
        .run(move |conn| complete_milestone_sync(conn, milestone_id))
        .await?;

    if let Some(gid) = goal_id_result {
        let progress = ctx
            .pool()
            .run(move |conn| update_goal_progress_from_milestones_sync(conn, gid))
            .await?;

        // Record session-goal link
        record_goal_interaction(ctx, gid, "milestone_completed").await;

        Ok(Json(GoalOutput {
            action: "complete_milestone".into(),
            message: format!(
                "Completed milestone {}. Goal progress updated to {}%",
                milestone_id, progress
            ),
            data: Some(GoalData::MilestoneProgress(MilestoneProgressData {
                milestone_id,
                goal_id: Some(gid),
                progress_percent: Some(progress),
            })),
        }))
    } else {
        Ok(Json(GoalOutput {
            action: "complete_milestone".into(),
            message: format!("Completed milestone {}", milestone_id),
            data: Some(GoalData::MilestoneProgress(MilestoneProgressData {
                milestone_id,
                goal_id: None,
                progress_percent: None,
            })),
        }))
    }
}

/// Delete a milestone and update goal progress
async fn action_delete_milestone<C: ToolContext>(
    ctx: &C,
    milestone_id: i64,
) -> Result<Json<GoalOutput>, String> {
    verify_milestone_project(ctx, milestone_id).await?;

    let goal_id_result = ctx
        .pool()
        .run(move |conn| delete_milestone_sync(conn, milestone_id))
        .await?;

    if let Some(gid) = goal_id_result {
        let progress = ctx
            .pool()
            .run(move |conn| update_goal_progress_from_milestones_sync(conn, gid))
            .await?;

        Ok(Json(GoalOutput {
            action: "delete_milestone".into(),
            message: format!(
                "Deleted milestone {}. Goal progress updated to {}%",
                milestone_id, progress
            ),
            data: Some(GoalData::MilestoneProgress(MilestoneProgressData {
                milestone_id,
                goal_id: Some(gid),
                progress_percent: Some(progress),
            })),
        }))
    } else {
        Ok(Json(GoalOutput {
            action: "delete_milestone".into(),
            message: format!("Deleted milestone {}", milestone_id),
            data: Some(GoalData::MilestoneProgress(MilestoneProgressData {
                milestone_id,
                goal_id: None,
                progress_percent: None,
            })),
        }))
    }
}

/// List sessions that worked on a goal
async fn action_sessions<C: ToolContext>(
    ctx: &C,
    goal_id: i64,
    limit: usize,
) -> Result<Json<GoalOutput>, String> {
    get_authorized_goal(ctx, goal_id).await?;

    let lim = limit;
    let links = ctx
        .pool()
        .run(move |conn| get_sessions_for_goal_sync(conn, goal_id, lim))
        .await
        .map_err(|e| format!("Failed to get sessions for goal: {}", e))?;

    let total = ctx
        .pool()
        .run(move |conn| count_sessions_for_goal_sync(conn, goal_id))
        .await
        .map_err(|e| format!("Failed to count sessions: {}", e))?;

    let mut response = format!("Goal {} — {} distinct session(s):\n", goal_id, total);
    let entries: Vec<GoalSessionEntry> = links
        .into_iter()
        .map(|link| {
            response.push_str(&format!(
                "  {} — {} (last: {})\n",
                link.session_id, link.interaction_type, link.created_at
            ));
            GoalSessionEntry {
                session_id: link.session_id,
                interaction_type: link.interaction_type,
                created_at: link.created_at,
            }
        })
        .collect();

    Ok(Json(GoalOutput {
        action: "sessions".into(),
        message: response,
        data: Some(GoalData::Sessions(GoalSessionsData {
            goal_id,
            sessions: entries,
            total_sessions: total,
        })),
    }))
}

// ============================================================================
// Main dispatcher
// ============================================================================

/// Unified goal tool with actions: create, bulk_create, list, get, update, progress, delete,
/// add_milestone, complete_milestone, delete_milestone
pub async fn goal<C: ToolContext>(ctx: &C, req: GoalRequest) -> Result<Json<GoalOutput>, String> {
    let project_id = ctx.project_id().await;

    match req.action {
        GoalAction::Get => {
            let id = req
                .goal_id
                .ok_or("goal_id is required for goal(action=get). Use goal(action=\"list\") to see available goals.")?;
            let id = validate_positive_id(id, "goal_id")?;
            action_get(ctx, id).await
        }
        GoalAction::Create => {
            let t = req
                .title
                .ok_or("title is required for goal(action=create)")?;
            action_create(
                ctx,
                project_id,
                t,
                req.description,
                req.status,
                req.priority,
                req.progress_percent,
            )
            .await
        }
        GoalAction::BulkCreate => {
            let g = req
                .goals
                .ok_or("goals is required for goal(action=bulk_create)")?;
            action_bulk_create(ctx, project_id, &g).await
        }
        GoalAction::List => {
            action_list(
                ctx,
                project_id,
                req.include_finished.unwrap_or(false),
                req.limit.unwrap_or(10),
            )
            .await
        }
        GoalAction::Update | GoalAction::Progress => {
            let action_name = if matches!(req.action, GoalAction::Progress) {
                "progress"
            } else {
                "update"
            };
            let id = req
                .goal_id
                .ok_or_else(|| format!("goal_id is required for goal(action={}). Use goal(action=\"list\") to see available goals.", action_name))?;
            let id = validate_positive_id(id, "goal_id")?;
            action_update(
                ctx,
                id,
                req.title,
                req.description,
                req.status,
                req.priority,
                req.progress_percent,
            )
            .await
        }
        GoalAction::Delete => {
            let id = req
                .goal_id
                .ok_or("goal_id is required for goal(action=delete). Use goal(action=\"list\") to see available goals.")?;
            let id = validate_positive_id(id, "goal_id")?;
            action_delete(ctx, id).await
        }
        GoalAction::AddMilestone => {
            let gid = req
                .goal_id
                .ok_or("goal_id is required for goal(action=add_milestone). Use goal(action=\"list\") to see available goals.")?;
            let gid = validate_positive_id(gid, "goal_id")?;
            let mt = req
                .milestone_title
                .ok_or("milestone_title is required for goal(action=add_milestone)")?;
            action_add_milestone(ctx, gid, mt, req.weight).await
        }
        GoalAction::CompleteMilestone => {
            let mid = req
                .milestone_id
                .ok_or("milestone_id is required for goal(action=complete_milestone). Use goal(action=\"get\", goal_id=N) to see milestones.")?;
            let mid = validate_positive_id(mid, "milestone_id")?;
            action_complete_milestone(ctx, mid).await
        }
        GoalAction::DeleteMilestone => {
            let mid = req
                .milestone_id
                .ok_or("milestone_id is required for goal(action=delete_milestone). Use goal(action=\"get\", goal_id=N) to see milestones.")?;
            let mid = validate_positive_id(mid, "milestone_id")?;
            action_delete_milestone(ctx, mid).await
        }
        GoalAction::Sessions => {
            let id = req
                .goal_id
                .ok_or("goal_id is required for goal(action=sessions). Use goal(action=\"list\") to see available goals.")?;
            let id = validate_positive_id(id, "goal_id")?;
            let limit = req.limit.unwrap_or(20).max(1) as usize;
            action_sessions(ctx, id, limit).await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ═══════════════════════════════════════════════════════════════════════════
    // verify_goal_project
    // ═══════════════════════════════════════════════════════════════════════════

    #[test]
    fn test_verify_same_project() {
        assert!(verify_goal_project(Some(1), Some(1)).is_ok());
    }

    #[test]
    fn test_verify_different_project_denied() {
        let result = verify_goal_project(Some(1), Some(2));
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("Access denied"));
        assert!(err.contains("different project"));
    }

    #[test]
    fn test_verify_goal_has_project_ctx_none_denied() {
        // Goal belongs to a project but context has no project — deny
        let result = verify_goal_project(Some(1), None);
        assert!(result.is_err());
    }

    #[test]
    fn test_verify_global_goal_with_project_ctx() {
        // Global goal (no project_id) accessible from any project context
        assert!(verify_goal_project(None, Some(1)).is_ok());
    }

    #[test]
    fn test_verify_both_none() {
        // Global goal with no project context — allow
        assert!(verify_goal_project(None, None).is_ok());
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // validate_positive_id
    // ═══════════════════════════════════════════════════════════════════════════

    #[test]
    fn test_validate_positive() {
        assert_eq!(validate_positive_id(1, "goal_id").unwrap(), 1);
        assert_eq!(validate_positive_id(100, "goal_id").unwrap(), 100);
    }

    #[test]
    fn test_validate_zero_rejected() {
        let err = validate_positive_id(0, "goal_id").unwrap_err();
        assert!(err.contains("goal_id"));
        assert!(err.contains("must be positive"));
    }

    #[test]
    fn test_validate_negative_rejected() {
        let err = validate_positive_id(-5, "milestone_id").unwrap_err();
        assert!(err.contains("milestone_id"));
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // BulkGoal deserialization
    // ═══════════════════════════════════════════════════════════════════════════

    #[test]
    fn test_bulk_goal_full() {
        let json = r#"{"title": "My Goal", "description": "Details", "priority": "high", "status": "planning"}"#;
        let g: BulkGoal = serde_json::from_str(json).unwrap();
        assert_eq!(g.title, "My Goal");
        assert_eq!(g.description.unwrap(), "Details");
        assert_eq!(g.priority.unwrap(), "high");
        assert_eq!(g.status.unwrap(), "planning");
    }

    #[test]
    fn test_bulk_goal_minimal() {
        let json = r#"{"title": "Just a title"}"#;
        let g: BulkGoal = serde_json::from_str(json).unwrap();
        assert_eq!(g.title, "Just a title");
        assert!(g.description.is_none());
        assert!(g.priority.is_none());
        assert!(g.status.is_none());
    }

    #[test]
    fn test_bulk_goal_array() {
        let json = r#"[{"title": "A"}, {"title": "B", "priority": "low"}]"#;
        let goals: Vec<BulkGoal> = serde_json::from_str(json).unwrap();
        assert_eq!(goals.len(), 2);
        assert_eq!(goals[0].title, "A");
        assert_eq!(goals[1].priority.as_deref(), Some("low"));
    }

    #[test]
    fn test_bulk_goal_missing_title_fails() {
        let json = r#"{"description": "no title"}"#;
        let result = serde_json::from_str::<BulkGoal>(json);
        assert!(result.is_err());
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // GoalRequest dispatcher validation
    // ═══════════════════════════════════════════════════════════════════════════

    // ═══════════════════════════════════════════════════════════════════════════
    // Status and priority validation
    // ═══════════════════════════════════════════════════════════════════════════

    #[test]
    fn test_validate_status_valid() {
        for s in VALID_STATUSES {
            assert!(validate_status(&Some(s.to_string())).is_ok());
        }
        // None is always valid (field is optional)
        assert!(validate_status(&None).is_ok());
    }

    #[test]
    fn test_validate_status_invalid() {
        let err = validate_status(&Some("typo".into())).unwrap_err();
        assert!(err.contains("Invalid status 'typo'"));
        assert!(err.contains("planning"));
    }

    #[test]
    fn test_validate_priority_valid() {
        for p in VALID_PRIORITIES {
            assert!(validate_priority(&Some(p.to_string())).is_ok());
        }
        assert!(validate_priority(&None).is_ok());
    }

    #[test]
    fn test_validate_priority_invalid() {
        let err = validate_priority(&Some("urgent".into())).unwrap_err();
        assert!(err.contains("Invalid priority 'urgent'"));
        assert!(err.contains("low"));
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // GoalRequest dispatcher validation
    // ═══════════════════════════════════════════════════════════════════════════

    #[test]
    fn test_goal_action_deserialize() {
        let actions = [
            ("get", GoalAction::Get),
            ("create", GoalAction::Create),
            ("bulk_create", GoalAction::BulkCreate),
            ("list", GoalAction::List),
            ("update", GoalAction::Update),
            ("progress", GoalAction::Progress),
            ("delete", GoalAction::Delete),
            ("add_milestone", GoalAction::AddMilestone),
            ("complete_milestone", GoalAction::CompleteMilestone),
            ("delete_milestone", GoalAction::DeleteMilestone),
            ("sessions", GoalAction::Sessions),
        ];
        for (s, expected) in actions {
            let json = format!(r#"{{"action": "{}"}}"#, s);
            let req: GoalRequest = serde_json::from_str(&json).unwrap();
            assert_eq!(
                std::mem::discriminant(&req.action),
                std::mem::discriminant(&expected),
                "Failed for action: {}",
                s
            );
        }
    }
}
