// src/operations/engine/events.rs
// Event types emitted during operation execution

use crate::operations::Artifact;

/// Events emitted by the operation engine
#[derive(Debug, Clone)]
pub enum OperationEngineEvent {
    Started {
        operation_id: String,
    },
    StatusChanged {
        operation_id: String,
        old_status: String,
        new_status: String,
    },
    Streaming {
        operation_id: String,
        content: String,
    },
    /// Plan was generated for a complex operation
    PlanGenerated {
        operation_id: String,
        plan_text: String,
        reasoning_tokens: Option<i32>,
    },
    Delegated {
        operation_id: String,
        delegated_to: String,
        reason: String,
    },
    ArtifactPreview {
        operation_id: String,
        artifact_id: String,
        path: String,
        preview: String,
    },
    ArtifactCompleted {
        operation_id: String,
        artifact: Artifact,
    },
    Completed {
        operation_id: String,
        result: Option<String>,
        artifacts: Vec<Artifact>,
    },
    Failed {
        operation_id: String,
        error: String,
    },
    /// Task was created for tracking operation progress
    TaskCreated {
        operation_id: String,
        task_id: String,
        sequence: i32,
        description: String,
        active_form: String,
    },
    /// Task execution started
    TaskStarted {
        operation_id: String,
        task_id: String,
    },
    /// Task completed successfully
    TaskCompleted {
        operation_id: String,
        task_id: String,
    },
    /// Task failed with error
    TaskFailed {
        operation_id: String,
        task_id: String,
        error: String,
    },
    /// Sudo command requires user approval
    SudoApprovalRequired {
        operation_id: String,
        approval_request_id: String,
        command: String,
        reason: Option<String>,
    },
    /// Sudo approval was granted
    SudoApproved {
        operation_id: String,
        approval_request_id: String,
        approved_by: String,
    },
    /// Sudo approval was denied
    SudoDenied {
        operation_id: String,
        approval_request_id: String,
        denied_by: String,
        reason: Option<String>,
    },
    /// Tool was executed (file operations, git, code intelligence, etc.)
    ToolExecuted {
        operation_id: String,
        tool_name: String,
        tool_type: String, // 'file_write', 'file_edit', 'file_read', 'git', 'code_analysis', etc.
        summary: String, // Human-readable summary like "Wrote file src/main.rs (245 lines)"
        success: bool,
        details: Option<serde_json::Value>, // Optional structured data
    },
    /// Agent was spawned for a subtask
    AgentSpawned {
        operation_id: String,
        agent_execution_id: String,
        agent_name: String,
        task: String,
    },
    /// Agent is making progress
    AgentProgress {
        operation_id: String,
        agent_execution_id: String,
        agent_name: String,
        iteration: usize,
        max_iterations: usize,
        current_activity: String,
    },
    /// Agent streaming content
    AgentStreaming {
        operation_id: String,
        agent_execution_id: String,
        content: String,
    },
    /// Agent completed successfully
    AgentCompleted {
        operation_id: String,
        agent_execution_id: String,
        agent_name: String,
        summary: String,
        iterations_used: usize,
    },
    /// Agent failed with error
    AgentFailed {
        operation_id: String,
        agent_execution_id: String,
        agent_name: String,
        error: String,
    },
    /// Context size warning - approaching or exceeding pricing threshold
    ContextWarning {
        operation_id: String,
        warning_level: String, // "approaching", "near_threshold", "over_threshold"
        message: String,
        tokens_input: i64,
        threshold: i64,
    },
    /// LLM usage info - sent after each LLM call with pricing tier information
    UsageInfo {
        operation_id: String,
        tokens_input: i64,
        tokens_output: i64,
        pricing_tier: String, // "standard" or "large_context"
        cost_usd: f64,
        from_cache: bool,
    },
    /// Thinking status - shows what the AI is currently doing
    /// Similar to Claude Code's "Opus thinking..." status line
    Thinking {
        operation_id: String,
        status: String,              // "gathering_context", "thinking", "executing_tool"
        message: String,             // Human-readable status like "Thinking..."
        tokens_in: i64,              // Cumulative input tokens so far
        tokens_out: i64,             // Cumulative output tokens so far
        active_tool: Option<String>, // Currently executing tool (if any)
    },
}
