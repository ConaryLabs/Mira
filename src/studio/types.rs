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

// === Anthropic API Cache Types ===

/// Cache control for Anthropic prompt caching
#[derive(Debug, Clone, Serialize)]
pub struct CacheControl {
    #[serde(rename = "type")]
    pub cache_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttl: Option<String>,
}

impl CacheControl {
    /// Create ephemeral cache with default 5m TTL
    pub fn ephemeral() -> Self {
        Self {
            cache_type: "ephemeral".to_string(),
            ttl: None,
        }
    }

    /// Create ephemeral cache with 1h TTL
    pub fn ephemeral_1h() -> Self {
        Self {
            cache_type: "ephemeral".to_string(),
            ttl: Some("1h".to_string()),
        }
    }
}

/// System prompt block with optional cache control
#[derive(Debug, Clone, Serialize)]
pub struct SystemBlock {
    #[serde(rename = "type")]
    pub block_type: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<CacheControl>,
}

impl SystemBlock {
    /// Create a text block without caching
    pub fn text(content: impl Into<String>) -> Self {
        Self {
            block_type: "text".to_string(),
            text: content.into(),
            cache_control: None,
        }
    }

    /// Create a text block with 5m cache
    pub fn cached(content: impl Into<String>) -> Self {
        Self {
            block_type: "text".to_string(),
            text: content.into(),
            cache_control: Some(CacheControl::ephemeral()),
        }
    }

    /// Create a text block with 1h cache
    pub fn cached_1h(content: impl Into<String>) -> Self {
        Self {
            block_type: "text".to_string(),
            text: content.into(),
            cache_control: Some(CacheControl::ephemeral_1h()),
        }
    }
}

/// Content block for messages (supports cache control)
#[derive(Debug, Clone, Serialize)]
pub struct ContentBlock {
    #[serde(rename = "type")]
    pub block_type: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<CacheControl>,
}

impl ContentBlock {
    /// Create a text content block with cache control
    pub fn cached(content: impl Into<String>) -> Self {
        Self {
            block_type: "text".to_string(),
            text: content.into(),
            cache_control: Some(CacheControl::ephemeral()),
        }
    }
}

/// Message content - either simple string or blocks with cache control
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum MessageContent {
    Text(String),
    Blocks(Vec<ContentBlock>),
}

/// Chat message with flexible content (string or blocks)
#[derive(Debug, Clone, Serialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: MessageContent,
}

impl ChatMessage {
    /// Create a simple text message
    pub fn text(role: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            content: MessageContent::Text(content.into()),
        }
    }

    /// Create a message with cache control on content
    pub fn cached(role: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            content: MessageContent::Blocks(vec![ContentBlock::cached(content)]),
        }
    }
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
    pub system: Option<Vec<SystemBlock>>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct AnthropicStreamEvent {
    #[serde(rename = "type")]
    pub event_type: String,
    #[serde(default)]
    pub delta: Option<ContentDelta>,
    #[serde(default)]
    pub usage: Option<UsageInfo>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ContentDelta {
    #[serde(rename = "type")]
    #[allow(dead_code)]
    pub delta_type: Option<String>,
    pub text: Option<String>,
}

/// Token usage info from Anthropic API (includes cache metrics)
#[derive(Debug, Clone, Deserialize, Default)]
pub struct UsageInfo {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub cache_creation_input_tokens: Option<u64>,
    #[serde(default)]
    pub cache_read_input_tokens: Option<u64>,
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
