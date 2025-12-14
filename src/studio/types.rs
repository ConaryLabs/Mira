// src/studio/types.rs
// Type definitions for Mira Studio

use reqwest::Client;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::sync::Arc;
use tokio::sync::broadcast;

use crate::tools::SemanticSearch;

/// Events streamed to the workspace terminal
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkspaceEvent {
    /// System info message
    Info { message: String },
    /// Tool/operation started
    ToolStart { tool: String, args: Option<String> },
    /// Tool/operation completed
    ToolEnd { tool: String, result: Option<String>, success: bool },
    /// Memory operation
    Memory { action: String, content: String },
    /// File operation (reserved for future use)
    #[allow(dead_code)]
    File { action: String, path: String },
    /// Context loaded
    Context { kind: String, count: usize },
    /// Claude Code session starting
    ClaudeCodeStart { task: String },
    /// Claude Code output line
    ClaudeCodeOutput { line: String, stream: String },
    /// Claude Code session ended
    ClaudeCodeEnd { exit_code: i32, success: bool },
}

#[derive(Clone)]
pub struct StudioState {
    pub db: Arc<SqlitePool>,
    pub semantic: Arc<SemanticSearch>,
    pub http_client: Client,
    pub anthropic_key: Option<String>,
    pub workspace_tx: broadcast::Sender<WorkspaceEvent>,
}

impl StudioState {
    pub fn new(
        db: Arc<SqlitePool>,
        semantic: Arc<SemanticSearch>,
        http_client: Client,
        anthropic_key: Option<String>,
    ) -> Self {
        let (workspace_tx, _) = broadcast::channel(100);
        Self {
            db,
            semantic,
            http_client,
            anthropic_key,
            workspace_tx,
        }
    }

    /// Emit a workspace event (non-blocking, ignores if no subscribers)
    pub fn emit(&self, event: WorkspaceEvent) {
        let _ = self.workspace_tx.send(event);
    }
}

#[derive(Debug, Deserialize)]
pub struct ChatRequest {
    /// Conversation ID (created if not provided)
    #[serde(default)]
    pub conversation_id: Option<String>,
    /// The new user message
    pub message: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
}

fn default_max_tokens() -> u32 {
    4096
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Serialize)]
pub struct ConversationInfo {
    pub id: String,
    pub title: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub message_count: i64,
}

#[derive(Debug, Serialize)]
pub struct MessageInfo {
    pub id: String,
    pub role: String,
    pub content: String,
    pub created_at: i64,
}

#[derive(Debug, Deserialize)]
pub struct MessagesQuery {
    #[serde(default = "default_limit")]
    pub limit: i64,
    pub before: Option<String>,
}

fn default_limit() -> i64 {
    20
}

#[derive(Debug, Serialize)]
pub(crate) struct AnthropicRequest {
    pub model: String,
    pub max_tokens: u32,
    pub messages: Vec<ChatMessage>,
    pub stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct AnthropicStreamEvent {
    #[serde(rename = "type")]
    pub event_type: String,
    #[serde(default)]
    pub delta: Option<ContentDelta>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ContentDelta {
    #[serde(rename = "type")]
    #[allow(dead_code)]
    pub delta_type: Option<String>,
    pub text: Option<String>,
}

#[derive(Default)]
pub(crate) struct WorkContextCounts {
    pub goals: usize,
    pub tasks: usize,
    pub corrections: usize,
    pub documents: usize,
}

impl WorkContextCounts {
    pub fn total(&self) -> usize {
        self.goals + self.tasks + self.corrections + self.documents
    }
}

/// Request to launch a Claude Code session
#[derive(Debug, Deserialize)]
pub struct LaunchClaudeCodeRequest {
    /// The task for Claude Code to work on
    pub task: String,
    /// Project path (defaults to current directory)
    #[serde(default)]
    pub project_path: Option<String>,
}
