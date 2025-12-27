//! MCP Session tracking and phase detection
//!
//! Tracks Claude Code sessions with lifecycle management and
//! phase detection for context-aware responses.

use std::collections::HashSet;
use chrono::Utc;
use serde::{Deserialize, Serialize};

use super::super::{CoreResult, OpContext};

// ============================================================================
// Session Phase
// ============================================================================

/// Session phase based on activity patterns
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionPhase {
    /// Initial exploration: reading, searching, understanding
    Early,
    /// Active implementation: editing, building, testing
    Middle,
    /// Refinement: fixing issues, polishing
    Late,
    /// Wrapping up: commits, summaries, cleanup
    Wrapping,
}

impl SessionPhase {
    pub fn as_str(&self) -> &'static str {
        match self {
            SessionPhase::Early => "early",
            SessionPhase::Middle => "middle",
            SessionPhase::Late => "late",
            SessionPhase::Wrapping => "wrapping",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "early" => Some(SessionPhase::Early),
            "middle" => Some(SessionPhase::Middle),
            "late" => Some(SessionPhase::Late),
            "wrapping" => Some(SessionPhase::Wrapping),
            _ => None,
        }
    }

    /// Detect phase from session metrics
    pub fn detect(metrics: &SessionMetrics) -> Self {
        // Wrapping: commits made, high completion signals
        if metrics.commit_count > 0 && metrics.write_count > 5 {
            return SessionPhase::Wrapping;
        }

        // Early: mostly reads, minimal writes
        if metrics.write_count == 0 && metrics.read_count > 3 {
            return SessionPhase::Early;
        }
        if metrics.tool_call_count < 10 && metrics.write_count < 3 {
            return SessionPhase::Early;
        }

        // Late: builds passing, low error rate, refinement
        if metrics.build_count > 2 && metrics.error_count < metrics.build_count / 2 {
            if metrics.write_count > 10 {
                return SessionPhase::Late;
            }
        }

        // Default to middle during active work
        SessionPhase::Middle
    }

    /// Progress estimate (0.0 - 1.0) based on phase
    pub fn progress_estimate(&self) -> f32 {
        match self {
            SessionPhase::Early => 0.1,
            SessionPhase::Middle => 0.5,
            SessionPhase::Late => 0.85,
            SessionPhase::Wrapping => 0.95,
        }
    }
}

impl Default for SessionPhase {
    fn default() -> Self {
        SessionPhase::Early
    }
}

impl std::fmt::Display for SessionPhase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

// ============================================================================
// Session Metrics
// ============================================================================

/// Metrics for phase detection
#[derive(Debug, Clone, Default)]
pub struct SessionMetrics {
    pub tool_call_count: u32,
    pub read_count: u32,
    pub write_count: u32,
    pub build_count: u32,
    pub error_count: u32,
    pub commit_count: u32,
}

impl SessionMetrics {
    /// Classify a tool call as read or write
    /// Handles both Mira MCP tools and Claude Code built-in tools
    pub fn classify_tool(tool_name: &str) -> ToolType {
        // Strip mcp__mira__ prefix if present
        let name = tool_name.strip_prefix("mcp__mira__").unwrap_or(tool_name);

        match name {
            // === Mira MCP Read operations ===
            "recall" | "get_symbols" | "get_call_graph" | "semantic_code_search"
            | "get_recent_commits" | "search_commits" | "find_cochange_patterns"
            | "get_related_files" | "get_guidelines" | "get_session_context"
            | "get_proactive_context" | "get_codebase_style" | "search_sessions"
            | "get_work_state" | "search_mcp_history" | "get_project"
            | "get_pending_instructions" | "carousel" | "query" | "list_tables"
            | "debounce" | "document" | "track_activity" => ToolType::Read,

            // === Mira MCP Write/mutation operations ===
            "remember" | "forget" | "store_session" | "store_decision"
            | "add_guideline" | "goal" | "task" | "correction"
            | "record_error_fix" | "record_rejected_approach" | "permission"
            | "sync_work_state" | "mark_instruction" | "proposal"
            | "set_project" | "session_start" | "index" | "file_search" => ToolType::Write,

            // === Mira MCP Build operations ===
            "build" | "find_similar_fixes" => ToolType::Build,

            // === Claude Code built-in tools ===
            // Read operations
            "Read" | "Glob" | "Grep" | "WebFetch" | "WebSearch"
            | "Task" | "TaskOutput" | "AskUserQuestion" => ToolType::Read,

            // Write operations
            "Edit" | "Write" | "NotebookEdit" | "TodoWrite" => ToolType::Write,

            // Build operations (Bash with build commands detected separately)
            "Bash" => ToolType::Build,

            // Plan mode
            "EnterPlanMode" | "ExitPlanMode" => ToolType::Read,

            // Unknown - default to read
            _ => ToolType::Read,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolType {
    Read,
    Write,
    Build,
}

// ============================================================================
// MCP Session Record
// ============================================================================

/// Full MCP session record from database
#[derive(Debug, Clone)]
pub struct McpSession {
    pub id: String,
    pub project_id: Option<i64>,
    pub phase: SessionPhase,
    pub started_at: i64,
    pub last_activity: i64,
    pub tool_call_count: i32,
    pub read_count: i32,
    pub write_count: i32,
    pub build_count: i32,
    pub error_count: i32,
    pub commit_count: i32,
    pub estimated_progress: f32,
    pub active_goal_id: Option<String>,
    pub status: SessionStatus,
    pub touched_files: HashSet<String>,
    pub topics: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionStatus {
    Active,
    Idle,
    Ended,
}

impl SessionStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            SessionStatus::Active => "active",
            SessionStatus::Idle => "idle",
            SessionStatus::Ended => "ended",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "active" => SessionStatus::Active,
            "idle" => SessionStatus::Idle,
            "ended" => SessionStatus::Ended,
            _ => SessionStatus::Active,
        }
    }
}

// ============================================================================
// Operations
// ============================================================================

/// Create or update an MCP session
pub async fn upsert_mcp_session(
    ctx: &OpContext,
    session_id: &str,
    project_id: Option<i64>,
) -> CoreResult<McpSession> {
    let db = ctx.require_db()?;
    let now = Utc::now().timestamp();

    sqlx::query(r#"
        INSERT INTO mcp_sessions (id, project_id, started_at, last_activity)
        VALUES ($1, $2, $3, $3)
        ON CONFLICT(id) DO UPDATE SET
            project_id = COALESCE(excluded.project_id, mcp_sessions.project_id),
            last_activity = excluded.last_activity,
            status = 'active'
    "#)
    .bind(session_id)
    .bind(project_id)
    .bind(now)
    .execute(db)
    .await?;

    get_mcp_session(ctx, session_id).await
}

/// Get an MCP session by ID
pub async fn get_mcp_session(ctx: &OpContext, session_id: &str) -> CoreResult<McpSession> {
    let db = ctx.require_db()?;

    let row = sqlx::query_as::<_, (
        String,           // id
        Option<i64>,      // project_id
        String,           // phase
        i64,              // started_at
        i64,              // last_activity
        i32,              // tool_call_count
        i32,              // read_count
        i32,              // write_count
        i32,              // build_count
        i32,              // error_count
        i32,              // commit_count
        f64,              // estimated_progress
        Option<String>,   // active_goal_id
        String,           // status
        Option<String>,   // touched_files (JSON)
        Option<String>,   // topics (JSON)
    )>(r#"
        SELECT id, project_id, phase, started_at, last_activity,
               tool_call_count, read_count, write_count, build_count,
               error_count, commit_count, estimated_progress, active_goal_id,
               status, touched_files, topics
        FROM mcp_sessions
        WHERE id = $1
    "#)
    .bind(session_id)
    .fetch_one(db)
    .await?;

    let touched_files: HashSet<String> = row.14
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();

    let topics: Vec<String> = row.15
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();

    Ok(McpSession {
        id: row.0,
        project_id: row.1,
        phase: SessionPhase::from_str(&row.2).unwrap_or_default(),
        started_at: row.3,
        last_activity: row.4,
        tool_call_count: row.5,
        read_count: row.6,
        write_count: row.7,
        build_count: row.8,
        error_count: row.9,
        commit_count: row.10,
        estimated_progress: row.11 as f32,
        active_goal_id: row.12,
        status: SessionStatus::from_str(&row.13),
        touched_files,
        topics,
    })
}

/// Record a tool call and update session metrics
pub async fn record_tool_call(
    ctx: &OpContext,
    session_id: &str,
    tool_name: &str,
    success: bool,
) -> CoreResult<SessionPhase> {
    let db = ctx.require_db()?;
    let now = Utc::now().timestamp();

    let tool_type = SessionMetrics::classify_tool(tool_name);
    let is_error = !success;

    // Update metrics based on tool type
    let (read_inc, write_inc, build_inc, error_inc) = match tool_type {
        ToolType::Read => (1, 0, 0, if is_error { 1 } else { 0 }),
        ToolType::Write => (0, 1, 0, if is_error { 1 } else { 0 }),
        ToolType::Build => (0, 0, 1, if is_error { 1 } else { 0 }),
    };

    sqlx::query(r#"
        UPDATE mcp_sessions SET
            last_activity = $2,
            tool_call_count = tool_call_count + 1,
            read_count = read_count + $3,
            write_count = write_count + $4,
            build_count = build_count + $5,
            error_count = error_count + $6,
            status = 'active'
        WHERE id = $1
    "#)
    .bind(session_id)
    .bind(now)
    .bind(read_inc)
    .bind(write_inc)
    .bind(build_inc)
    .bind(error_inc)
    .execute(db)
    .await?;

    // Get updated metrics and detect phase
    let session = get_mcp_session(ctx, session_id).await?;
    let metrics = SessionMetrics {
        tool_call_count: session.tool_call_count as u32,
        read_count: session.read_count as u32,
        write_count: session.write_count as u32,
        build_count: session.build_count as u32,
        error_count: session.error_count as u32,
        commit_count: session.commit_count as u32,
    };

    let new_phase = SessionPhase::detect(&metrics);

    // Update phase if changed
    if new_phase != session.phase {
        sqlx::query("UPDATE mcp_sessions SET phase = $2, estimated_progress = $3 WHERE id = $1")
            .bind(session_id)
            .bind(new_phase.as_str())
            .bind(new_phase.progress_estimate())
            .execute(db)
            .await?;
    }

    Ok(new_phase)
}

/// Record a file touch (for session context boost)
pub async fn record_file_touch(
    ctx: &OpContext,
    session_id: &str,
    file_path: &str,
) -> CoreResult<()> {
    let db = ctx.require_db()?;

    // Get current touched files
    let row: Option<(Option<String>,)> = sqlx::query_as(
        "SELECT touched_files FROM mcp_sessions WHERE id = $1"
    )
    .bind(session_id)
    .fetch_optional(db)
    .await?;

    if let Some((touched_json,)) = row {
        let mut files: HashSet<String> = touched_json
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();

        files.insert(file_path.to_string());

        // Keep only last 100 files
        let files_vec: Vec<_> = files.into_iter().take(100).collect();
        let new_json = serde_json::to_string(&files_vec)?;

        sqlx::query("UPDATE mcp_sessions SET touched_files = $2 WHERE id = $1")
            .bind(session_id)
            .bind(new_json)
            .execute(db)
            .await?;
    }

    Ok(())
}

/// Find resumable session for a project
pub async fn find_resumable_session(
    ctx: &OpContext,
    project_id: i64,
    max_age_hours: i64,
) -> CoreResult<Option<McpSession>> {
    let db = ctx.require_db()?;
    let cutoff = Utc::now().timestamp() - (max_age_hours * 3600);

    let row = sqlx::query_as::<_, (String,)>(r#"
        SELECT id
        FROM mcp_sessions
        WHERE project_id = $1
          AND status != 'ended'
          AND last_activity > $2
        ORDER BY last_activity DESC
        LIMIT 1
    "#)
    .bind(project_id)
    .bind(cutoff)
    .fetch_optional(db)
    .await?;

    if let Some((session_id,)) = row {
        let session = get_mcp_session(ctx, &session_id).await?;
        Ok(Some(session))
    } else {
        Ok(None)
    }
}

/// End an MCP session
pub async fn end_mcp_session(
    ctx: &OpContext,
    session_id: &str,
    reason: Option<&str>,
) -> CoreResult<()> {
    let db = ctx.require_db()?;
    let now = Utc::now().timestamp();

    sqlx::query(r#"
        UPDATE mcp_sessions SET
            status = 'ended',
            last_activity = $2,
            end_reason = $3,
            phase = 'wrapping'
        WHERE id = $1
    "#)
    .bind(session_id)
    .bind(now)
    .bind(reason)
    .execute(db)
    .await?;

    Ok(())
}

// ============================================================================
// Resume Context
// ============================================================================

/// Context for resuming a previous session
#[derive(Debug, Clone, Serialize)]
pub struct ResumeContext {
    pub previous_session_id: String,
    pub previous_phase: SessionPhase,
    pub progress_percent: f32,
    pub tool_call_count: i32,
    pub last_activity_ago_mins: i64,
    pub touched_files: Vec<String>,
    pub topics: Vec<String>,
    pub active_goal_id: Option<String>,
}

impl ResumeContext {
    pub fn from_session(session: &McpSession) -> Self {
        let now = Utc::now().timestamp();
        let ago_mins = (now - session.last_activity) / 60;

        ResumeContext {
            previous_session_id: session.id.clone(),
            previous_phase: session.phase,
            progress_percent: session.estimated_progress * 100.0,
            tool_call_count: session.tool_call_count,
            last_activity_ago_mins: ago_mins,
            touched_files: session.touched_files.iter().take(10).cloned().collect(),
            topics: session.topics.clone(),
            active_goal_id: session.active_goal_id.clone(),
        }
    }
}
