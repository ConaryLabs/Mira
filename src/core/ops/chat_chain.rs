//! Core chat chain operations - shared by Chat/Studio
//!
//! Response chain management, handoff context, reset hysteresis,
//! failure tracking, and artifact tracking.

use chrono::Utc;

use super::super::{CoreResult, OpContext};

// ============================================================================
// Input/Output Types
// ============================================================================

/// A recent chat message for handoff context
#[derive(Debug, Clone)]
pub struct RecentMessage {
    pub role: String,
    pub blocks_json: String,
    pub created_at: i64,
}

/// An active goal for handoff context
#[derive(Debug, Clone)]
pub struct ActiveGoal {
    pub title: String,
    pub status: String,
    pub progress_percent: i32,
}

/// A recent decision for handoff context
#[derive(Debug, Clone)]
pub struct RecentDecision {
    pub value: String,
}

/// Reset tracking state
#[derive(Debug, Clone, Default)]
pub struct ResetTrackingState {
    pub consecutive_low_cache_turns: u32,
    pub turns_since_reset: u32,
}

/// Failure info for handoff
#[derive(Debug, Clone)]
pub struct FailureInfo {
    pub command: String,
    pub error: String,
}

// ============================================================================
// Response ID Operations
// ============================================================================

/// Set the previous response ID
pub async fn set_response_id(
    ctx: &OpContext,
    project_path: &str,
    response_id: &str,
) -> CoreResult<()> {
    let db = ctx.require_db()?;
    let now = Utc::now().timestamp();

    sqlx::query(
        r#"
        UPDATE chat_context
        SET last_response_id = $1, updated_at = $2
        WHERE project_path = $3
        "#,
    )
    .bind(response_id)
    .bind(now)
    .bind(project_path)
    .execute(db)
    .await?;

    Ok(())
}

/// Get the previous response ID
pub async fn get_response_id(
    ctx: &OpContext,
    project_path: &str,
) -> CoreResult<Option<String>> {
    let db = ctx.require_db()?;

    let row: Option<(Option<String>,)> = sqlx::query_as(
        "SELECT last_response_id FROM chat_context WHERE project_path = $1",
    )
    .bind(project_path)
    .fetch_optional(db)
    .await?;

    Ok(row.and_then(|(id,)| id))
}

/// Clear the response ID (no handoff)
pub async fn clear_response_id(
    ctx: &OpContext,
    project_path: &str,
) -> CoreResult<()> {
    let db = ctx.require_db()?;
    let now = Utc::now().timestamp();

    sqlx::query(
        r#"
        UPDATE chat_context
        SET last_response_id = NULL, updated_at = $1
        WHERE project_path = $2
        "#,
    )
    .bind(now)
    .bind(project_path)
    .execute(db)
    .await?;

    Ok(())
}

/// Clear the response ID and set handoff flag with blob
pub async fn clear_response_id_with_handoff(
    ctx: &OpContext,
    project_path: &str,
    handoff_blob: &str,
) -> CoreResult<()> {
    let db = ctx.require_db()?;
    let now = Utc::now().timestamp();

    sqlx::query(
        r#"
        UPDATE chat_context
        SET last_response_id = NULL, needs_handoff = 1, handoff_blob = $1, updated_at = $2
        WHERE project_path = $3
        "#,
    )
    .bind(handoff_blob)
    .bind(now)
    .bind(project_path)
    .execute(db)
    .await?;

    Ok(())
}

// ============================================================================
// Handoff Operations
// ============================================================================

/// Check if next request needs handoff context
pub async fn needs_handoff(ctx: &OpContext, project_path: &str) -> CoreResult<bool> {
    let db = ctx.require_db()?;

    let row: Option<(i32,)> = sqlx::query_as(
        "SELECT needs_handoff FROM chat_context WHERE project_path = $1",
    )
    .bind(project_path)
    .fetch_optional(db)
    .await?;

    Ok(row.map(|(v,)| v != 0).unwrap_or(false))
}

/// Get the handoff blob and clear the flag
pub async fn consume_handoff(
    ctx: &OpContext,
    project_path: &str,
) -> CoreResult<Option<String>> {
    let db = ctx.require_db()?;

    let row: Option<(Option<String>,)> = sqlx::query_as(
        "SELECT handoff_blob FROM chat_context WHERE project_path = $1 AND needs_handoff = 1",
    )
    .bind(project_path)
    .fetch_optional(db)
    .await?;

    if let Some((blob,)) = row {
        let now = Utc::now().timestamp();
        sqlx::query(
            "UPDATE chat_context SET needs_handoff = 0, handoff_blob = NULL, updated_at = $1 WHERE project_path = $2",
        )
        .bind(now)
        .bind(project_path)
        .execute(db)
        .await?;
        Ok(blob)
    } else {
        Ok(None)
    }
}

// ============================================================================
// Handoff Data Fetching (for building handoff blob)
// ============================================================================

/// Get recent chat messages for handoff
pub async fn get_recent_messages(
    ctx: &OpContext,
    limit: usize,
) -> CoreResult<Vec<RecentMessage>> {
    let db = ctx.require_db()?;

    let rows: Vec<(String, String, i64)> = sqlx::query_as(
        r#"
        SELECT role, blocks, created_at FROM chat_messages
        WHERE archived_at IS NULL
        ORDER BY created_at DESC
        LIMIT $1
        "#,
    )
    .bind(limit as i64)
    .fetch_all(db)
    .await?;

    Ok(rows
        .into_iter()
        .map(|(role, blocks_json, created_at)| RecentMessage {
            role,
            blocks_json,
            created_at,
        })
        .collect())
}

/// Get latest summary for handoff
pub async fn get_latest_summary(
    ctx: &OpContext,
    project_path: &str,
) -> CoreResult<Option<String>> {
    let db = ctx.require_db()?;

    let row: Option<(String,)> = sqlx::query_as(
        "SELECT summary FROM chat_summaries WHERE project_path = $1 ORDER BY created_at DESC LIMIT 1",
    )
    .bind(project_path)
    .fetch_optional(db)
    .await?;

    Ok(row.map(|(s,)| s))
}

/// Get active goals for handoff
pub async fn get_active_goals_for_handoff(
    ctx: &OpContext,
    project_path: &str,
    limit: usize,
) -> CoreResult<Vec<ActiveGoal>> {
    let db = ctx.require_db()?;

    let rows: Vec<(String, String, i32)> = sqlx::query_as(
        r#"
        SELECT title, status, progress_percent FROM goals
        WHERE project_id = (SELECT id FROM projects WHERE path = $1)
          AND status IN ('planning', 'in_progress', 'blocked')
        ORDER BY updated_at DESC LIMIT $2
        "#,
    )
    .bind(project_path)
    .bind(limit as i64)
    .fetch_all(db)
    .await?;

    Ok(rows
        .into_iter()
        .map(|(title, status, progress_percent)| ActiveGoal {
            title,
            status,
            progress_percent,
        })
        .collect())
}

/// Get recent decisions for handoff
pub async fn get_recent_decisions_for_handoff(
    ctx: &OpContext,
    project_path: &str,
    limit: usize,
) -> CoreResult<Vec<RecentDecision>> {
    let db = ctx.require_db()?;

    let rows: Vec<(String,)> = sqlx::query_as(
        r#"
        SELECT value FROM memory_facts
        WHERE (project_id = (SELECT id FROM projects WHERE path = $1) OR project_id IS NULL)
          AND fact_type = 'decision'
        ORDER BY updated_at DESC LIMIT $2
        "#,
    )
    .bind(project_path)
    .bind(limit as i64)
    .fetch_all(db)
    .await?;

    Ok(rows
        .into_iter()
        .map(|(value,)| RecentDecision { value })
        .collect())
}

// ============================================================================
// Reset Tracking Operations
// ============================================================================

/// Get current reset tracking state
pub async fn get_reset_tracking(
    ctx: &OpContext,
    project_path: &str,
) -> CoreResult<ResetTrackingState> {
    let db = ctx.require_db()?;

    let row: Option<(i32, i32)> = sqlx::query_as(
        "SELECT consecutive_low_cache_turns, turns_since_reset FROM chat_context WHERE project_path = $1",
    )
    .bind(project_path)
    .fetch_optional(db)
    .await?;

    Ok(row
        .map(|(c, t)| ResetTrackingState {
            consecutive_low_cache_turns: c as u32,
            turns_since_reset: t as u32,
        })
        .unwrap_or(ResetTrackingState {
            consecutive_low_cache_turns: 0,
            turns_since_reset: 999, // No cooldown on first run
        }))
}

/// Update consecutive low-cache counter
pub async fn update_consecutive_low_cache(
    ctx: &OpContext,
    project_path: &str,
    count: u32,
) -> CoreResult<()> {
    let db = ctx.require_db()?;
    let now = Utc::now().timestamp();

    sqlx::query(
        "UPDATE chat_context SET consecutive_low_cache_turns = $1, updated_at = $2 WHERE project_path = $3",
    )
    .bind(count as i32)
    .bind(now)
    .bind(project_path)
    .execute(db)
    .await?;

    Ok(())
}

/// Increment turns since last reset
pub async fn increment_turns_since_reset(
    ctx: &OpContext,
    project_path: &str,
) -> CoreResult<()> {
    let db = ctx.require_db()?;
    let now = Utc::now().timestamp();

    sqlx::query(
        "UPDATE chat_context SET turns_since_reset = turns_since_reset + 1, updated_at = $1 WHERE project_path = $2",
    )
    .bind(now)
    .bind(project_path)
    .execute(db)
    .await?;

    Ok(())
}

/// Record that a reset occurred (reset all counters)
pub async fn record_reset(ctx: &OpContext, project_path: &str) -> CoreResult<()> {
    let db = ctx.require_db()?;
    let now = Utc::now().timestamp();

    sqlx::query(
        r#"
        UPDATE chat_context
        SET consecutive_low_cache_turns = 0, turns_since_reset = 0, updated_at = $1
        WHERE project_path = $2
        "#,
    )
    .bind(now)
    .bind(project_path)
    .execute(db)
    .await?;

    Ok(())
}

// ============================================================================
// Failure Tracking Operations
// ============================================================================

/// Record a failure for handoff context
pub async fn record_failure(
    ctx: &OpContext,
    project_path: &str,
    command: &str,
    error: &str,
) -> CoreResult<()> {
    let db = ctx.require_db()?;
    let now = Utc::now().timestamp();

    sqlx::query(
        r#"
        UPDATE chat_context
        SET last_failure_command = $1, last_failure_error = $2, last_failure_at = $3, updated_at = $3
        WHERE project_path = $4
        "#,
    )
    .bind(command)
    .bind(error)
    .bind(now)
    .bind(project_path)
    .execute(db)
    .await?;

    Ok(())
}

/// Clear failure after success
pub async fn clear_failure(ctx: &OpContext, project_path: &str) -> CoreResult<()> {
    let db = ctx.require_db()?;
    let now = Utc::now().timestamp();

    sqlx::query(
        r#"
        UPDATE chat_context
        SET last_failure_command = NULL, last_failure_error = NULL, last_failure_at = NULL, updated_at = $1
        WHERE project_path = $2
        "#,
    )
    .bind(now)
    .bind(project_path)
    .execute(db)
    .await?;

    Ok(())
}

/// Get last failure (command, error)
pub async fn get_last_failure(
    ctx: &OpContext,
    project_path: &str,
) -> CoreResult<Option<FailureInfo>> {
    let db = ctx.require_db()?;

    let row: Option<(Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT last_failure_command, last_failure_error FROM chat_context WHERE project_path = $1",
    )
    .bind(project_path)
    .fetch_optional(db)
    .await?;

    Ok(row.and_then(|(c, e)| {
        c.zip(e).map(|(command, error)| FailureInfo { command, error })
    }))
}

// ============================================================================
// Artifact Tracking Operations
// ============================================================================

/// Get recent artifact IDs
pub async fn get_recent_artifact_ids(
    ctx: &OpContext,
    project_path: &str,
) -> CoreResult<Vec<String>> {
    let db = ctx.require_db()?;

    let row: Option<(Option<String>,)> = sqlx::query_as(
        "SELECT recent_artifact_ids FROM chat_context WHERE project_path = $1",
    )
    .bind(project_path)
    .fetch_optional(db)
    .await?;

    match row {
        Some((Some(json),)) => {
            let ids: Vec<String> = serde_json::from_str(&json).unwrap_or_default();
            Ok(ids)
        }
        _ => Ok(Vec::new()),
    }
}

/// Track an artifact ID (keeps last N)
pub async fn track_artifact(
    ctx: &OpContext,
    project_path: &str,
    artifact_id: &str,
    max_artifacts: usize,
) -> CoreResult<()> {
    let db = ctx.require_db()?;

    // Get current list
    let mut ids = get_recent_artifact_ids(ctx, project_path).await?;

    // Add new, keep last N
    ids.push(artifact_id.to_string());
    if ids.len() > max_artifacts {
        let skip_count = ids.len() - max_artifacts;
        ids = ids.into_iter().skip(skip_count).collect();
    }

    let now = Utc::now().timestamp();
    sqlx::query(
        "UPDATE chat_context SET recent_artifact_ids = $1, updated_at = $2 WHERE project_path = $3",
    )
    .bind(serde_json::to_string(&ids).unwrap_or_else(|_| "[]".to_string()))
    .bind(now)
    .bind(project_path)
    .execute(db)
    .await?;

    Ok(())
}
