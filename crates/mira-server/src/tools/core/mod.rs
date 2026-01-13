//! Core tool implementations for MCP.
//!
//! All tools are implemented as async functions that accept `&impl ToolContext`
//! and return `Result<String, String>` for consistent error handling.

use async_trait::async_trait;
use mira_types::{ProjectContext, WsEvent};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{oneshot, RwLock};

use crate::db::Database;
use crate::embeddings::Embeddings;
use crate::llm::DeepSeekClient;
use crate::background::watcher::WatcherHandle;

/// Common context required by all tools.
/// Implemented by MiraServer (MCP).
#[async_trait]
pub trait ToolContext: Send + Sync {
    // === Core Resources (always available) ===

    /// Database connection with semantic search capabilities
    fn db(&self) -> &Arc<Database>;

    /// Embeddings client for semantic search (optional)
    fn embeddings(&self) -> Option<&Arc<Embeddings>>;

    /// DeepSeek client for chat/completion (optional)
    fn deepseek(&self) -> Option<&Arc<DeepSeekClient>>;

    // === Project/Session State ===

    /// Get current project context (if any)
    async fn get_project(&self) -> Option<ProjectContext>;

    /// Set current project context
    async fn set_project(&self, project: ProjectContext);

    /// Get current project ID (convenience method)
    async fn project_id(&self) -> Option<i64> {
        self.get_project().await.map(|p| p.id)
    }

    /// Get current session ID (if any)
    async fn get_session_id(&self) -> Option<String>;

    /// Get or create a session ID for the current project
    async fn get_or_create_session(&self) -> String;

    // === Event Broadcasting ===

    /// Broadcast a WebSocket event to connected clients (no-op if no broadcaster)
    fn broadcast(&self, event: WsEvent);

    // === Optional Services ===

    /// Pending responses for agent collaboration
    fn pending_responses(&self) -> Option<&Arc<RwLock<HashMap<String, oneshot::Sender<String>>>>> {
        None
    }

    /// Watcher handle for file system monitoring (optional)
    fn watcher(&self) -> Option<&WatcherHandle> {
        None
    }
}

// Sub-modules with tool implementations
pub mod bash;
pub mod code;
pub mod experts;
pub mod memory;
pub mod project;
pub mod tasks_goals;

// Re-export commonly used functions
pub use bash::*;
pub use code::*;
pub use experts::*;
pub use memory::*;
pub use project::*;
pub use tasks_goals::*;
