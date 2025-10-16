// src/operations/types.rs

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Operation kinds (stored as strings in DB)
pub mod operation_kinds {
    pub const CODE_GENERATION: &str = "code_generation";
    pub const CODE_MODIFICATION: &str = "code_modification";
    pub const CODE_REVIEW: &str = "code_review";
    pub const REFACTOR: &str = "refactor";
    pub const DEBUG: &str = "debug";
}

/// Operation statuses (stored as strings in DB)
pub mod operation_status {
    pub const PENDING: &str = "pending";
    pub const PLANNING: &str = "planning";
    pub const DELEGATED: &str = "delegated";
    pub const GENERATING: &str = "generating";
    pub const REVIEWING: &str = "reviewing";
    pub const COMPLETED: &str = "completed";
    pub const FAILED: &str = "failed";
    pub const CANCELLED: &str = "cancelled";
}

/// Artifact kinds (stored as strings in DB)
pub mod artifact_kinds {
    pub const FILE: &str = "file";
    pub const SNIPPET: &str = "snippet";
    pub const DIFF: &str = "diff";
    pub const TEST: &str = "test";
}

/// Event types (stored as strings in DB)
pub mod event_types {
    pub const STATUS_CHANGE: &str = "status_change";
    pub const GPT5_ANALYSIS: &str = "gpt5_analysis";
    pub const DELEGATION: &str = "delegation";
    pub const DEEPSEEK_PROGRESS: &str = "deepseek_progress";
    pub const ARTIFACT_CREATED: &str = "artifact_created";
    pub const ARTIFACT_UPDATED: &str = "artifact_updated";
    pub const REVIEW_FEEDBACK: &str = "review_feedback";
    pub const ERROR: &str = "error";
    pub const USER_FEEDBACK: &str = "user_feedback";
}

/// Structured context snapshot for operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextSnapshot {
    pub project_id: Option<String>,
    pub active_files: Vec<String>,
    pub git_branch: Option<String>,
    pub git_dirty: bool,
    pub recent_changes: Vec<RecentChange>,
    pub relevant_code: Vec<CodeContext>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecentChange {
    pub file_path: String,
    pub change_type: String, // "created", "modified", "deleted"
    pub timestamp: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeContext {
    pub file_path: String,
    pub content: String,
    pub language: String,
    pub relevance_score: f32,
}

/// Structured context for code generation operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeGenerationContext {
    pub file_path: String,
    pub description: String,
    pub requirements: Vec<String>,
    pub style_preferences: Option<HashMap<String, String>>,
    pub related_files: Vec<String>,
}

/// Structured context for code modification operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeModificationContext {
    pub file_path: String,
    pub target_section: Option<String>,
    pub modification_type: String, // "add", "update", "delete", "refactor"
    pub description: String,
    pub preserve_behavior: bool,
}

/// Delegation instructions for DeepSeek
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DelegationInstructions {
    pub task_description: String,
    pub code_context: Vec<CodeFile>,
    pub constraints: Vec<String>,
    pub style_guide: Option<String>,
    pub expected_output_format: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeFile {
    pub path: String,
    pub content: String,
    pub language: String,
    pub relevance: f32,
}

/// Payload for status change events
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusChangePayload {
    pub from_status: String,
    pub to_status: String,
    pub reason: Option<String>,
}

/// Payload for GPT-5 analysis events
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Gpt5AnalysisPayload {
    pub analysis: String,
    pub response_id: String,
    pub suggested_approach: String,
    pub complexity_score: f64,
    pub should_delegate: bool,
    pub delegation_reason: Option<String>,
}

/// Payload for delegation events
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DelegationPayload {
    pub delegated_to: String, // e.g., "deepseek-reasoner-3.2"
    pub instructions: DelegationInstructions,
    pub estimated_duration_ms: Option<u64>,
}

/// Payload for DeepSeek progress events
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeepseekProgressPayload {
    pub stage: String,
    pub progress_percent: Option<f32>,
    pub current_step: String,
    pub partial_output: Option<String>,
}

/// Payload for artifact created/updated events
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactPayload {
    pub artifact_id: String,
    pub file_path: Option<String>,
    pub kind: String,
    pub size_bytes: usize,
    pub has_diff: bool,
}

/// Payload for review feedback events
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewFeedbackPayload {
    pub reviewer: String, // "gpt5"
    pub response_id: String,
    pub rating: String, // "excellent", "good", "acceptable", "needs_work", "rejected"
    pub feedback: String,
    pub suggested_improvements: Vec<String>,
    pub approved: bool,
}

/// Payload for error events
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorPayload {
    pub error_type: String,
    pub message: String,
    pub details: Option<serde_json::Value>,
    pub recoverable: bool,
}

/// Payload for user feedback events
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserFeedbackPayload {
    pub feedback_type: String, // "accept", "reject", "request_changes", "comment"
    pub content: String,
    pub artifact_id: Option<String>,
}

/// Statistics about an operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationStats {
    pub duration_ms: u64,
    pub artifacts_generated: usize,
    pub events_count: usize,
    pub tokens_used: Option<TokenUsage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input: i64,
    pub output: i64,
    pub reasoning: Option<i64>,
    pub total: i64,
}

/// Metadata for artifacts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactMetadata {
    pub generated_by: String,
    pub generation_time_ms: i64,
    pub context_tokens: i64,
    pub output_tokens: i64,
    pub model_version: String,
}
