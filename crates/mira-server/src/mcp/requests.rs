// crates/mira-server/src/mcp/requests.rs
// MCP tool request types

use rmcp::schemars;
use serde::Deserialize;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SessionStartRequest {
    #[schemars(description = "Project root path")]
    pub project_path: String,
    #[schemars(description = "Project name")]
    pub name: Option<String>,
    #[schemars(description = "Optional session ID")]
    pub session_id: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SetProjectRequest {
    #[schemars(description = "Project root path")]
    pub project_path: String,
    #[schemars(description = "Project name")]
    pub name: Option<String>,
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
    #[schemars(description = "Visibility scope: personal (only creator), project (default, anyone with project access), team (team members only)")]
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
pub struct CheckCapabilityRequest {
    #[schemars(description = "Description of capability to check")]
    pub description: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GoalRequest {
    #[schemars(description = "Action: create/bulk_create/list/get/update/delete/add_milestone/complete_milestone/delete_milestone/progress")]
    pub action: String,
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
    #[schemars(description = "For bulk_create: JSON array of goals [{title, description?, priority?}, ...]")]
    pub goals: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CrossProjectRequest {
    #[schemars(description = "Action: get_preferences/status/enable_sharing/disable_sharing/reset_budget/get_stats/extract_patterns/sync")]
    pub action: String,
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
    #[schemars(description = "Action: project/file/status")]
    pub action: String,
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
    pub action: String,
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
    #[schemars(description = "Is your response complete? Set to false if you need more information.")]
    pub complete: Option<bool>,
}

// Expert consultation request types

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ConsultArchitectRequest {
    #[schemars(description = "Code, design, or situation to analyze")]
    pub context: String,
    #[schemars(description = "Specific question to answer (optional)")]
    pub question: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ConsultPlanReviewerRequest {
    #[schemars(description = "Implementation plan to review")]
    pub context: String,
    #[schemars(description = "Specific concern to address (optional)")]
    pub question: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ConsultScopeAnalystRequest {
    #[schemars(description = "Requirements or plan to analyze for gaps")]
    pub context: String,
    #[schemars(description = "Specific area to focus on (optional)")]
    pub question: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ConsultCodeReviewerRequest {
    #[schemars(description = "Code to review")]
    pub context: String,
    #[schemars(description = "Specific aspect to focus on (optional)")]
    pub question: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ConsultSecurityRequest {
    #[schemars(description = "Code or design to analyze for security")]
    pub context: String,
    #[schemars(description = "Specific security concern (optional)")]
    pub question: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ConsultExpertsRequest {
    #[schemars(description = "List of expert roles to consult in parallel: architect, plan_reviewer, scope_analyst, code_reviewer, security")]
    pub roles: Vec<String>,
    #[schemars(description = "Code, design, or situation to analyze")]
    pub context: String,
    #[schemars(description = "Specific question for all experts (optional)")]
    pub question: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ConfigureExpertRequest {
    #[schemars(description = "Action: set/get/delete/list/providers")]
    pub action: String,
    #[schemars(description = "Expert role: architect/plan_reviewer/scope_analyst/code_reviewer/security")]
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
pub struct ListDocTasksRequest {
    #[schemars(description = "Filter by status")]
    pub status: Option<String>,
    #[schemars(description = "Filter by documentation type")]
    pub doc_type: Option<String>,
    #[schemars(description = "Filter by priority")]
    pub priority: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SkipDocTaskRequest {
    #[schemars(description = "Task ID to skip")]
    pub task_id: i64,
    #[schemars(description = "Reason for skipping")]
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct WriteDocumentationRequest {
    #[schemars(description = "Task ID from list_doc_tasks. Expert will generate and write the documentation directly.")]
    pub task_id: i64,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct TeamRequest {
    #[schemars(description = "Action: create/invite/remove/list/members")]
    pub action: String,
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

// Review findings request types

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ListFindingsRequest {
    #[schemars(description = "Filter by status: pending/accepted/rejected/fixed")]
    pub status: Option<String>,
    #[schemars(description = "Filter by file path")]
    pub file_path: Option<String>,
    #[schemars(description = "Filter by expert role: code_reviewer/security/architect/etc.")]
    pub expert_role: Option<String>,
    #[schemars(description = "Max results (default: 20)")]
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ReviewFindingRequest {
    #[schemars(description = "Finding ID to review")]
    pub finding_id: i64,
    #[schemars(description = "New status: accepted/rejected/fixed")]
    pub status: String,
    #[schemars(description = "Optional feedback explaining why (helps learning)")]
    pub feedback: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct BulkReviewFindingsRequest {
    #[schemars(description = "List of finding IDs to update")]
    pub finding_ids: Vec<i64>,
    #[schemars(description = "New status: accepted/rejected/fixed")]
    pub status: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetFindingRequest {
    #[schemars(description = "Finding ID to retrieve")]
    pub finding_id: i64,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetLearnedPatternsRequest {
    #[schemars(description = "Filter by correction type: bug/style/security/performance")]
    pub correction_type: Option<String>,
    #[schemars(description = "Max results (default: 20)")]
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct AnalyzeDiffRequest {
    #[schemars(description = "Starting git ref (commit, branch, tag). Default: HEAD~1 for commits, or analyzes staged/working changes if present")]
    pub from_ref: Option<String>,
    #[schemars(description = "Ending git ref. Default: HEAD")]
    pub to_ref: Option<String>,
    #[schemars(description = "Include impact analysis (find affected callers). Default: true")]
    pub include_impact: Option<bool>,
}
