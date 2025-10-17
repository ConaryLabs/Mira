// src/operations/mod.rs

pub mod engine;
pub mod types;
pub mod delegation_tools;

pub use engine::{OperationEngine, OperationEngineEvent};
pub use delegation_tools::{get_delegation_tools, parse_tool_call};

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
    pub id: String,
    pub operation_id: String,
    pub event_type: String, // e.g., "started", "analysis", "delegated", "completed"
    pub created_at: i64,
    pub sequence_number: i64,
    pub payload: Option<String>, // JSON
}

/// Artifacts generated during operations (code files, documents, etc.)
/// Maps directly to `artifacts` table
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Artifact {
    pub id: String,
    pub operation_id: String,
    pub kind: String, // e.g., "code", "document", "diagram"
    pub file_path: Option<String>,
    pub content: String,
    pub content_hash: String, // SHA-256 for deduplication
    pub language: Option<String>,
    pub diff: Option<String>,
    pub created_at: i64,
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
    /// Create a new event
    pub fn new(
        operation_id: String,
        event_type: String,
        sequence_number: i64,
        payload: Option<String>,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            operation_id,
            event_type,
            created_at: chrono::Utc::now().timestamp(),
            sequence_number,
            payload,
        }
    }
}

impl Artifact {
    /// Create a new artifact
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
        }
    }
}
