//! Operation context - shared state for all core operations
//!
//! OpContext bundles all the dependencies that operations need,
//! avoiding parameter spaghetti in function signatures.

use super::primitives::semantic::SemanticSearch;
use sqlx::SqlitePool;
use std::path::PathBuf;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

/// Shared context for all core operations
///
/// Clone is cheap - all fields are either Copy, Arc, or Clone-friendly.
#[derive(Clone)]
pub struct OpContext {
    /// SQLite database pool
    pub db: Option<SqlitePool>,

    /// Semantic search client (Qdrant + embeddings)
    pub semantic: Option<Arc<SemanticSearch>>,

    /// HTTP client for web operations
    pub http: reqwest::Client,

    /// Current working directory for file operations
    pub cwd: PathBuf,

    /// Project path for scoping operations
    pub project_path: String,

    /// Cancellation token for long-running operations
    pub cancel: CancellationToken,
}

impl OpContext {
    /// Create a new context with minimal configuration
    pub fn new(cwd: PathBuf) -> Self {
        Self {
            db: None,
            semantic: None,
            http: reqwest::Client::new(),
            cwd: cwd.clone(),
            project_path: cwd.to_string_lossy().to_string(),
            cancel: CancellationToken::new(),
        }
    }

    /// Create context with database
    pub fn with_db(mut self, db: SqlitePool) -> Self {
        self.db = Some(db);
        self
    }

    /// Create context with semantic search
    pub fn with_semantic(mut self, semantic: Arc<SemanticSearch>) -> Self {
        self.semantic = Some(semantic);
        self
    }

    /// Create context with custom HTTP client
    pub fn with_http(mut self, http: reqwest::Client) -> Self {
        self.http = http;
        self
    }

    /// Create context with project path
    pub fn with_project_path(mut self, project_path: String) -> Self {
        self.project_path = project_path;
        self
    }

    /// Create context with cancellation token
    pub fn with_cancel(mut self, cancel: CancellationToken) -> Self {
        self.cancel = cancel;
        self
    }

    // =========================================================================
    // Convenience constructors for common patterns
    // =========================================================================

    /// Create context with just a database (common for MCP tools)
    pub fn just_db(db: SqlitePool) -> Self {
        Self::default().with_db(db)
    }

    /// Create context with database and semantic search (common for MCP tools)
    pub fn with_db_and_semantic(db: SqlitePool, semantic: Arc<SemanticSearch>) -> Self {
        Self::default()
            .with_db(db)
            .with_semantic(semantic)
    }

    /// Check if database is available
    pub fn has_db(&self) -> bool {
        self.db.is_some()
    }

    /// Check if semantic search is available
    pub fn has_semantic(&self) -> bool {
        self.semantic.is_some()
    }

    /// Get database, returning error if unavailable
    pub fn require_db(&self) -> Result<&SqlitePool, super::CoreError> {
        self.db.as_ref().ok_or(super::CoreError::DatabaseUnavailable)
    }

    /// Get semantic search, returning error if unavailable
    pub fn require_semantic(&self) -> Result<&Arc<SemanticSearch>, super::CoreError> {
        self.semantic
            .as_ref()
            .ok_or(super::CoreError::SemanticUnavailable)
    }

    /// Check if operation should be cancelled
    pub fn is_cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }

    /// Return error if cancelled
    pub fn check_cancelled(&self) -> Result<(), super::CoreError> {
        if self.is_cancelled() {
            Err(super::CoreError::Cancelled)
        } else {
            Ok(())
        }
    }
}

impl Default for OpContext {
    fn default() -> Self {
        Self::new(std::env::current_dir().unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_context_builder() {
        let ctx = OpContext::new(PathBuf::from("/tmp"))
            .with_project_path("/home/user/project".into());

        assert_eq!(ctx.cwd, PathBuf::from("/tmp"));
        assert_eq!(ctx.project_path, "/home/user/project");
        assert!(!ctx.has_db());
        assert!(!ctx.has_semantic());
    }

    #[test]
    fn test_cancellation() {
        let cancel = CancellationToken::new();
        let ctx = OpContext::default().with_cancel(cancel.clone());

        assert!(!ctx.is_cancelled());
        assert!(ctx.check_cancelled().is_ok());

        cancel.cancel();

        assert!(ctx.is_cancelled());
        assert!(ctx.check_cancelled().is_err());
    }
}
