// background/pondering/types.rs
// Shared types for pondering submodules

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct ToolUsageEntry {
    pub tool_name: String,
    pub arguments_summary: String,
    pub success: bool,
    pub timestamp: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct PonderingInsight {
    pub pattern_type: String,
    pub description: String,
    #[serde(default = "default_confidence")]
    pub confidence: f64,
    pub evidence: Vec<String>,
}

fn default_confidence() -> f64 {
    0.7
}

/// Container for all project-aware insight data.
/// Replaces tool-count-only data with rich, actionable project signals.
#[derive(Debug, Default)]
pub(crate) struct ProjectInsightData {
    pub stale_goals: Vec<StaleGoal>,
    pub fragile_modules: Vec<FragileModule>,
    pub revert_clusters: Vec<RevertCluster>,
    pub recurring_errors: Vec<RecurringError>,
}

impl ProjectInsightData {
    /// Returns true if at least one field has data worth analyzing.
    pub fn has_data(&self) -> bool {
        !self.stale_goals.is_empty()
            || !self.fragile_modules.is_empty()
            || !self.revert_clusters.is_empty()
            || !self.recurring_errors.is_empty()
    }
}

/// A goal stuck in `in_progress` status for too long.
#[derive(Debug)]
pub(crate) struct StaleGoal {
    pub goal_id: i64,
    pub title: String,
    pub status: String,
    pub progress_percent: i32,
    pub days_since_update: i64,
    pub milestones_total: i64,
    pub milestones_completed: i64,
}

/// A module (top-level directory) with a high rate of reverts or follow-up fixes.
#[derive(Debug)]
pub(crate) struct FragileModule {
    pub module: String,
    pub total_changes: i64,
    pub reverted: i64,
    pub follow_up_fixes: i64,
    pub bad_rate: f64,
}

/// Multiple reverts in the same module within a short time window.
#[derive(Debug)]
pub(crate) struct RevertCluster {
    pub module: String,
    pub revert_count: i64,
    pub timespan_hours: i64,
    pub commits: Vec<String>,
}

/// An error that recurs across multiple sessions without resolution.
#[derive(Debug)]
pub(crate) struct RecurringError {
    pub tool_name: String,
    pub error_template: String,
    pub occurrence_count: i64,
    #[allow(dead_code)] // Stored for future cross-session linking
    pub first_seen_session_id: Option<String>,
    #[allow(dead_code)] // Stored for future cross-session linking
    pub last_seen_session_id: Option<String>,
}
