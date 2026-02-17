// db/types.rs
// Data structures returned by database operations

// Note: MemoryFact is in mira_types (shared crate)
// Use parse_memory_fact_row() from db/memory.rs for row parsing

/// Tool history entry
#[derive(Debug, Clone)]
pub struct ToolHistoryEntry {
    pub id: i64,
    pub session_id: String,
    pub tool_name: String,
    pub arguments: Option<String>,
    pub result_summary: Option<String>,
    pub full_result: Option<String>,
    pub success: bool,
    pub created_at: String,
}

/// Session info
#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub id: String,
    pub project_id: Option<i64>,
    pub status: String,
    pub summary: Option<String>,
    pub started_at: String,
    pub last_activity: String,
    pub source: Option<String>,
    pub resumed_from: Option<String>,
}

/// Chat message record
#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub id: i64,
    pub role: String,
    pub content: String,
    pub reasoning_content: Option<String>,
    pub created_at: String,
}

/// Chat summary record
#[derive(Debug, Clone)]
pub struct ChatSummary {
    pub id: i64,
    pub project_id: Option<i64>,
    pub summary: String,
    pub message_range_start: i64,
    pub message_range_end: i64,
    pub summary_level: i32,
    pub created_at: String,
}

/// Task record
#[derive(Debug, Clone)]
pub struct Task {
    pub id: i64,
    pub project_id: Option<i64>,
    pub goal_id: Option<i64>,
    pub title: String,
    pub description: Option<String>,
    pub status: String,
    pub priority: String,
    pub created_at: String,
}

/// Goal record
#[derive(Debug, Clone)]
pub struct Goal {
    pub id: i64,
    pub project_id: Option<i64>,
    pub title: String,
    pub description: Option<String>,
    pub status: String,
    pub priority: String,
    pub progress_percent: i32,
    pub created_at: String,
}

/// Session-goal link record (junction table)
#[derive(Debug, Clone)]
pub struct SessionGoalLink {
    pub id: i64,
    pub session_id: String,
    pub goal_id: i64,
    pub interaction_type: String,
    pub created_at: String,
}

/// Milestone record (sub-item of a goal)
#[derive(Debug, Clone)]
pub struct Milestone {
    pub id: i64,
    pub goal_id: Option<i64>,
    pub title: String,
    pub completed: bool,
    pub weight: i32,
    pub created_at: Option<String>,
    pub completed_at: Option<String>,
    pub completed_in_session_id: Option<String>,
}

/// Unified insight from pondering, proactive suggestions, or doc gaps
#[derive(Debug, Clone, serde::Serialize)]
pub struct UnifiedInsight {
    pub source: String,
    pub source_type: String,
    pub description: String,
    pub priority_score: f64,
    pub confidence: f64,
    pub timestamp: String,
    pub evidence: Option<String>,
    /// Row ID from behavior_patterns (pondering) for marking as shown
    pub row_id: Option<i64>,
    /// Trend direction for health insights: "improved", "degraded", "stable", or "baseline"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trend: Option<String>,
    /// Human-readable change summary, e.g. "B → C" or "42.3 → 58.1"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub change_summary: Option<String>,
    /// Dashboard category for grouping: "attention", "quality", "testing", "workflow", "documentation", "health"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
}

/// A point-in-time snapshot of codebase health metrics
#[derive(Debug, Clone, serde::Serialize)]
pub struct HealthSnapshot {
    pub id: i64,
    pub avg_debt_score: f64,
    pub max_debt_score: f64,
    pub tier_distribution: String,
    pub module_count: i64,
    pub snapshot_at: String,
    pub warning_count: i64,
    pub todo_count: i64,
    pub unwrap_count: i64,
    pub error_handling_count: i64,
    pub total_finding_count: i64,
}

/// Project briefing (What's New since last session)
#[derive(Debug, Clone)]
pub struct ProjectBriefing {
    pub project_id: i64,
    pub last_known_commit: Option<String>,
    pub last_session_at: Option<String>,
    pub briefing_text: Option<String>,
    pub generated_at: Option<String>,
}
