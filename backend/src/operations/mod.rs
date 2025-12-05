// src/operations/mod.rs
// FIXED: Expanded Artifact struct to use all database fields

pub mod context_loader;
pub mod delegation_tools;
pub mod engine;
pub mod tasks;
pub mod tool_builder;
pub mod tools;
pub mod types;

pub use context_loader::ContextLoader;
pub use delegation_tools::{get_delegation_tools, parse_tool_call};
pub use engine::{OperationEngine, OperationEngineEvent};
pub use tasks::{OperationTask, TaskManager, TaskProgress, TaskStatus};
pub use tools::{get_code_tools, get_external_tools, get_file_low_level_tools, get_file_operation_tools, get_git_tools};

use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

/// Core operation tracking for coding workflows
/// Maps directly to `operations` table in database
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Operation {
    pub id: String,
    pub session_id: String,
    pub kind: String,   // e.g., "code_generation", "refactor", etc.
    pub status: String, // e.g., "pending", "completed", "failed"

    // Timing
    #[sqlx(default)]
    pub created_at: i64,
    pub started_at: Option<i64>,
    pub completed_at: Option<i64>,

    // Input
    pub user_message: String,
    pub context_snapshot: Option<String>, // JSON snapshot of relevant context

    // Analysis & Routing
    pub complexity_score: Option<f64>,
    pub delegated_to: Option<String>,  // e.g., "gemini"
    pub primary_model: Option<String>, // e.g., "gemini-2.5-flash"
    pub delegation_reason: Option<String>,

    // LLM Response Tracking
    pub response_id: Option<String>,
    pub parent_response_id: Option<String>,
    pub parent_operation_id: Option<String>,

    // Code-specific context
    pub target_language: Option<String>,
    pub target_framework: Option<String>,
    pub operation_intent: Option<String>,
    pub files_affected: Option<String>, // JSON array

    // Results
    pub result: Option<String>,
    pub error: Option<String>,

    // Cost Tracking
    pub tokens_input: Option<i64>,
    pub tokens_output: Option<i64>,
    pub tokens_reasoning: Option<i64>,
    pub cost_usd: Option<f64>,
    #[sqlx(default)]
    pub delegate_calls: i64,

    // Metadata
    pub metadata: Option<String>, // JSON
}

/// Events that occur during an operation's lifecycle
/// Maps directly to `operation_events` table
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct OperationEvent {
    pub id: i64, // Changed from String to i64 to match INTEGER PRIMARY KEY AUTOINCREMENT
    pub operation_id: String,
    pub event_type: String, // e.g., "started", "analysis", "delegated", "completed"
    pub created_at: i64,
    pub sequence_number: i64,
    pub event_data: Option<String>, // JSON
}

/// Artifacts generated during operations (code files, documents, etc.)
///
/// Database schema includes comprehensive metadata fields for tracking generation context,
/// performance metrics, and relationships. This struct exposes all available fields.
///
/// Maps to `artifacts` table (see migration 20251016_unified_baseline.sql)
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Artifact {
    // Core identification
    pub id: String,
    pub operation_id: String,
    pub kind: String, // e.g., "code", "document", "diagram"

    // Content fields
    pub file_path: Option<String>,
    pub content: String,
    pub content_hash: String, // SHA-256 for deduplication
    pub language: Option<String>,

    // FIXED: Database column is 'diff_from_previous' but API uses 'diff'
    #[sqlx(rename = "diff_from_previous")]
    pub diff: Option<String>,

    // Preview for quick display (first N lines)
    pub preview: Option<String>,

    // Relationship tracking
    pub previous_artifact_id: Option<String>,

    // FIXED: SQLite uses INTEGER for booleans (0/1)
    #[sqlx(rename = "is_new_file")]
    pub is_new_file: Option<i32>, // 0 = false, 1 = true

    // Context JSON fields - stored as TEXT in SQLite
    pub related_files: Option<String>,   // JSON array of file paths
    pub dependencies: Option<String>,    // JSON array of dependencies
    pub project_context: Option<String>, // JSON object with project info
    pub user_requirements: Option<String>, // JSON object with requirements
    pub constraints: Option<String>,     // JSON array of constraints

    // Generation metadata
    pub generated_by: Option<String>,    // e.g., "gemini"
    pub generation_time_ms: Option<i64>, // How long generation took
    pub context_tokens: Option<i64>,     // Tokens in context
    pub output_tokens: Option<i64>,      // Tokens generated

    // Lifecycle timestamps
    pub created_at: i64,
    pub completed_at: Option<i64>,
    pub applied_at: Option<i64>,

    // Additional metadata
    pub metadata: Option<String>, // JSON object for extensibility
}

impl Operation {
    /// Create a new operation
    pub fn new(session_id: String, kind: String, user_message: String) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            session_id,
            kind,
            status: "pending".to_string(),
            created_at: chrono::Utc::now().timestamp(),
            started_at: None,
            completed_at: None,
            user_message,
            context_snapshot: None,
            complexity_score: None,
            delegated_to: None,
            primary_model: None,
            delegation_reason: None,
            response_id: None,
            parent_response_id: None,
            parent_operation_id: None,
            target_language: None,
            target_framework: None,
            operation_intent: None,
            files_affected: None,
            result: None,
            error: None,
            tokens_input: None,
            tokens_output: None,
            tokens_reasoning: None,
            cost_usd: None,
            delegate_calls: 0,
            metadata: None,
        }
    }
}

impl OperationEvent {
    /// Create a new event - DB will auto-generate the ID
    pub fn new(
        operation_id: String,
        event_type: String,
        sequence_number: i64,
        event_data: Option<String>,
    ) -> Self {
        Self {
            id: 0, // Placeholder - DB will auto-increment this
            operation_id,
            event_type,
            created_at: chrono::Utc::now().timestamp(),
            sequence_number,
            event_data,
        }
    }
}

impl Artifact {
    /// Create a new artifact with core fields (backward compatible)
    pub fn new(
        operation_id: String,
        kind: String,
        file_path: Option<String>,
        content: String,
        content_hash: String,
        language: Option<String>,
        diff: Option<String>,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            operation_id,
            kind,
            file_path,
            content,
            content_hash,
            language,
            diff,
            created_at: chrono::Utc::now().timestamp(),

            // Default all optional metadata fields to None
            preview: None,
            previous_artifact_id: None,
            is_new_file: None,
            related_files: None,
            dependencies: None,
            project_context: None,
            user_requirements: None,
            constraints: None,
            generated_by: None,
            generation_time_ms: None,
            context_tokens: None,
            output_tokens: None,
            completed_at: None,
            applied_at: None,
            metadata: None,
        }
    }

    /// Create a new artifact with generation metadata
    #[allow(clippy::too_many_arguments)]
    pub fn with_metadata(
        operation_id: String,
        kind: String,
        file_path: Option<String>,
        content: String,
        content_hash: String,
        language: Option<String>,
        diff: Option<String>,
        generated_by: Option<String>,
        generation_time_ms: Option<i64>,
        context_tokens: Option<i64>,
        output_tokens: Option<i64>,
    ) -> Self {
        let mut artifact = Self::new(
            operation_id,
            kind,
            file_path,
            content,
            content_hash,
            language,
            diff,
        );

        artifact.generated_by = generated_by;
        artifact.generation_time_ms = generation_time_ms;
        artifact.context_tokens = context_tokens;
        artifact.output_tokens = output_tokens;

        artifact
    }

    /// Set preview (first N lines of content)
    pub fn with_preview(mut self, lines: usize) -> Self {
        let preview: String = self
            .content
            .lines()
            .take(lines)
            .collect::<Vec<_>>()
            .join("\n");
        self.preview = Some(preview);
        self
    }

    /// Mark as new file
    pub fn as_new_file(mut self) -> Self {
        self.is_new_file = Some(1);
        self
    }

    /// Mark as existing file modification
    pub fn as_modification(mut self) -> Self {
        self.is_new_file = Some(0);
        self
    }

    /// Set related files from Vec
    pub fn with_related_files(mut self, files: Vec<String>) -> Self {
        self.related_files = Some(serde_json::to_string(&files).unwrap_or_default());
        self
    }

    /// Set dependencies from Vec
    pub fn with_dependencies(mut self, deps: Vec<String>) -> Self {
        self.dependencies = Some(serde_json::to_string(&deps).unwrap_or_default());
        self
    }

    /// Get related files as Vec
    pub fn get_related_files(&self) -> Vec<String> {
        self.related_files
            .as_ref()
            .and_then(|json| serde_json::from_str(json).ok())
            .unwrap_or_default()
    }

    /// Get dependencies as Vec
    pub fn get_dependencies(&self) -> Vec<String> {
        self.dependencies
            .as_ref()
            .and_then(|json| serde_json::from_str(json).ok())
            .unwrap_or_default()
    }

    /// Check if this is a new file
    pub fn is_new(&self) -> bool {
        self.is_new_file == Some(1)
    }

    /// Mark as completed
    pub fn mark_completed(mut self) -> Self {
        self.completed_at = Some(chrono::Utc::now().timestamp());
        self
    }

    /// Mark as applied
    pub fn mark_applied(mut self) -> Self {
        self.applied_at = Some(chrono::Utc::now().timestamp());
        self
    }
}
