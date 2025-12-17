//! Tool definitions and executor for GPT-5.2 function calling
//!
//! Implements coding assistant tools:
//! - File operations (read, write, edit, glob, grep)
//! - Shell execution
//! - Web search/fetch
//! - Memory (remember, recall)
//! - Mira power armor (task, goal, correction, store_decision, record_rejected_approach)
//!
//! Tools are executed locally, results returned to GPT-5.2

mod definitions;
mod file;
mod memory;
mod mira;
mod shell;
pub mod types;
mod web;

use anyhow::Result;
use serde_json::Value;
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

pub use definitions::get_tools;
pub use types::{DiffInfo, RichToolResult};

use crate::semantic::SemanticSearch;
use crate::session::SessionManager;

/// Cached file entry with content and timestamp
#[derive(Clone)]
struct CacheEntry {
    content: String,
    cached_at: Instant,
}

/// Thread-safe file cache
#[derive(Clone, Default)]
pub struct FileCache {
    entries: Arc<RwLock<HashMap<PathBuf, CacheEntry>>>,
}

impl FileCache {
    const TTL: Duration = Duration::from_secs(30);
    const MAX_ENTRIES: usize = 100;
    const MAX_FILE_SIZE: usize = 512 * 1024; // 512KB max cached file

    pub fn new() -> Self {
        Self::default()
    }

    /// Get cached content if fresh
    pub fn get(&self, path: &PathBuf) -> Option<String> {
        let entries = self.entries.read().ok()?;
        let entry = entries.get(path)?;

        if entry.cached_at.elapsed() < Self::TTL {
            Some(entry.content.clone())
        } else {
            None
        }
    }

    /// Cache file content
    pub fn put(&self, path: PathBuf, content: String) {
        // Don't cache large files
        if content.len() > Self::MAX_FILE_SIZE {
            return;
        }

        if let Ok(mut entries) = self.entries.write() {
            // Evict oldest entries if at capacity
            if entries.len() >= Self::MAX_ENTRIES {
                // Find and remove oldest entry
                if let Some(oldest_key) = entries
                    .iter()
                    .min_by_key(|(_, v)| v.cached_at)
                    .map(|(k, _)| k.clone())
                {
                    entries.remove(&oldest_key);
                }
            }

            entries.insert(path, CacheEntry {
                content,
                cached_at: Instant::now(),
            });
        }
    }

    /// Invalidate cache entry (on write/edit)
    pub fn invalidate(&self, path: &PathBuf) {
        if let Ok(mut entries) = self.entries.write() {
            entries.remove(path);
        }
    }

    /// Update cache with new content (after write/edit)
    pub fn update(&self, path: PathBuf, content: String) {
        self.invalidate(&path);
        self.put(path, content);
    }
}

use file::FileTools;
use memory::MemoryTools;
use mira::MiraTools;
use shell::ShellTools;
use web::WebTools;

/// Tool executor handles tool invocation and result formatting
///
/// Clone is cheap - uses Arc for shared state.
#[derive(Clone)]
pub struct ToolExecutor {
    /// Working directory for file operations
    pub cwd: std::path::PathBuf,
    /// Semantic search client (optional)
    semantic: Option<Arc<SemanticSearch>>,
    /// SQLite database pool (optional, internally Arc-based)
    db: Option<SqlitePool>,
    /// Session manager for file tracking (optional)
    session: Option<Arc<SessionManager>>,
    /// File content cache for avoiding re-reads
    file_cache: FileCache,
}

impl ToolExecutor {
    pub fn new() -> Self {
        Self {
            cwd: std::env::current_dir().unwrap_or_default(),
            semantic: None,
            db: None,
            session: None,
            file_cache: FileCache::new(),
        }
    }

    /// Configure with semantic search
    pub fn with_semantic(mut self, semantic: Arc<SemanticSearch>) -> Self {
        self.semantic = Some(semantic);
        self
    }

    /// Configure with database
    pub fn with_db(mut self, db: SqlitePool) -> Self {
        self.db = Some(db);
        self
    }

    /// Configure with session manager for file tracking
    pub fn with_session(mut self, session: Arc<SessionManager>) -> Self {
        self.session = Some(session);
        self
    }

    /// Execute a tool by name with JSON arguments
    pub async fn execute(&self, name: &str, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;

        match name {
            // File operations
            "read_file" => self.file_tools().read_file(&args).await,
            "write_file" => self.file_tools().write_file(&args).await,
            "edit_file" => self.file_tools().edit_file(&args).await,
            "glob" => self.file_tools().glob(&args).await,
            "grep" => self.file_tools().grep(&args).await,

            // Shell
            "bash" => self.shell_tools().bash(&args).await,

            // Web
            "web_search" => self.web_tools().web_search(&args).await,
            "web_fetch" => self.web_tools().web_fetch(&args).await,

            // Memory
            "remember" => self.memory_tools().remember(&args).await,
            "recall" => self.memory_tools().recall(&args).await,

            // Mira power armor tools
            "task" => self.mira_tools().task(&args).await,
            "goal" => self.mira_tools().goal(&args).await,
            "correction" => self.mira_tools().correction(&args).await,
            "store_decision" => self.mira_tools().store_decision(&args).await,
            "record_rejected_approach" => self.mira_tools().record_rejected_approach(&args).await,

            _ => Ok(format!("Unknown tool: {}", name)),
        }
    }

    /// Execute a tool and return rich result with diff information
    ///
    /// For write_file and edit_file, captures before/after content for diff display.
    /// Other tools return simple output without diff info.
    pub async fn execute_rich(&self, name: &str, arguments: &str) -> Result<RichToolResult> {
        let args: Value = serde_json::from_str(arguments)?;

        match name {
            "write_file" => self.file_tools().write_file_rich(&args).await,
            "edit_file" => self.file_tools().edit_file_rich(&args).await,
            // All other tools don't produce diffs
            _ => {
                let output = self.execute(name, arguments).await?;
                let success = !output.starts_with("Error") && !output.contains("Error:");
                Ok(RichToolResult {
                    success,
                    output,
                    diff: None,
                })
            }
        }
    }

    // Tool group accessors

    fn file_tools(&self) -> FileTools<'_> {
        FileTools {
            cwd: &self.cwd,
            session: &self.session,
            cache: &self.file_cache,
        }
    }

    fn shell_tools(&self) -> ShellTools<'_> {
        ShellTools { cwd: &self.cwd }
    }

    fn web_tools(&self) -> WebTools {
        WebTools
    }

    fn memory_tools(&self) -> MemoryTools<'_> {
        MemoryTools {
            semantic: &self.semantic,
            db: &self.db,
        }
    }

    fn mira_tools(&self) -> MiraTools<'_> {
        MiraTools {
            cwd: &self.cwd,
            semantic: &self.semantic,
            db: &self.db,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[tokio::test]
    async fn test_executor_read_file() {
        let executor = ToolExecutor::new();
        let result = executor
            .execute("read_file", r#"{"path": "Cargo.toml"}"#)
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_edit_file_success() {
        let mut temp = NamedTempFile::new().unwrap();
        writeln!(temp, "Hello world").unwrap();
        let path = temp.path().to_str().unwrap();

        let executor = ToolExecutor::new();
        let args = format!(
            r#"{{"path": "{}", "old_string": "Hello", "new_string": "Goodbye"}}"#,
            path.replace('\\', "\\\\")
        );
        let result = executor.execute("edit_file", &args).await.unwrap();

        assert!(result.contains("Edited"));

        // Verify content changed
        let content = std::fs::read_to_string(path).unwrap();
        assert!(content.contains("Goodbye"));
        assert!(!content.contains("Hello"));
    }

    #[tokio::test]
    async fn test_edit_file_not_found() {
        let mut temp = NamedTempFile::new().unwrap();
        writeln!(temp, "Hello world").unwrap();
        let path = temp.path().to_str().unwrap();

        let executor = ToolExecutor::new();
        let args = format!(
            r#"{{"path": "{}", "old_string": "NotInFile", "new_string": "Replacement"}}"#,
            path.replace('\\', "\\\\")
        );
        let result = executor.execute("edit_file", &args).await.unwrap();

        assert!(result.contains("not found"));
    }

    #[tokio::test]
    async fn test_edit_file_not_unique() {
        let mut temp = NamedTempFile::new().unwrap();
        writeln!(temp, "foo bar foo").unwrap();
        let path = temp.path().to_str().unwrap();

        let executor = ToolExecutor::new();
        let args = format!(
            r#"{{"path": "{}", "old_string": "foo", "new_string": "baz"}}"#,
            path.replace('\\', "\\\\")
        );
        let result = executor.execute("edit_file", &args).await.unwrap();

        assert!(result.contains("2 times"));
    }

    #[tokio::test]
    async fn test_edit_file_replace_all() {
        let mut temp = NamedTempFile::new().unwrap();
        writeln!(temp, "foo bar foo").unwrap();
        let path = temp.path().to_str().unwrap();

        let executor = ToolExecutor::new();
        let args = format!(
            r#"{{"path": "{}", "old_string": "foo", "new_string": "baz", "replace_all": true}}"#,
            path.replace('\\', "\\\\")
        );
        let result = executor.execute("edit_file", &args).await.unwrap();

        assert!(result.contains("Edited"));

        let content = std::fs::read_to_string(path).unwrap();
        assert_eq!(content.matches("baz").count(), 2);
        assert!(!content.contains("foo"));
    }
}
