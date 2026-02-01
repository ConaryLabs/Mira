// crates/mira-server/src/mcp/requests.rs
// MCP tool request types

use rmcp::schemars;
use serde::Deserialize;

// ============================================================================
// Action Enums - typed alternatives to stringly-typed action parameters
// ============================================================================

#[derive(Debug, Clone, Copy, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProjectAction {
    /// Initialize session with project context
    Start,
    /// Change active project
    Set,
    /// Show current project
    Get,
}

#[derive(Debug, Clone, Copy, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GoalAction {
    /// Get a goal by ID
    Get,
    /// Create a new goal
    Create,
    /// Create multiple goals at once
    BulkCreate,
    /// List goals
    List,
    /// Update a goal
    Update,
    /// Update goal progress
    Progress,
    /// Delete a goal
    Delete,
    /// Add a milestone to a goal
    AddMilestone,
    /// Mark a milestone as complete
    CompleteMilestone,
    /// Delete a milestone
    DeleteMilestone,
}

#[derive(Debug, Clone, Copy, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SessionHistoryAction {
    /// Show current session
    Current,
    /// List recent sessions
    ListSessions,
    /// Get history for a session
    GetHistory,
}

#[derive(Debug, Clone, Copy, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum IndexAction {
    /// Index entire project
    Project,
    /// Index a single file
    File,
    /// Show index status
    Status,
    /// Compact vec_code storage and VACUUM
    Compact,
}

#[derive(Debug, Clone, Copy, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TeamAction {
    /// Create a new team
    Create,
    /// Invite a user to a team
    Invite,
    /// Alias for invite
    Add,
    /// Remove a user from a team
    Remove,
    /// List teams
    List,
    /// List team members
    Members,
}

#[derive(Debug, Clone, Copy, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CrossProjectAction {
    /// Get sharing preferences
    GetPreferences,
    /// Alias for get_preferences
    Status,
    /// Enable pattern sharing
    EnableSharing,
    /// Disable pattern sharing
    DisableSharing,
    /// Reset privacy budget
    ResetBudget,
    /// Get sharing statistics
    GetStats,
    /// Extract patterns from project
    ExtractPatterns,
    /// Sync patterns with network
    Sync,
}

#[derive(Debug, Clone, Copy, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExpertConfigAction {
    /// Set expert configuration
    Set,
    /// Get expert configuration
    Get,
    /// Delete expert configuration
    Delete,
    /// List all configurations
    List,
    /// List available providers
    Providers,
}

#[derive(Debug, Clone, Copy, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DocumentationAction {
    /// List documentation tasks
    List,
    /// Get full task details with writing guidelines
    Get,
    /// Mark a task as complete (after Claude writes the doc)
    Complete,
    /// Skip a documentation task
    Skip,
    /// Show documentation inventory
    Inventory,
    /// Trigger documentation scan
    Scan,
}

#[derive(Debug, Clone, Copy, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FindingAction {
    /// List findings
    List,
    /// Get a finding by ID
    Get,
    /// Review a finding
    Review,
    /// Get finding statistics
    Stats,
    /// Get learned patterns
    Patterns,
    /// Extract patterns from findings
    Extract,
}

#[derive(Debug, Clone, Copy, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum UsageAction {
    /// Get usage summary
    Summary,
    /// Get usage stats grouped by dimension
    Stats,
    /// List recent usage records
    List,
}

// ============================================================================
// Request Structs
// ============================================================================

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ProjectRequest {
    #[schemars(
        description = "Action: start (initialize session), set (change project), get (show current)"
    )]
    pub action: ProjectAction,
    #[schemars(description = "Project root path (required for start/set)")]
    pub project_path: Option<String>,
    #[schemars(description = "Project name")]
    pub name: Option<String>,
    #[schemars(description = "Optional session ID (for start action)")]
    pub session_id: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct RememberRequest {
    #[schemars(description = "Content to remember")]
    pub content: String,
    #[schemars(description = "Key for upsert")]
    pub key: Option<String>,
    #[schemars(description = "Type: preference/decision/context/general")]
    pub fact_type: Option<String>,
    #[schemars(description = "Category")]
    pub category: Option<String>,
    #[schemars(description = "Confidence score (0.0-1.0)")]
    pub confidence: Option<f64>,
    #[schemars(
        description = "Visibility scope: personal (only creator), project (default, anyone with project access), team (team members only)"
    )]
    pub scope: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct RecallRequest {
    #[schemars(description = "Search query")]
    pub query: String,
    #[schemars(description = "Max results")]
    pub limit: Option<i64>,
    #[schemars(description = "Filter by category")]
    pub category: Option<String>,
    #[schemars(description = "Filter by type")]
    pub fact_type: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ForgetRequest {
    #[schemars(description = "Memory ID to delete")]
    pub id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetSymbolsRequest {
    #[schemars(description = "File path")]
    pub file_path: String,
    #[schemars(description = "Symbol type")]
    pub symbol_type: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SemanticCodeSearchRequest {
    #[schemars(description = "Query")]
    pub query: String,
    #[schemars(description = "Language")]
    pub language: Option<String>,
    #[schemars(description = "Max results")]
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct FindCallersRequest {
    #[schemars(description = "Function name to find callers for")]
    pub function_name: String,
    #[schemars(description = "Max results")]
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct FindCalleesRequest {
    #[schemars(description = "Function name to find callees for")]
    pub function_name: String,
    #[schemars(description = "Max results")]
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GoalRequest {
    #[schemars(
        description = "Action: create/bulk_create/list/get/update/delete/add_milestone/complete_milestone/delete_milestone/progress"
    )]
    pub action: GoalAction,
    #[schemars(description = "Goal ID")]
    pub goal_id: Option<String>,
    #[schemars(description = "Title")]
    pub title: Option<String>,
    #[schemars(description = "Description")]
    pub description: Option<String>,
    #[schemars(description = "Status: planning/in_progress/blocked/completed/abandoned")]
    pub status: Option<String>,
    #[schemars(description = "Priority: low/medium/high/critical")]
    pub priority: Option<String>,
    #[schemars(description = "Success criteria")]
    pub success_criteria: Option<String>,
    #[schemars(description = "Progress percent (0-100)")]
    pub progress_percent: Option<i32>,
    #[schemars(description = "Include finished goals")]
    pub include_finished: Option<bool>,
    #[schemars(description = "Milestone ID (for complete_milestone/delete_milestone)")]
    pub milestone_id: Option<String>,
    #[schemars(description = "Milestone title (for add_milestone)")]
    pub milestone_title: Option<String>,
    #[schemars(description = "Milestone weight (for add_milestone, default: 1)")]
    pub weight: Option<i32>,
    #[schemars(description = "Max results")]
    pub limit: Option<i64>,
    #[schemars(
        description = "For bulk_create: JSON array of goals [{title, description?, priority?}, ...]"
    )]
    pub goals: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CrossProjectRequest {
    #[schemars(
        description = "Action: get_preferences/status/enable_sharing/disable_sharing/reset_budget/get_stats/extract_patterns/sync"
    )]
    pub action: CrossProjectAction,
    #[schemars(description = "Enable pattern export (for enable_sharing)")]
    pub export: Option<bool>,
    #[schemars(description = "Enable pattern import (for enable_sharing)")]
    pub import: Option<bool>,
    #[schemars(description = "Minimum confidence for pattern extraction (default: 0.6)")]
    pub min_confidence: Option<f64>,
    #[schemars(description = "Privacy budget epsilon (default: 1.0)")]
    pub epsilon: Option<f64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct IndexRequest {
    #[schemars(description = "Action: project/file/status/compact")]
    pub action: IndexAction,
    #[schemars(description = "Path")]
    pub path: Option<String>,
    #[schemars(description = "Commit limit")]
    pub commit_limit: Option<i64>,
    #[schemars(description = "Parallel")]
    pub parallel: Option<bool>,
    #[schemars(description = "Max workers")]
    pub max_workers: Option<i64>,
    #[schemars(description = "Skip embedding generation (faster indexing)")]
    pub skip_embed: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SessionHistoryRequest {
    #[schemars(description = "Action: list_sessions/get_history/current")]
    pub action: SessionHistoryAction,
    #[schemars(description = "Session ID (for get_history)")]
    pub session_id: Option<String>,
    #[schemars(description = "Max results")]
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ReplyToMiraRequest {
    #[schemars(description = "The message_id you are replying to")]
    pub in_reply_to: String,
    #[schemars(description = "Your response content")]
    pub content: String,
    #[schemars(
        description = "Is your response complete? Set to false if you need more information."
    )]
    pub complete: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ConsultExpertsRequest {
    #[schemars(
        description = "List of expert roles to consult in parallel: architect, plan_reviewer, scope_analyst, code_reviewer, security"
    )]
    pub roles: Vec<String>,
    #[schemars(description = "Code, design, or situation to analyze")]
    pub context: String,
    #[schemars(description = "Specific question for all experts (optional)")]
    pub question: Option<String>,
    #[schemars(description = "Collaboration mode: parallel (default) or debate")]
    pub mode: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ConfigureExpertRequest {
    #[schemars(description = "Action: set/get/delete/list/providers")]
    pub action: ExpertConfigAction,
    #[schemars(
        description = "Expert role: architect/plan_reviewer/scope_analyst/code_reviewer/security"
    )]
    pub role: Option<String>,
    #[schemars(description = "Custom system prompt (for 'set' action)")]
    pub prompt: Option<String>,
    #[schemars(description = "LLM provider: deepseek/gemini (for 'set' action)")]
    pub provider: Option<String>,
    #[schemars(description = "Custom model name, e.g. gemini-3-pro (for 'set' action)")]
    pub model: Option<String>,
}

// Documentation request types

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct DocumentationRequest {
    #[schemars(description = "Action: list, get, complete, skip, inventory, scan")]
    pub action: DocumentationAction,
    #[schemars(description = "Task ID (for get/complete/skip actions)")]
    pub task_id: Option<i64>,
    #[schemars(description = "Reason for skipping (for skip action)")]
    pub reason: Option<String>,
    #[schemars(description = "Filter by documentation type")]
    pub doc_type: Option<String>,
    #[schemars(description = "Filter by priority")]
    pub priority: Option<String>,
    #[schemars(description = "Filter by status")]
    pub status: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct TeamRequest {
    #[schemars(description = "Action: create/invite/remove/list/members")]
    pub action: TeamAction,
    #[schemars(description = "Team ID (for invite/remove/members actions)")]
    pub team_id: Option<i64>,
    #[schemars(description = "Team name (for create action)")]
    pub name: Option<String>,
    #[schemars(description = "Team description (for create action)")]
    pub description: Option<String>,
    #[schemars(description = "User identity to invite/remove")]
    pub user_identity: Option<String>,
    #[schemars(description = "Role for invited user: member/admin (default: member)")]
    pub role: Option<String>,
}

// Review findings request type (unified)

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct FindingRequest {
    #[schemars(description = "Action: list, get, review, stats, patterns, extract")]
    pub action: FindingAction,
    #[schemars(description = "Finding ID (for get/review single)")]
    pub finding_id: Option<i64>,
    #[schemars(description = "Finding IDs for bulk review")]
    pub finding_ids: Option<Vec<i64>>,
    #[schemars(
        description = "Status filter (for list) or new status (for review): pending/accepted/rejected/fixed"
    )]
    pub status: Option<String>,
    #[schemars(description = "Feedback explaining review decision (helps learning)")]
    pub feedback: Option<String>,
    #[schemars(description = "Filter by file path")]
    pub file_path: Option<String>,
    #[schemars(description = "Filter by expert role: code_reviewer/security/architect/etc.")]
    pub expert_role: Option<String>,
    #[schemars(
        description = "Filter by correction type: bug/style/security/performance (for patterns)"
    )]
    pub correction_type: Option<String>,
    #[schemars(description = "Max results (default: 20)")]
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct AnalyzeDiffRequest {
    #[schemars(
        description = "Starting git ref (commit, branch, tag). Default: HEAD~1 for commits, or analyzes staged/working changes if present"
    )]
    pub from_ref: Option<String>,
    #[schemars(description = "Ending git ref. Default: HEAD")]
    pub to_ref: Option<String>,
    #[schemars(description = "Include impact analysis (find affected callers). Default: true")]
    pub include_impact: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct UsageRequest {
    #[schemars(
        description = "Action: summary (totals), stats (grouped by dimension), list (recent records)"
    )]
    pub action: UsageAction,
    #[schemars(
        description = "Group by: role, provider, model, or provider_model (for stats action)"
    )]
    pub group_by: Option<String>,
    #[schemars(description = "Filter to last N days (default: 30)")]
    pub since_days: Option<u32>,
    #[schemars(description = "Max results for list action (default: 50)")]
    pub limit: Option<i64>,
}
