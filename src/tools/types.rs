// src/tools/types.rs
// Request types for MCP tools - simplified for Claude Code augmentation

use schemars::JsonSchema;
use serde::Deserialize;

// ============================================================================
// Memory Tools
// ============================================================================

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RememberRequest {
    #[schemars(description = "The fact, decision, or preference to remember")]
    pub content: String,
    #[schemars(description = "Type: 'preference', 'decision', 'context', 'general'")]
    pub fact_type: Option<String>,
    #[schemars(description = "Category for organization")]
    pub category: Option<String>,
    #[schemars(description = "Unique key for upsert (auto-generated if not provided)")]
    pub key: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct RecallRequest {
    #[schemars(description = "Search query to find relevant memories")]
    pub query: String,
    #[schemars(description = "Filter by fact type")]
    pub fact_type: Option<String>,
    #[schemars(description = "Filter by category")]
    pub category: Option<String>,
    #[schemars(description = "Maximum results (default: 10)")]
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ForgetRequest {
    #[schemars(description = "ID of the memory to forget")]
    pub id: String,
}

// ============================================================================
// Guidelines Tools
// ============================================================================

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetGuidelinesRequest {
    #[schemars(description = "Filter by project path")]
    pub project_path: Option<String>,
    #[schemars(description = "Filter by category: 'mira_usage' for Mira tool guidance, or 'naming', 'style', 'architecture', 'testing' for project conventions")]
    pub category: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AddGuidelineRequest {
    #[schemars(description = "The guideline content")]
    pub content: String,
    #[schemars(description = "Category: 'naming', 'style', 'architecture', 'testing', 'other'")]
    pub category: String,
    #[schemars(description = "Project path (optional - global if not specified)")]
    pub project_path: Option<String>,
    #[schemars(description = "Priority (higher = more important)")]
    pub priority: Option<i32>,
}

// ============================================================================
// Task Tools
// ============================================================================

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreateTaskRequest {
    #[schemars(description = "Task title")]
    pub title: String,
    #[schemars(description = "Detailed description")]
    pub description: Option<String>,
    #[schemars(description = "Priority: 'low', 'medium', 'high', 'urgent'")]
    pub priority: Option<String>,
    #[schemars(description = "Project path")]
    pub project_path: Option<String>,
    #[schemars(description = "Tags (JSON array or comma-separated)")]
    pub tags: Option<String>,
    #[schemars(description = "Parent task ID for subtasks")]
    pub parent_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListTasksRequest {
    #[schemars(description = "Filter by status: 'pending', 'in_progress', 'completed', 'blocked'")]
    pub status: Option<String>,
    #[schemars(description = "Filter by project path")]
    pub project_path: Option<String>,
    #[schemars(description = "Filter by parent task ID")]
    pub parent_id: Option<String>,
    #[schemars(description = "Include completed tasks (default: false)")]
    pub include_completed: Option<bool>,
    #[schemars(description = "Maximum results (default: 20)")]
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct GetTaskRequest {
    #[schemars(description = "Task ID")]
    pub task_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct UpdateTaskRequest {
    #[schemars(description = "Task ID to update")]
    pub task_id: String,
    #[schemars(description = "New title")]
    pub title: Option<String>,
    #[schemars(description = "New description")]
    pub description: Option<String>,
    #[schemars(description = "New status")]
    pub status: Option<String>,
    #[schemars(description = "New priority")]
    pub priority: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CompleteTaskRequest {
    #[schemars(description = "Task ID to complete")]
    pub task_id: String,
    #[schemars(description = "Completion notes")]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct DeleteTaskRequest {
    #[schemars(description = "Task ID to delete")]
    pub task_id: String,
}

// ============================================================================
// Code Intelligence Tools
// ============================================================================

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct GetSymbolsRequest {
    #[schemars(description = "File path to get symbols from")]
    pub file_path: String,
    #[schemars(description = "Filter by symbol type: 'function', 'class', 'struct', 'trait'")]
    pub symbol_type: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetCallGraphRequest {
    #[schemars(description = "Symbol name to get call graph for")]
    pub symbol: String,
    #[schemars(description = "Depth of call graph (default: 2)")]
    pub depth: Option<i32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetRelatedFilesRequest {
    #[schemars(description = "File path to find related files for")]
    pub file_path: String,
    #[schemars(description = "Relation type: 'imports', 'cochange', 'all'")]
    pub relation_type: Option<String>,
    #[schemars(description = "Maximum results (default: 10)")]
    pub limit: Option<i64>,
}

// ============================================================================
// Git Intelligence Tools
// ============================================================================

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetRecentCommitsRequest {
    #[schemars(description = "Maximum commits to return (default: 20)")]
    pub limit: Option<i64>,
    #[schemars(description = "Filter by file path")]
    pub file_path: Option<String>,
    #[schemars(description = "Filter by author email")]
    pub author: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchCommitsRequest {
    #[schemars(description = "Search query for commit messages")]
    pub query: String,
    #[schemars(description = "Maximum results (default: 20)")]
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct FindCochangeRequest {
    #[schemars(description = "File path to find co-change patterns for")]
    pub file_path: String,
    #[schemars(description = "Maximum results (default: 10)")]
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct FindSimilarFixesRequest {
    #[schemars(description = "Error message or pattern to search for")]
    pub error: String,
    #[schemars(description = "Error category filter")]
    pub category: Option<String>,
    #[schemars(description = "Language filter")]
    pub language: Option<String>,
    #[schemars(description = "Maximum results (default: 5)")]
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RecordErrorFixRequest {
    #[schemars(description = "The error message/pattern")]
    pub error_pattern: String,
    #[schemars(description = "Error category: 'type_error', 'borrow_error', 'import_error', etc.")]
    pub category: Option<String>,
    #[schemars(description = "Programming language")]
    pub language: Option<String>,
    #[schemars(description = "File pattern where this occurred")]
    pub file_pattern: Option<String>,
    #[schemars(description = "Description of the fix")]
    pub fix_description: String,
    #[schemars(description = "The diff that fixed it")]
    pub fix_diff: Option<String>,
    #[schemars(description = "Commit hash")]
    pub fix_commit: Option<String>,
}

// ============================================================================
// Build Intelligence Tools
// ============================================================================

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetBuildErrorsRequest {
    #[schemars(description = "Filter by file path")]
    pub file_path: Option<String>,
    #[schemars(description = "Filter by category: 'type_error', 'syntax_error', 'linker_error'")]
    pub category: Option<String>,
    #[schemars(description = "Include resolved errors (default: false)")]
    pub include_resolved: Option<bool>,
    #[schemars(description = "Maximum results (default: 20)")]
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RecordBuildRequest {
    #[schemars(description = "Build command that was run")]
    pub command: String,
    #[schemars(description = "Whether the build succeeded")]
    pub success: bool,
    #[schemars(description = "Project path")]
    pub project_path: Option<String>,
    #[schemars(description = "Build duration in milliseconds")]
    pub duration_ms: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RecordBuildErrorRequest {
    #[schemars(description = "Error message")]
    pub message: String,
    #[schemars(description = "Error category")]
    pub category: Option<String>,
    #[schemars(description = "Severity: 'error' or 'warning'")]
    pub severity: Option<String>,
    #[schemars(description = "File path")]
    pub file_path: Option<String>,
    #[schemars(description = "Line number")]
    pub line_number: Option<i32>,
    #[schemars(description = "Error code (e.g., E0308)")]
    pub code: Option<String>,
    #[schemars(description = "Compiler suggestion")]
    pub suggestion: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ResolveErrorRequest {
    #[schemars(description = "Error ID to mark as resolved")]
    pub error_id: i64,
}

// ============================================================================
// Document Tools
// ============================================================================

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListDocumentsRequest {
    #[schemars(description = "Filter by document type: 'pdf', 'markdown', 'text', 'code'")]
    pub doc_type: Option<String>,
    #[schemars(description = "Maximum results (default: 20)")]
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct SearchDocumentsRequest {
    #[schemars(description = "Search query")]
    pub query: String,
    #[schemars(description = "Maximum results (default: 10)")]
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct GetDocumentRequest {
    #[schemars(description = "Document ID")]
    pub document_id: String,
    #[schemars(description = "Include full content (default: false)")]
    pub include_content: Option<bool>,
}

// ============================================================================
// Workspace/Context Tools
// ============================================================================

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RecordActivityRequest {
    #[schemars(description = "File path")]
    pub file_path: String,
    #[schemars(description = "Activity type: 'read', 'write', 'error', 'test'")]
    pub activity_type: String,
    #[schemars(description = "Optional context (error message, test result, etc.)")]
    pub context: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetRecentActivityRequest {
    #[schemars(description = "Filter by file path")]
    pub file_path: Option<String>,
    #[schemars(description = "Filter by activity type")]
    pub activity_type: Option<String>,
    #[schemars(description = "Maximum results (default: 20)")]
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SetContextRequest {
    #[schemars(description = "Context type: 'active_task', 'recent_error', 'current_file'")]
    pub context_type: String,
    #[schemars(description = "Context key")]
    pub key: String,
    #[schemars(description = "Context value")]
    pub value: String,
    #[schemars(description = "Priority (higher = more important)")]
    pub priority: Option<i32>,
    #[schemars(description = "TTL in seconds (optional)")]
    pub ttl_seconds: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetContextRequest {
    #[schemars(description = "Filter by context type")]
    pub context_type: Option<String>,
}

// ============================================================================
// Session/Cross-Session Memory Tools
// ============================================================================

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetSessionContextRequest {
    #[schemars(description = "Include recent memories (default: true)")]
    pub include_memories: Option<bool>,
    #[schemars(description = "Include pending tasks (default: true)")]
    pub include_tasks: Option<bool>,
    #[schemars(description = "Include recent sessions (default: true)")]
    pub include_sessions: Option<bool>,
    #[schemars(description = "Maximum items per category (default: 5)")]
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct StoreSessionRequest {
    #[schemars(description = "Summary of what happened in this session")]
    pub summary: String,
    #[schemars(description = "Session ID (auto-generated if not provided)")]
    pub session_id: Option<String>,
    #[schemars(description = "Project path this session was about")]
    pub project_path: Option<String>,
    #[schemars(description = "Key topics discussed")]
    pub topics: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchSessionsRequest {
    #[schemars(description = "Query to search past sessions (semantic search if available)")]
    pub query: String,
    #[schemars(description = "Maximum results (default: 10)")]
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct StoreDecisionRequest {
    #[schemars(description = "Unique key for this decision (for updates)")]
    pub key: String,
    #[schemars(description = "The decision or important context")]
    pub decision: String,
    #[schemars(description = "Category: 'architecture', 'api', 'convention', 'preference'")]
    pub category: Option<String>,
    #[schemars(description = "Additional context about why this decision was made")]
    pub context: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SemanticCodeSearchRequest {
    #[schemars(description = "Natural language query to find relevant code")]
    pub query: String,
    #[schemars(description = "Filter by language: 'rust', 'python', 'typescript', etc.")]
    pub language: Option<String>,
    #[schemars(description = "Maximum results (default: 10)")]
    pub limit: Option<i64>,
}

// ============================================================================
// Project Tools
// ============================================================================

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SetProjectRequest {
    #[schemars(description = "Absolute path to project root (e.g., /home/user/myproject)")]
    pub project_path: String,
    #[schemars(description = "Optional project name (auto-detected from directory name if not provided)")]
    pub name: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetProjectRequest {}

// ============================================================================
// Analytics/Introspection Tools
// ============================================================================

#[derive(Debug, Deserialize, JsonSchema)]
pub struct QueryRequest {
    #[schemars(description = "SQL SELECT query to execute")]
    pub sql: String,
    #[schemars(description = "Maximum rows to return (default: 100)")]
    pub limit: Option<i64>,
}
