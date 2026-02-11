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
pub enum DocumentationAction {
    /// List documentation tasks
    List,
    /// Get full task details with writing guidelines
    Get,
    /// Mark a task as complete (after Claude writes the doc)
    Complete,
    /// Skip a documentation task
    Skip,
    /// Skip multiple documentation tasks by IDs or filter
    BatchSkip,
    /// Show documentation inventory
    Inventory,
    /// Trigger documentation scan
    Scan,
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
    /// Archive a memory (exclude from auto-export, keep for history)
    Archive,
    /// Export Mira memories to CLAUDE.local.md
    ExportClaudeLocal,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct MemoryRequest {
    #[schemars(description = "Action: remember, recall, forget, archive, export_claude_local")]
    pub action: MemoryAction,
    #[schemars(description = "Content to remember (required for remember)")]
    pub content: Option<String>,
    #[schemars(description = "Search query (required for recall)")]
    pub query: Option<String>,
    #[schemars(description = "Memory ID to delete (required for forget)")]
    pub id: Option<i64>,
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
    /// Analyze git diff semantically (change types, impact, risks)
    Diff,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CodeRequest {
    #[schemars(
        description = "Action: search, symbols, callers, callees, dependencies, patterns, tech_debt, diff"
    )]
    pub action: CodeAction,
    #[schemars(description = "Search query (required for search)")]
    pub query: Option<String>,
    #[schemars(description = "File path (required for symbols)")]
    pub file_path: Option<String>,
    #[schemars(description = "Function name (required for callers/callees)")]
    pub function_name: Option<String>,
    #[schemars(description = "Symbol type filter")]
    pub symbol_type: Option<String>,
    #[schemars(description = "Max results")]
    pub limit: Option<i64>,
    #[schemars(
        description = "Starting git ref for diff (commit, branch, tag). Default: HEAD~1 or staged/working changes"
    )]
    pub from_ref: Option<String>,
    #[schemars(description = "Ending git ref for diff. Default: HEAD")]
    pub to_ref: Option<String>,
    #[schemars(
        description = "Include impact analysis in diff (find affected callers). Default: true"
    )]
    pub include_impact: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GoalRequest {
    #[schemars(
        description = "Action: create/bulk_create/list/get/update/delete/add_milestone/complete_milestone/delete_milestone/progress"
    )]
    pub action: GoalAction,
    #[schemars(description = "Goal ID")]
    pub goal_id: Option<i64>,
    #[schemars(description = "Title")]
    pub title: Option<String>,
    #[schemars(description = "Description")]
    pub description: Option<String>,
    #[schemars(description = "Status: planning/in_progress/blocked/completed/abandoned")]
    pub status: Option<String>,
    #[schemars(description = "Priority: low/medium/high/critical")]
    pub priority: Option<String>,
    #[schemars(description = "Progress percent (0-100)")]
    pub progress_percent: Option<i32>,
    #[schemars(description = "Include finished goals")]
    pub include_finished: Option<bool>,
    #[schemars(description = "Milestone ID (for complete_milestone/delete_milestone)")]
    pub milestone_id: Option<i64>,
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
pub struct IndexRequest {
    #[schemars(description = "Action: project/file/status/compact/summarize/health")]
    pub action: IndexAction,
    #[schemars(description = "Project root path (defaults to active project if omitted)")]
    pub path: Option<String>,
    #[schemars(description = "Skip embedding generation (faster indexing)")]
    pub skip_embed: Option<bool>,
}

#[derive(Debug, Clone, Copy, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SessionAction {
    /// Show current session
    CurrentSession,
    /// List recent sessions
    ListSessions,
    /// Get history for a session
    GetHistory,
    /// Get session recap (preferences, recent context, goals)
    Recap,
    /// Get LLM usage summary
    UsageSummary,
    /// Get LLM usage stats grouped by dimension
    UsageStats,
    /// List recent LLM usage records
    UsageList,
    /// Query unified insights digest (pondering, proactive, doc gaps)
    Insights,
    /// List all running and recently completed tasks
    TasksList,
    /// Get status and result of a specific task
    TasksGet,
    /// Cancel a running task
    TasksCancel,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SessionRequest {
    #[schemars(
        description = "Action: current_session, list_sessions, get_history, recap, usage_summary, usage_stats, usage_list, insights, tasks_list, tasks_get, tasks_cancel"
    )]
    pub action: SessionAction,
    #[schemars(description = "Session ID (for get_history)")]
    pub session_id: Option<String>,
    #[schemars(description = "Task ID (for tasks_get/tasks_cancel)")]
    pub task_id: Option<String>,
    #[schemars(description = "Max results")]
    pub limit: Option<i64>,
    #[schemars(
        description = "Group by: role, provider, model, or provider_model (for usage_stats)"
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

// Documentation request types

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct DocumentationRequest {
    #[schemars(description = "Action: list, get, complete, skip, batch_skip, inventory, scan")]
    pub action: DocumentationAction,
    #[schemars(description = "Task ID (for get/complete/skip actions)")]
    pub task_id: Option<i64>,
    #[schemars(description = "List of task IDs (for batch_skip action)")]
    pub task_ids: Option<Vec<i64>>,
    #[schemars(description = "Reason for skipping (for skip/batch_skip actions)")]
    pub reason: Option<String>,
    #[schemars(description = "Filter by documentation type: api, architecture, guide")]
    pub doc_type: Option<String>,
    #[schemars(description = "Filter by priority: urgent, high, medium, low")]
    pub priority: Option<String>,
    #[schemars(description = "Filter by status: pending, completed, skipped")]
    pub status: Option<String>,
    #[schemars(description = "Max results to return (default: 50, for list action)")]
    pub limit: Option<i64>,
    #[schemars(description = "Number of results to skip (for list action pagination)")]
    pub offset: Option<i64>,
}

#[derive(Debug, Clone, Copy, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TeamAction {
    /// Get team status: members, files, conflicts
    Status,
    /// Review a teammate's modified files
    Review,
    /// Distill key findings/decisions from team work into team-scoped memories
    Distill,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct TeamRequest {
    #[schemars(
        description = "Action: status (team overview), review (teammate's work), distill (extract key findings)"
    )]
    pub action: TeamAction,
    #[schemars(description = "Teammate name (for review action, defaults to self)")]
    pub teammate: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RecipeAction {
    /// List available recipes
    List,
    /// Get full recipe details
    Get,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct RecipeRequest {
    #[schemars(description = "Action: list (available recipes), get (full recipe details)")]
    pub action: RecipeAction,
    #[schemars(description = "Recipe name (required for get action)")]
    pub name: Option<String>,
}
