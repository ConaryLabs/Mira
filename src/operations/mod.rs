// src/operations/mod.rs

pub mod engine;
pub mod types;

pub use engine::{OperationEngine, OperationEngineEvent};

use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

/// Core operation tracking for coding workflows
/// Maps directly to `operations` table in database
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Operation {
    pub id: String,
    pub session_id: String,
    pub kind: String, // e.g., "code_generation", "refactor", etc.
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
    pub delegated_to: Option<String>, // e.g., "deepseek"
    pub primary_model: Option<String>, // e.g., "gpt-5"
    pub delegation_reason: Option<String>,
    
    // GPT-5 Responses API Tracking
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
    #[sqlx(default)]
    pub id: i64,
    pub operation_id: String,
    pub event_type: String,
    pub event_data: Option<String>, // JSON
    pub sequence_number: i64,
    #[sqlx(default)]
    pub created_at: i64,
}

/// Generated code artifacts from operations
/// Maps directly to `artifacts` table
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Artifact {
    pub id: String,
    pub operation_id: String,
    
    // Artifact content
    pub kind: String, // "file", "snippet", "diff", "test"
    pub file_path: Option<String>,
    pub content: String,
    pub preview: Option<String>,
    
    // Code-specific fields
    pub language: Option<String>,
    
    // Change tracking
    pub content_hash: Option<String>, // SHA-256 of content
    pub previous_artifact_id: Option<String>,
    #[sqlx(default)]
    pub is_new_file: i64, // SQLite boolean (0 or 1)
    pub diff_from_previous: Option<String>,
    
    // Context used for generation
    pub related_files: Option<String>, // JSON array
    pub dependencies: Option<String>, // JSON array
    pub project_context: Option<String>, // JSON
    pub user_requirements: Option<String>,
    pub constraints: Option<String>,
    
    // Timing
    #[sqlx(default)]
    pub created_at: i64,
    pub completed_at: Option<i64>,
    pub applied_at: Option<i64>, // When user applied to codebase
    
    // Generation metadata
    pub generated_by: Option<String>, // e.g., "deepseek-reasoner-3.2"
    pub generation_time_ms: Option<i64>,
    pub context_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    
    // Metadata
    pub metadata: Option<String>, // JSON
}

impl Operation {
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
