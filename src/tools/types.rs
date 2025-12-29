// src/tools/types.rs
// Consolidated request types for MCP tools - optimized for minimal token footprint

use schemars::JsonSchema;
use serde::Deserialize;

// ============================================================================
// Memory Tools (keep separate - high usage, simple)
// ============================================================================

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RememberRequest {
    #[schemars(description = "Content to remember")]
    pub content: String,
    #[schemars(description = "Type: preference/decision/context/general")]
    pub fact_type: Option<String>,
    #[schemars(description = "Category")]
    pub category: Option<String>,
    #[schemars(description = "Key for upsert")]
    pub key: Option<String>,
    #[schemars(description = "Confidence/truthiness (0.0-1.0, default 1.0). Use 0.8 for compaction summaries.")]
    pub confidence: Option<f64>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct RecallRequest {
    #[schemars(description = "Search query")]
    pub query: String,
    #[schemars(description = "Filter by type")]
    pub fact_type: Option<String>,
    #[schemars(description = "Filter by category")]
    pub category: Option<String>,
    #[schemars(description = "Max results")]
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ForgetRequest {
    #[schemars(description = "Memory ID to delete")]
    pub id: String,
}

// ============================================================================
// Consolidated Task Tool (6→1)
// ============================================================================

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TaskRequest {
    #[schemars(description = "Action: create/list/get/update/complete/delete")]
    pub action: String,
    #[schemars(description = "Task ID")]
    pub task_id: Option<String>,
    #[schemars(description = "Title")]
    pub title: Option<String>,
    #[schemars(description = "Description")]
    pub description: Option<String>,
    #[schemars(description = "Priority: low/medium/high/urgent")]
    pub priority: Option<String>,
    #[schemars(description = "Status: pending/in_progress/completed/blocked")]
    pub status: Option<String>,
    #[schemars(description = "Parent task ID")]
    pub parent_id: Option<String>,
    #[schemars(description = "Completion notes")]
    pub notes: Option<String>,
    #[schemars(description = "Include completed")]
    pub include_completed: Option<bool>,
    #[schemars(description = "Max results")]
    pub limit: Option<i64>,
}

// ============================================================================
// Consolidated Goal Tool (6→1)
// ============================================================================

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GoalRequest {
    #[schemars(description = "Action: create/list/get/update/add_milestone/complete_milestone/progress")]
    pub action: String,
    #[schemars(description = "Goal ID")]
    pub goal_id: Option<String>,
    #[schemars(description = "Milestone ID")]
    pub milestone_id: Option<String>,
    #[schemars(description = "Title")]
    pub title: Option<String>,
    #[schemars(description = "Description")]
    pub description: Option<String>,
    #[schemars(description = "Success criteria")]
    pub success_criteria: Option<String>,
    #[schemars(description = "Priority: low/medium/high/critical")]
    pub priority: Option<String>,
    #[schemars(description = "Status: planning/in_progress/blocked/completed/abandoned")]
    pub status: Option<String>,
    #[schemars(description = "Progress percent (0-100)")]
    pub progress_percent: Option<i32>,
    #[schemars(description = "Milestone weight")]
    pub weight: Option<i32>,
    #[schemars(description = "Include finished goals")]
    pub include_finished: Option<bool>,
    #[schemars(description = "Max results")]
    pub limit: Option<i64>,
}

// ============================================================================
// Consolidated Correction Tool (4→1)
// ============================================================================

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CorrectionRequest {
    #[schemars(description = "Action: record/get/validate/list")]
    pub action: String,
    #[schemars(description = "Correction ID")]
    pub correction_id: Option<String>,
    #[schemars(description = "Type: style/approach/pattern/preference/anti_pattern")]
    pub correction_type: Option<String>,
    #[schemars(description = "What was wrong")]
    pub what_was_wrong: Option<String>,
    #[schemars(description = "What is right")]
    pub what_is_right: Option<String>,
    #[schemars(description = "Rationale")]
    pub rationale: Option<String>,
    #[schemars(description = "Scope: global/project/file/topic")]
    pub scope: Option<String>,
    #[schemars(description = "Keywords")]
    pub keywords: Option<String>,
    #[schemars(description = "File path")]
    pub file_path: Option<String>,
    #[schemars(description = "Topic")]
    pub topic: Option<String>,
    #[schemars(description = "Outcome: validated/overridden/deprecated")]
    pub outcome: Option<String>,
    #[schemars(description = "Max results")]
    pub limit: Option<i64>,
}

// ============================================================================
// Consolidated Document Tool (3→1)
// ============================================================================

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DocumentRequest {
    #[schemars(description = "Action: list/search/get/ingest/delete")]
    pub action: String,
    #[schemars(description = "Document ID")]
    pub document_id: Option<String>,
    #[schemars(description = "File path")]
    pub path: Option<String>,
    #[schemars(description = "Document name")]
    pub name: Option<String>,
    #[schemars(description = "Search query")]
    pub query: Option<String>,
    #[schemars(description = "Filter by type: pdf/markdown/text/code")]
    pub doc_type: Option<String>,
    #[schemars(description = "Include full content")]
    pub include_content: Option<bool>,
    #[schemars(description = "Max results")]
    pub limit: Option<i64>,
}

// ============================================================================
// File Search Tool - Gemini RAG with per-project stores
// ============================================================================

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FileSearchRequest {
    #[schemars(description = "Action: index/list/remove/status")]
    pub action: String,
    #[schemars(description = "File path to index (for 'index' action)")]
    pub path: Option<String>,
    #[schemars(description = "Display name for the file")]
    pub display_name: Option<String>,
    #[schemars(description = "Custom metadata key-value pairs as JSON")]
    pub metadata: Option<String>,
    #[schemars(description = "Wait for indexing to complete (default: false)")]
    pub wait: Option<bool>,
}

// ============================================================================
// Consolidated Permission Tool (3→1)
// ============================================================================

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PermissionRequest {
    #[schemars(description = "Action: save/list/delete")]
    pub action: String,
    #[schemars(description = "Rule ID")]
    pub rule_id: Option<String>,
    #[schemars(description = "Tool name")]
    pub tool_name: Option<String>,
    #[schemars(description = "Field to match")]
    pub input_field: Option<String>,
    #[schemars(description = "Pattern")]
    pub input_pattern: Option<String>,
    #[schemars(description = "Match type: exact/prefix/glob")]
    pub match_type: Option<String>,
    #[schemars(description = "Scope: global/project")]
    pub scope: Option<String>,
    #[schemars(description = "Description")]
    pub description: Option<String>,
    #[schemars(description = "Max results")]
    pub limit: Option<i64>,
}

// ============================================================================
// Consolidated Build Tool (4→1)
// ============================================================================

#[derive(Debug, Deserialize, JsonSchema)]
pub struct BuildRequest {
    #[schemars(description = "Action: record/record_error/get_errors/resolve")]
    pub action: String,
    #[schemars(description = "Error ID")]
    pub error_id: Option<i64>,
    #[schemars(description = "Build command")]
    pub command: Option<String>,
    #[schemars(description = "Build succeeded")]
    pub success: Option<bool>,
    #[schemars(description = "Duration in ms")]
    pub duration_ms: Option<i64>,
    #[schemars(description = "Error message")]
    pub message: Option<String>,
    #[schemars(description = "Category")]
    pub category: Option<String>,
    #[schemars(description = "Severity: error/warning")]
    pub severity: Option<String>,
    #[schemars(description = "File path")]
    pub file_path: Option<String>,
    #[schemars(description = "Line number")]
    pub line_number: Option<i32>,
    #[schemars(description = "Error code")]
    pub code: Option<String>,
    #[schemars(description = "Include resolved")]
    pub include_resolved: Option<bool>,
    #[schemars(description = "Max results")]
    pub limit: Option<i64>,
}

// ============================================================================
// Guidelines Tools (keep separate - different structure)
// ============================================================================

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetGuidelinesRequest {
    #[schemars(description = "Category: mira_usage/naming/style/architecture/testing")]
    pub category: Option<String>,
    #[schemars(description = "Project path")]
    pub project_path: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AddGuidelineRequest {
    #[schemars(description = "Guideline content")]
    pub content: String,
    #[schemars(description = "Category")]
    pub category: String,
    #[schemars(description = "Project path")]
    pub project_path: Option<String>,
    #[schemars(description = "Priority")]
    pub priority: Option<i32>,
}

// ============================================================================
// Code Intelligence Tools (keep separate - distinct use cases)
// ============================================================================

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct GetSymbolsRequest {
    #[schemars(description = "File path")]
    pub file_path: String,
    #[schemars(description = "Symbol type")]
    pub symbol_type: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetCallGraphRequest {
    #[schemars(description = "Symbol name")]
    pub symbol: String,
    #[schemars(description = "Depth")]
    pub depth: Option<i32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetRelatedFilesRequest {
    #[schemars(description = "File path")]
    pub file_path: String,
    #[schemars(description = "Relation type")]
    pub relation_type: Option<String>,
    #[schemars(description = "Max results")]
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SemanticCodeSearchRequest {
    #[schemars(description = "Query")]
    pub query: String,
    #[schemars(description = "Language")]
    pub language: Option<String>,
    #[schemars(description = "Max results")]
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetCodebaseStyleRequest {
    #[schemars(description = "Project path")]
    pub project_path: Option<String>,
}

// ============================================================================
// Git Intelligence Tools
// ============================================================================

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetRecentCommitsRequest {
    #[schemars(description = "Max commits")]
    pub limit: Option<i64>,
    #[schemars(description = "File path")]
    pub file_path: Option<String>,
    #[schemars(description = "Author")]
    pub author: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchCommitsRequest {
    #[schemars(description = "Search query")]
    pub query: String,
    #[schemars(description = "Max results")]
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct FindCochangeRequest {
    #[schemars(description = "File path")]
    pub file_path: String,
    #[schemars(description = "Max results")]
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct FindSimilarFixesRequest {
    #[schemars(description = "Error message")]
    pub error: String,
    #[schemars(description = "Category")]
    pub category: Option<String>,
    #[schemars(description = "Language")]
    pub language: Option<String>,
    #[schemars(description = "Max results")]
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RecordErrorFixRequest {
    #[schemars(description = "Error pattern")]
    pub error_pattern: String,
    #[schemars(description = "Fix description")]
    pub fix_description: String,
    #[schemars(description = "Category")]
    pub category: Option<String>,
    #[schemars(description = "Language")]
    pub language: Option<String>,
    #[schemars(description = "File pattern")]
    pub file_pattern: Option<String>,
    #[schemars(description = "Diff")]
    pub fix_diff: Option<String>,
    #[schemars(description = "Commit")]
    pub fix_commit: Option<String>,
}

// ============================================================================
// Session Tools
// ============================================================================

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetSessionContextRequest {
    #[schemars(description = "Include memories")]
    pub include_memories: Option<bool>,
    #[schemars(description = "Include tasks")]
    pub include_tasks: Option<bool>,
    #[schemars(description = "Include sessions")]
    pub include_sessions: Option<bool>,
    #[schemars(description = "Include goals")]
    pub include_goals: Option<bool>,
    #[schemars(description = "Include corrections")]
    pub include_corrections: Option<bool>,
    #[schemars(description = "Max items")]
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct StoreSessionRequest {
    #[schemars(description = "Session summary")]
    pub summary: String,
    #[schemars(description = "Session ID")]
    pub session_id: Option<String>,
    #[schemars(description = "Project path")]
    pub project_path: Option<String>,
    #[schemars(description = "Topics")]
    pub topics: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchSessionsRequest {
    #[schemars(description = "Search query")]
    pub query: String,
    #[schemars(description = "Max results")]
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchMcpHistoryRequest {
    #[schemars(description = "Search query (searches result summaries and arguments)")]
    pub query: Option<String>,
    #[schemars(description = "Filter by tool name (e.g., 'remember', 'recall')")]
    pub tool_name: Option<String>,
    #[schemars(description = "Max results")]
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct StoreDecisionRequest {
    #[schemars(description = "Unique key")]
    pub key: String,
    #[schemars(description = "Decision content")]
    pub decision: String,
    #[schemars(description = "Category")]
    pub category: Option<String>,
    #[schemars(description = "Context/rationale")]
    pub context: Option<String>,
}

// ============================================================================
// Project Tools
// ============================================================================

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SetProjectRequest {
    #[schemars(description = "Project root path")]
    pub project_path: String,
    #[schemars(description = "Project name")]
    pub name: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetProjectRequest {}

// ============================================================================
// Analytics/Admin Tools
// ============================================================================

#[derive(Debug, Deserialize, JsonSchema)]
pub struct QueryRequest {
    #[schemars(description = "SQL SELECT query")]
    pub sql: String,
    #[schemars(description = "Max rows")]
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DebounceRequest {
    #[schemars(description = "Unique key for debouncing (e.g., 'pretool:/path/to/file.rs')")]
    pub key: String,
    #[schemars(description = "Time-to-live in seconds before the key can trigger again")]
    pub ttl_secs: u64,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TrackActivityRequest {
    #[schemars(description = "Tool name that was called (e.g., 'Edit', 'Bash', 'Read')")]
    pub tool_name: String,
    #[schemars(description = "Whether the tool call succeeded")]
    #[serde(default = "default_true")]
    pub success: bool,
    #[schemars(description = "Optional file path if a file was touched")]
    pub file_path: Option<String>,
}

fn default_true() -> bool { true }

/// Heartbeat request for session liveness tracking
#[derive(Debug, Deserialize, JsonSchema)]
pub struct HeartbeatRequest {
    #[schemars(description = "Session ID to heartbeat")]
    pub session_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ExtractRequest {
    #[schemars(description = "Transcript or text content to extract decisions/topics from")]
    pub transcript: String,
}

// ============================================================================
// Proactive Context
// ============================================================================

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetProactiveContextRequest {
    #[schemars(description = "Files")]
    pub files: Option<Vec<String>>,
    #[schemars(description = "Topics")]
    pub topics: Option<Vec<String>>,
    #[schemars(description = "Error")]
    pub error: Option<String>,
    #[schemars(description = "Task")]
    pub task: Option<String>,
    #[schemars(description = "Max per category")]
    pub limit_per_category: Option<i32>,
    #[schemars(description = "Session phase: early/middle/late/wrapping")]
    pub session_phase: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RecordRejectedApproachRequest {
    #[schemars(description = "Problem context")]
    pub problem_context: String,
    #[schemars(description = "Approach tried")]
    pub approach: String,
    #[schemars(description = "Why rejected")]
    pub rejection_reason: String,
    #[schemars(description = "Related files")]
    pub related_files: Option<String>,
    #[schemars(description = "Related topics")]
    pub related_topics: Option<String>,
}

// ============================================================================
// Session Start (combined startup tool)
// ============================================================================

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SessionStartRequest {
    #[schemars(description = "Project root path")]
    pub project_path: String,
    #[schemars(description = "Project name")]
    pub name: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SyncWorkStateRequest {
    #[schemars(description = "Context type: active_todos/current_file/active_goal")]
    pub context_type: String,
    #[schemars(description = "Context key")]
    pub context_key: String,
    #[schemars(description = "Context value (JSON)")]
    pub context_value: serde_json::Value,
    #[schemars(description = "TTL in hours")]
    pub ttl_hours: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetWorkStateRequest {
    #[schemars(description = "Context type")]
    pub context_type: Option<String>,
    #[schemars(description = "Context key")]
    pub context_key: Option<String>,
    #[schemars(description = "Include expired")]
    pub include_expired: Option<bool>,
}

// ============================================================================
// Proposal Tools (Proactive Organization System)
// ============================================================================

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ProposalRequest {
    #[schemars(description = "Action: extract/list/confirm/reject/review")]
    pub action: String,
    #[schemars(description = "Proposal ID (for confirm/reject)")]
    pub proposal_id: Option<String>,
    #[schemars(description = "Text to extract proposals from (for extract)")]
    pub text: Option<String>,
    #[schemars(description = "Base confidence for extraction (0.0-1.0, default 0.5)")]
    pub base_confidence: Option<f64>,
    #[schemars(description = "Filter by type: goal/task/decision/summary")]
    pub proposal_type: Option<String>,
    #[schemars(description = "Filter by status: pending/confirmed/rejected/auto_committed")]
    pub status: Option<String>,
    #[schemars(description = "Max results")]
    pub limit: Option<i64>,
}

// ============================================================================
// Carousel Tool - Control context rotation
// ============================================================================

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CarouselRequest {
    #[schemars(description = "Action: status/pin/unpin/advance/focus/panic/exit_panic/anchor/log")]
    pub action: String,
    #[schemars(description = "Category: goals/decisions/memories/git/code/system/errors/patterns")]
    pub category: Option<String>,
    #[schemars(description = "Duration in minutes (for pin/focus, default: 30)")]
    pub duration_minutes: Option<i64>,
    #[schemars(description = "Content to anchor (for anchor action)")]
    pub content: Option<String>,
    #[schemars(description = "Reason for anchoring or panic")]
    pub reason: Option<String>,
}

// ============================================================================
// Indexing Tools
// ============================================================================

#[derive(Debug, Deserialize, JsonSchema)]
pub struct IndexRequest {
    #[schemars(description = "Action: project/file/status")]
    pub action: String,
    #[schemars(description = "Path")]
    pub path: Option<String>,
    #[schemars(description = "Include git")]
    pub include_git: Option<bool>,
    #[schemars(description = "Commit limit")]
    pub commit_limit: Option<i64>,
    #[schemars(description = "Parallel")]
    pub parallel: Option<bool>,
    #[schemars(description = "Max workers")]
    pub max_workers: Option<i64>,
}

// ============================================================================
// Instruction Queue Tools (Studio -> Claude Code communication)
// ============================================================================

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetPendingInstructionsRequest {
    #[schemars(description = "Maximum number of instructions to return")]
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct MarkInstructionRequest {
    #[schemars(description = "Instruction ID")]
    pub instruction_id: String,
    #[schemars(description = "New status: in_progress/completed/failed")]
    pub status: String,
    #[schemars(description = "Result or error message")]
    pub result: Option<String>,
}


