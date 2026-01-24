//! Core tool implementations for MCP.
//!
//! All tools are implemented as async functions that accept `&impl ToolContext`
//! and return `Result<String, String>` for consistent error handling.

use async_trait::async_trait;
use mira_types::{ProjectContext, WsEvent};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{oneshot, RwLock};

use crate::db::pool::DatabasePool;
use crate::embeddings::EmbeddingClient;
use crate::llm::{DeepSeekClient, ProviderFactory};
use crate::background::watcher::WatcherHandle;

/// Common context required by all tools.
/// Implemented by MiraServer (MCP).
#[async_trait]
pub trait ToolContext: Send + Sync {
    // === Core Resources (always available) ===

    /// Async connection pool for database operations
    fn pool(&self) -> &Arc<DatabasePool>;

    /// Embeddings client for semantic search (optional)
    fn embeddings(&self) -> Option<&Arc<EmbeddingClient>>;

    /// DeepSeek client for chat/completion (optional, deprecated - use llm_factory)
    fn deepseek(&self) -> Option<&Arc<DeepSeekClient>>;

    /// LLM provider factory for multi-provider support
    fn llm_factory(&self) -> &ProviderFactory;

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

    /// Set the session ID
    async fn set_session_id(&self, session_id: String);

    /// Get or create a session ID for the current project
    async fn get_or_create_session(&self) -> String;

    // === User Identity ===

    /// Get the current user's identity (for multi-user memory scoping)
    /// Returns None if identity cannot be determined
    fn get_user_identity(&self) -> Option<String> {
        crate::identity::get_current_user_identity()
    }

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
pub mod claude_local;
pub mod code;
pub mod dev;
pub mod diff;
pub mod documentation;
pub mod experts;
pub mod memory;
pub mod project;
pub mod reviews;
pub mod session;
pub mod session_notes;
pub mod tasks_goals;
pub mod teams;

// Re-export commonly used functions
pub use claude_local::export_claude_local;
pub use code::*;
pub use dev::*;
pub use diff::*;
pub use documentation::*;
pub use experts::*;
pub use memory::*;
pub use project::*;
pub use reviews::*;
pub use session::*;
pub use tasks_goals::*;
pub use teams::*;
