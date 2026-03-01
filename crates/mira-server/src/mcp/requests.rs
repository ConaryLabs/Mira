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
    /// Delete a goal
    Delete,
    /// Add a milestone to a goal
    AddMilestone,
    /// Mark a milestone as complete
    CompleteMilestone,
    /// Delete a milestone
    DeleteMilestone,
    /// List sessions that worked on a goal
    Sessions,
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
    /// Analyze git diff semantically (change types, impact, risks)
    Diff,
    /// Find unreferenced symbols (dead code candidates)
    DeadCode,
    /// Package module summaries, symbols, deps, and code into a context bundle for agent spawning
    Bundle,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CodeRequest {
    #[schemars(
        description = "Action: search, symbols, callers, callees, dependencies, diff, dead_code, bundle"
    )]
    pub action: CodeAction,
    #[schemars(description = "Search query (required for search)")]
    pub query: Option<String>,
    #[schemars(description = "File path (required for symbols)")]
    pub file_path: Option<String>,
    #[schemars(description = "Function name (required for callers/callees)")]
    pub function_name: Option<String>,
    #[schemars(
        description = "Symbol type filter (e.g. function, struct, trait, class, method, enum, interface, type)"
    )]
    pub symbol_type: Option<String>,
    #[schemars(
        description = "Max results (default: 20 for search/callers/callees, 50 for symbols)"
    )]
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
    #[schemars(
        description = "Module path or concept to bundle (required for bundle). E.g. 'src/tools/core/code/', 'authentication'"
    )]
    pub scope: Option<String>,
    #[schemars(
        description = "Max character budget for bundle output (default: 6000, ~1500 tokens)"
    )]
    pub budget: Option<i64>,
    #[schemars(
        description = "Bundle detail level: overview (summaries only), standard (default, + signatures + snippets), deep (+ full chunks)"
    )]
    pub depth: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GoalRequest {
    #[schemars(
        description = "Action: create/bulk_create/list/get/update/delete/add_milestone/complete_milestone/delete_milestone/sessions"
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
        description = "For bulk_create: JSON-encoded string containing an array of goals, e.g. \"[{\\\"title\\\": \\\"Goal A\\\", \\\"priority\\\": \\\"high\\\"}, ...]\"  (title required, description and priority optional)"
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
    /// Dismiss an insight by ID (insight_source required: 'pondering' or 'doc_gap')
    DismissInsight,
    /// Show database storage status and retention policy
    StorageStatus,
    /// Run data cleanup (dry_run by default)
    Cleanup,
    /// Show learned error patterns and fixes
    ErrorPatterns,
    /// Show session history with resume chains (CLI-only)
    SessionLineage,
    /// Show capability status: what features are available vs degraded (CLI-only)
    Capabilities,
    /// Session injection efficiency report
    Report,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SessionRequest {
    #[schemars(
        description = "Action: current_session, list_sessions, get_history, recap, usage_summary, usage_stats, usage_list, insights, dismiss_insight, storage_status, cleanup, error_patterns, session_lineage, capabilities, report"
    )]
    pub action: SessionAction,
    #[schemars(description = "Session ID (for get_history)")]
    pub session_id: Option<String>,
    #[schemars(description = "Max results")]
    pub limit: Option<i64>,
    #[schemars(
        description = "Group by: role, provider, model, or provider_model (for usage_stats)"
    )]
    pub group_by: Option<String>,
    #[schemars(description = "Filter to last N days (default: 30)")]
    pub since_days: Option<u32>,
    #[schemars(
        description = "Filter insights by source: pondering/proactive/doc_gap (for insights action). Required for dismiss_insight: 'pondering' or 'doc_gap'"
    )]
    pub insight_source: Option<String>,
    #[schemars(description = "Minimum confidence threshold for insights (0.0-1.0, default: 0.5)")]
    pub min_confidence: Option<f64>,
    #[schemars(description = "Insight row ID to dismiss (for dismiss_insight action)")]
    pub insight_id: Option<i64>,
    #[schemars(
        description = "Preview what would be cleaned without deleting (default: true, for cleanup action)"
    )]
    pub dry_run: Option<bool>,
    #[schemars(
        description = "Category to clean: sessions, analytics, behavior, all (default: all, for cleanup action)"
    )]
    pub category: Option<String>,
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
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct TeamRequest {
    #[schemars(description = "Action: status (team overview), review (teammate's work)")]
    pub action: TeamAction,
    #[schemars(description = "Teammate name (for review action, defaults to self)")]
    pub teammate: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct LaunchRequest {
    #[schemars(
        description = "Agent team file name (e.g. \"expert-review-team\"). Resolves to .claude/agents/{team}.md"
    )]
    pub team: String,
    #[schemars(description = "Scope for context enrichment: file path, module path, or concept.")]
    pub scope: Option<String>,
    #[schemars(
        description = "Filter to specific members by first name, comma-separated (e.g. \"nadia,jiro\")"
    )]
    pub members: Option<String>,
    #[schemars(description = "Context budget in characters (default: 4000, min: 500, max: 20000)")]
    pub context_budget: Option<i64>,
}

// ============================================================================
// Slim MCP types — reduced schema surface exposed to Claude Code.
// Full types above are still used by CLI (`mira tool`) and handlers.
// ============================================================================

// ── Project (3 → 2 actions: removes Set) ─────────────────────────────────

#[derive(Debug, Clone, Copy, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum McpProjectAction {
    /// Initialize session with project context
    Start,
    /// Show current project
    Get,
}

impl From<McpProjectAction> for ProjectAction {
    fn from(a: McpProjectAction) -> Self {
        match a {
            McpProjectAction::Start => ProjectAction::Start,
            McpProjectAction::Get => ProjectAction::Get,
        }
    }
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct McpProjectRequest {
    #[schemars(description = "Action: start (initialize session), get (show current)")]
    pub action: McpProjectAction,
    #[schemars(description = "Project root path (required for start)")]
    pub project_path: Option<String>,
    #[schemars(description = "Project name")]
    pub name: Option<String>,
    #[schemars(description = "Optional session ID (for start action)")]
    pub session_id: Option<String>,
}

// ── Code (8 → 5 actions: removes Dependencies, Patterns, TechDebt) ──────

#[derive(Debug, Clone, Copy, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum McpCodeAction {
    /// Search code by meaning
    Search,
    /// Get symbols from a file
    Symbols,
    /// Find all functions that call a given function
    Callers,
    /// Find all functions called by a given function
    Callees,
    /// Package module summaries, symbols, deps, and code into a context bundle for agent spawning
    Bundle,
}

impl From<McpCodeAction> for CodeAction {
    fn from(a: McpCodeAction) -> Self {
        match a {
            McpCodeAction::Search => CodeAction::Search,
            McpCodeAction::Symbols => CodeAction::Symbols,
            McpCodeAction::Callers => CodeAction::Callers,
            McpCodeAction::Callees => CodeAction::Callees,
            McpCodeAction::Bundle => CodeAction::Bundle,
        }
    }
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct McpCodeRequest {
    #[schemars(description = "Action: search, symbols, callers, callees, bundle")]
    pub action: McpCodeAction,
    #[schemars(description = "Search query (required for search)")]
    pub query: Option<String>,
    #[schemars(description = "File path (required for symbols)")]
    pub file_path: Option<String>,
    #[schemars(description = "Function name (required for callers/callees)")]
    pub function_name: Option<String>,
    #[schemars(
        description = "Symbol type filter (e.g. function, struct, trait, class, method, enum, interface, type)"
    )]
    pub symbol_type: Option<String>,
    #[schemars(
        description = "Max results (default: 20 for search/callers/callees, 50 for symbols)"
    )]
    pub limit: Option<i64>,
    #[schemars(
        description = "Module path or concept to bundle (required for bundle). E.g. 'src/tools/core/code/', 'authentication'"
    )]
    pub scope: Option<String>,
    #[schemars(
        description = "Max character budget for bundle output (default: 6000, ~1500 tokens)"
    )]
    pub budget: Option<i64>,
    #[schemars(
        description = "Bundle detail level: overview (summaries only), standard (default, + signatures + snippets), deep (+ full chunks)"
    )]
    pub depth: Option<String>,
}

impl From<McpCodeRequest> for CodeRequest {
    fn from(r: McpCodeRequest) -> Self {
        Self {
            action: r.action.into(),
            query: r.query,
            file_path: r.file_path,
            function_name: r.function_name,
            symbol_type: r.symbol_type,
            limit: r.limit,
            from_ref: None,
            to_ref: None,
            include_impact: None,
            scope: r.scope,
            budget: r.budget,
            depth: r.depth,
        }
    }
}

// ── Diff (standalone tool, extracted from code) ───────────────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct McpDiffRequest {
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

// ── Index (6 → 3 actions: removes Compact, Summarize, Health) ────────────

#[derive(Debug, Clone, Copy, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum McpIndexAction {
    /// Index entire project
    Project,
    /// Index a single file
    File,
    /// Show index status
    Status,
}

impl From<McpIndexAction> for IndexAction {
    fn from(a: McpIndexAction) -> Self {
        match a {
            McpIndexAction::Project => IndexAction::Project,
            McpIndexAction::File => IndexAction::File,
            McpIndexAction::Status => IndexAction::Status,
        }
    }
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct McpIndexRequest {
    #[schemars(
        description = "Action: project (full reindex), file (single file), status (index stats)"
    )]
    pub action: McpIndexAction,
    #[schemars(description = "Project root path (defaults to active project if omitted)")]
    pub path: Option<String>,
    #[schemars(description = "Skip embedding generation (faster indexing)")]
    pub skip_embed: Option<bool>,
}

// ── Session (14 → 4 actions: keeps Recap, Insights, DismissInsight, CurrentSession) ──

#[derive(Debug, Clone, Copy, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum McpSessionAction {
    /// Get session recap (preferences, recent context, goals)
    Recap,
    /// Show current session
    CurrentSession,
}

impl From<McpSessionAction> for SessionAction {
    fn from(a: McpSessionAction) -> Self {
        match a {
            McpSessionAction::Recap => SessionAction::Recap,
            McpSessionAction::CurrentSession => SessionAction::CurrentSession,
        }
    }
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct McpSessionRequest {
    #[schemars(
        description = "Action: recap (preferences + context + goals), current_session (show current)"
    )]
    pub action: McpSessionAction,
    #[schemars(description = "Max results")]
    pub limit: Option<i64>,
}

// Fields intentionally set to None belong to actions removed from MCP
// (list_sessions, get_history, usage_*, storage_status, cleanup, insights).
// If adding a new MCP action that needs these fields, add them to McpSessionRequest too.
impl From<McpSessionRequest> for SessionRequest {
    fn from(r: McpSessionRequest) -> Self {
        Self {
            action: r.action.into(),
            session_id: None,
            limit: r.limit,
            group_by: None,
            since_days: None,
            insight_source: None,
            min_confidence: None,
            insight_id: None,
            dry_run: None,
            category: None,
        }
    }
}

// ── Insights (extracted from session) ──

#[derive(Debug, Clone, Copy, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum McpInsightsAction {
    /// Query unified insights digest (pondering, proactive, doc gaps)
    Insights,
    /// Dismiss an insight by ID (insight_source required: 'pondering' or 'doc_gap')
    DismissInsight,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct McpInsightsRequest {
    #[schemars(
        description = "Action: insights (background analysis digest), dismiss_insight (remove resolved insight; insight_source required: 'pondering' or 'doc_gap')"
    )]
    pub action: McpInsightsAction,
    #[schemars(
        description = "Filter insights by source: pondering/proactive/doc_gap (for insights action). Required for dismiss_insight: 'pondering' or 'doc_gap'"
    )]
    pub insight_source: Option<String>,
    #[schemars(description = "Minimum confidence threshold for insights (0.0-1.0, default: 0.5)")]
    pub min_confidence: Option<f64>,
    #[schemars(description = "Insight row ID to dismiss (for dismiss_insight action)")]
    pub insight_id: Option<i64>,
    #[schemars(description = "Max results")]
    pub limit: Option<i64>,
    #[schemars(description = "Filter to last N days (default: 30)")]
    pub since_days: Option<u32>,
}

impl From<McpInsightsRequest> for SessionRequest {
    fn from(r: McpInsightsRequest) -> Self {
        Self {
            action: match r.action {
                McpInsightsAction::Insights => SessionAction::Insights,
                McpInsightsAction::DismissInsight => SessionAction::DismissInsight,
            },
            session_id: None,
            limit: r.limit,
            group_by: None,
            since_days: r.since_days,
            insight_source: r.insight_source,
            min_confidence: r.min_confidence,
            insight_id: r.insight_id,
            dry_run: None,
            category: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── McpSessionAction deserialization ──────────────────────────────

    #[test]
    fn session_action_current_session() {
        let a: McpSessionAction = serde_json::from_value(json!("current_session")).unwrap();
        assert!(matches!(a, McpSessionAction::CurrentSession));
    }

    #[test]
    fn session_action_recap() {
        let a: McpSessionAction = serde_json::from_value(json!("recap")).unwrap();
        assert!(matches!(a, McpSessionAction::Recap));
    }

    #[test]
    fn session_action_rejects_insights() {
        let result = serde_json::from_value::<McpSessionAction>(json!("insights"));
        assert!(
            result.is_err(),
            "McpSessionAction should reject 'insights' (now standalone tool)"
        );
    }

    // ── McpInsightsAction deserialization ─────────────────────────────

    #[test]
    fn insights_action_insights() {
        let a: McpInsightsAction = serde_json::from_value(json!("insights")).unwrap();
        assert!(matches!(a, McpInsightsAction::Insights));
    }

    #[test]
    fn insights_action_dismiss_insight() {
        let a: McpInsightsAction = serde_json::from_value(json!("dismiss_insight")).unwrap();
        assert!(matches!(a, McpInsightsAction::DismissInsight));
    }

    // ── McpProjectAction deserialization ──────────────────────────────

    #[test]
    fn project_action_start() {
        let a: McpProjectAction = serde_json::from_value(json!("start")).unwrap();
        assert!(matches!(a, McpProjectAction::Start));
    }

    #[test]
    fn project_action_get() {
        let a: McpProjectAction = serde_json::from_value(json!("get")).unwrap();
        assert!(matches!(a, McpProjectAction::Get));
    }

    // ── McpCodeAction deserialization ─────────────────────────────────

    #[test]
    fn code_action_search() {
        let a: McpCodeAction = serde_json::from_value(json!("search")).unwrap();
        assert!(matches!(a, McpCodeAction::Search));
    }

    #[test]
    fn code_action_bundle() {
        let a: McpCodeAction = serde_json::from_value(json!("bundle")).unwrap();
        assert!(matches!(a, McpCodeAction::Bundle));
    }

    #[test]
    fn code_action_rejects_diff() {
        let result = serde_json::from_value::<McpCodeAction>(json!("diff"));
        assert!(
            result.is_err(),
            "McpCodeAction should reject 'diff' (now a standalone tool)"
        );
    }

    // ── McpIndexAction deserialization ────────────────────────────────

    #[test]
    fn index_action_project() {
        let a: McpIndexAction = serde_json::from_value(json!("project")).unwrap();
        assert!(matches!(a, McpIndexAction::Project));
    }

    #[test]
    fn index_action_status() {
        let a: McpIndexAction = serde_json::from_value(json!("status")).unwrap();
        assert!(matches!(a, McpIndexAction::Status));
    }

    // ── Removed actions are rejected ─────────────────────────────────

    #[test]
    fn project_action_rejects_set() {
        let result = serde_json::from_value::<McpProjectAction>(json!("set"));
        assert!(result.is_err(), "McpProjectAction should reject 'set'");
    }

    #[test]
    fn code_action_rejects_dependencies() {
        let result = serde_json::from_value::<McpCodeAction>(json!("dependencies"));
        assert!(
            result.is_err(),
            "McpCodeAction should reject 'dependencies'"
        );
    }

    #[test]
    fn code_action_rejects_patterns() {
        let result = serde_json::from_value::<McpCodeAction>(json!("patterns"));
        assert!(result.is_err(), "McpCodeAction should reject 'patterns'");
    }

    #[test]
    fn code_action_rejects_tech_debt() {
        let result = serde_json::from_value::<McpCodeAction>(json!("tech_debt"));
        assert!(result.is_err(), "McpCodeAction should reject 'tech_debt'");
    }

    #[test]
    fn code_action_rejects_dead_code() {
        let result = serde_json::from_value::<McpCodeAction>(json!("dead_code"));
        assert!(result.is_err(), "McpCodeAction should reject 'dead_code'");
    }

    #[test]
    fn code_action_rejects_conventions() {
        let result = serde_json::from_value::<McpCodeAction>(json!("conventions"));
        assert!(result.is_err(), "McpCodeAction should reject 'conventions'");
    }

    #[test]
    fn code_action_rejects_debt_delta() {
        let result = serde_json::from_value::<McpCodeAction>(json!("debt_delta"));
        assert!(result.is_err(), "McpCodeAction should reject 'debt_delta'");
    }

    #[test]
    fn index_action_rejects_compact() {
        let result = serde_json::from_value::<McpIndexAction>(json!("compact"));
        assert!(result.is_err(), "McpIndexAction should reject 'compact'");
    }

    #[test]
    fn index_action_rejects_summarize() {
        let result = serde_json::from_value::<McpIndexAction>(json!("summarize"));
        assert!(result.is_err(), "McpIndexAction should reject 'summarize'");
    }

    #[test]
    fn index_action_rejects_health() {
        let result = serde_json::from_value::<McpIndexAction>(json!("health"));
        assert!(result.is_err(), "McpIndexAction should reject 'health'");
    }

    #[test]
    fn session_action_rejects_list_sessions() {
        let result = serde_json::from_value::<McpSessionAction>(json!("list_sessions"));
        assert!(
            result.is_err(),
            "McpSessionAction should reject 'list_sessions'"
        );
    }

    #[test]
    fn session_action_rejects_get_history() {
        let result = serde_json::from_value::<McpSessionAction>(json!("get_history"));
        assert!(
            result.is_err(),
            "McpSessionAction should reject 'get_history'"
        );
    }

    #[test]
    fn session_action_rejects_tasks_list() {
        let result = serde_json::from_value::<McpSessionAction>(json!("tasks_list"));
        assert!(
            result.is_err(),
            "McpSessionAction should reject 'tasks_list'"
        );
    }

    #[test]
    fn session_action_rejects_usage_summary() {
        let result = serde_json::from_value::<McpSessionAction>(json!("usage_summary"));
        assert!(
            result.is_err(),
            "McpSessionAction should reject 'usage_summary'"
        );
    }

    #[test]
    fn session_action_rejects_storage_status() {
        let result = serde_json::from_value::<McpSessionAction>(json!("storage_status"));
        assert!(
            result.is_err(),
            "McpSessionAction should reject 'storage_status'"
        );
    }

    #[test]
    fn session_action_rejects_cleanup() {
        let result = serde_json::from_value::<McpSessionAction>(json!("cleanup"));
        assert!(result.is_err(), "McpSessionAction should reject 'cleanup'");
    }

    #[test]
    fn session_action_rejects_error_patterns() {
        let result = serde_json::from_value::<McpSessionAction>(json!("error_patterns"));
        assert!(
            result.is_err(),
            "McpSessionAction should reject 'error_patterns'"
        );
    }

    #[test]
    fn session_action_rejects_health_trends() {
        let result = serde_json::from_value::<McpSessionAction>(json!("health_trends"));
        assert!(
            result.is_err(),
            "McpSessionAction should reject 'health_trends'"
        );
    }

    #[test]
    fn session_action_rejects_session_lineage() {
        let result = serde_json::from_value::<McpSessionAction>(json!("session_lineage"));
        assert!(
            result.is_err(),
            "McpSessionAction should reject 'session_lineage'"
        );
    }

    #[test]
    fn session_action_rejects_capabilities() {
        let result = serde_json::from_value::<McpSessionAction>(json!("capabilities"));
        assert!(
            result.is_err(),
            "McpSessionAction should reject 'capabilities'"
        );
    }

    // ── From<McpSessionRequest> for SessionRequest ───────────────────

    #[test]
    fn session_request_conversion() {
        let mcp = McpSessionRequest {
            action: McpSessionAction::Recap,
            limit: Some(10),
        };

        let full: SessionRequest = mcp.into();

        // Fields that pass through
        assert!(matches!(full.action, SessionAction::Recap));
        assert_eq!(full.limit, Some(10));

        // Fields intentionally None (belong to removed MCP actions)
        assert!(full.session_id.is_none());
        assert!(full.group_by.is_none());
        assert!(full.since_days.is_none());
        assert!(full.insight_source.is_none());
        assert!(full.min_confidence.is_none());
        assert!(full.insight_id.is_none());
        assert!(full.dry_run.is_none());
        assert!(full.category.is_none());
    }

    // ── From<McpInsightsRequest> for SessionRequest ───────────────────

    #[test]
    fn insights_request_conversion() {
        let mcp = McpInsightsRequest {
            action: McpInsightsAction::Insights,
            insight_source: Some("pondering".into()),
            min_confidence: Some(0.7),
            insight_id: Some(42),
            limit: Some(10),
            since_days: Some(7),
        };

        let full: SessionRequest = mcp.into();

        assert!(matches!(full.action, SessionAction::Insights));
        assert_eq!(full.insight_source.as_deref(), Some("pondering"));
        assert_eq!(full.min_confidence, Some(0.7));
        assert_eq!(full.insight_id, Some(42));
        assert_eq!(full.limit, Some(10));
        assert_eq!(full.since_days, Some(7));

        // Fields intentionally None
        assert!(full.session_id.is_none());
        assert!(full.group_by.is_none());
        assert!(full.dry_run.is_none());
        assert!(full.category.is_none());
    }

    // ── From<McpCodeRequest> for CodeRequest ─────────────────────────

    #[test]
    fn code_request_conversion() {
        let mcp = McpCodeRequest {
            action: McpCodeAction::Search,
            query: Some("authentication".into()),
            file_path: Some("src/auth.rs".into()),
            function_name: Some("login".into()),
            symbol_type: Some("function".into()),
            limit: Some(50),
            scope: None,
            budget: None,
            depth: None,
        };

        let full: CodeRequest = mcp.into();

        assert!(matches!(full.action, CodeAction::Search));
        assert_eq!(full.query.as_deref(), Some("authentication"));
        assert_eq!(full.file_path.as_deref(), Some("src/auth.rs"));
        assert_eq!(full.function_name.as_deref(), Some("login"));
        assert_eq!(full.symbol_type.as_deref(), Some("function"));
        assert_eq!(full.limit, Some(50));
        // Diff fields are None — they belong to the standalone diff tool now
        assert!(full.from_ref.is_none());
        assert!(full.to_ref.is_none());
        assert!(full.include_impact.is_none());
        // Bundle fields are None when not using bundle action
        assert!(full.scope.is_none());
        assert!(full.budget.is_none());
        assert!(full.depth.is_none());
    }

    // ── McpDiffRequest deserialization ──────────────────────────────────

    #[test]
    fn diff_request_deserialization() {
        let json = json!({
            "from_ref": "main",
            "to_ref": "HEAD",
            "include_impact": true
        });
        let req: McpDiffRequest = serde_json::from_value(json).unwrap();
        assert_eq!(req.from_ref.as_deref(), Some("main"));
        assert_eq!(req.to_ref.as_deref(), Some("HEAD"));
        assert_eq!(req.include_impact, Some(true));
    }

    #[test]
    fn diff_request_all_optional() {
        let json = json!({});
        let req: McpDiffRequest = serde_json::from_value(json).unwrap();
        assert!(req.from_ref.is_none());
        assert!(req.to_ref.is_none());
        assert!(req.include_impact.is_none());
    }
}
