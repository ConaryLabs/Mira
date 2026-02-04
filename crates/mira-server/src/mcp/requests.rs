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
    /// Generate LLM-powered summaries for codebase modules
    Summarize,
    /// Run a full code health scan (dependencies, patterns, tech debt, etc.)
    Health,
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
    /// Export Mira memories to CLAUDE.local.md
    ExportClaudeLocal,
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

#[derive(Debug, Clone, Copy, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MemoryAction {
    /// Store a fact for future recall
    Remember,
    /// Search memories using semantic similarity
    Recall,
    /// Delete a memory by ID
    Forget,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct MemoryRequest {
    #[schemars(description = "Action: remember, recall, forget")]
    pub action: MemoryAction,
    #[schemars(description = "Content to remember (required for remember)")]
    pub content: Option<String>,
    #[schemars(description = "Search query (required for recall)")]
    pub query: Option<String>,
    #[schemars(description = "Memory ID to delete (required for forget)")]
    pub id: Option<String>,
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
    #[schemars(description = "Max results")]
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Copy, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CodeAction {
    /// Search code by meaning
    Search,
    /// Get symbols from a file
    Symbols,
    /// Find all functions that call a given function
    Callers,
    /// Find all functions called by a given function
    Callees,
    /// Analyze module dependencies and detect circular dependencies
    Dependencies,
    /// Detect architectural patterns (repository, builder, factory, etc.)
    Patterns,
    /// Compute per-module tech debt scores
    TechDebt,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CodeRequest {
    #[schemars(
        description = "Action: search, symbols, callers, callees, dependencies, patterns, tech_debt"
    )]
    pub action: CodeAction,
    #[schemars(description = "Search query (required for search)")]
    pub query: Option<String>,
    #[schemars(description = "File path (required for symbols)")]
    pub file_path: Option<String>,
    #[schemars(description = "Function name (required for callers/callees)")]
    pub function_name: Option<String>,
    #[schemars(description = "Language filter")]
    pub language: Option<String>,
    #[schemars(description = "Symbol type filter")]
    pub symbol_type: Option<String>,
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
    #[schemars(description = "Action: project/file/status/compact/summarize/health")]
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

#[derive(Debug, Clone, Copy, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SessionAction {
    /// Query session history (list_sessions, get_history, current via history_action)
    History,
    /// Get session recap (preferences, recent context, goals)
    Recap,
    /// Query LLM usage analytics (summary, stats, list via usage_action)
    Usage,
    /// Query unified insights digest (pondering, proactive, doc gaps)
    Insights,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SessionRequest {
    #[schemars(description = "Action: history, recap, usage, insights")]
    pub action: SessionAction,
    #[schemars(description = "History sub-action: list_sessions/get_history/current")]
    pub history_action: Option<SessionHistoryAction>,
    #[schemars(description = "Usage sub-action: summary/stats/list")]
    pub usage_action: Option<UsageAction>,
    #[schemars(description = "Session ID (for get_history)")]
    pub session_id: Option<String>,
    #[schemars(description = "Max results")]
    pub limit: Option<i64>,
    #[schemars(
        description = "Group by: role, provider, model, or provider_model (for usage stats)"
    )]
    pub group_by: Option<String>,
    #[schemars(description = "Filter to last N days (default: 30)")]
    pub since_days: Option<u32>,
    #[schemars(
        description = "Filter insights by source: pondering/proactive/doc_gap (for insights action)"
    )]
    pub insight_source: Option<String>,
    #[schemars(description = "Minimum confidence threshold for insights (0.0-1.0, default: 0.3)")]
    pub min_confidence: Option<f64>,
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

#[derive(Debug, Clone, Copy, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExpertAction {
    /// Consult one or more experts in parallel
    Consult,
    /// Configure expert system prompts
    Configure,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ExpertRequest {
    #[schemars(description = "Action: consult, configure")]
    pub action: ExpertAction,
    #[schemars(
        description = "Expert roles to consult: architect, plan_reviewer, scope_analyst, code_reviewer, security (required for consult)"
    )]
    pub roles: Option<Vec<String>>,
    #[schemars(description = "Code, design, or situation to analyze (required for consult)")]
    pub context: Option<String>,
    #[schemars(description = "Specific question for all experts")]
    pub question: Option<String>,
    #[schemars(description = "Collaboration mode: parallel (default) or debate")]
    pub mode: Option<String>,
    #[schemars(
        description = "Configure sub-action: set/get/delete/list/providers (required for configure)"
    )]
    pub config_action: Option<ExpertConfigAction>,
    #[schemars(description = "Expert role for configuration")]
    pub role: Option<String>,
    #[schemars(description = "Custom system prompt (for configure set)")]
    pub prompt: Option<String>,
    #[schemars(description = "LLM provider: deepseek/gemini (for configure set)")]
    pub provider: Option<String>,
    #[schemars(description = "Custom model name, e.g. gemini-3-pro (for configure set)")]
    pub model: Option<String>,
}

// Documentation request types

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct DocumentationRequest {
    #[schemars(
        description = "Action: list, get, complete, skip, inventory, scan, export_claude_local"
    )]
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

// ============================================================================
// Tasks fallback tool (for clients without native task support)
// ============================================================================

#[derive(Debug, Clone, Copy, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TasksAction {
    /// List all running and recently completed tasks
    List,
    /// Get status and result of a specific task
    Get,
    /// Cancel a running task
    Cancel,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct TasksRequest {
    #[schemars(description = "Action: list, get (by task_id), cancel (by task_id)")]
    pub action: TasksAction,
    #[schemars(description = "Task ID (required for get and cancel)")]
    pub task_id: Option<String>,
}
