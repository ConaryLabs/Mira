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
mod git;
mod memory;
mod mira;
mod shell;
mod test;
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

use crate::artifacts::ArtifactStore;
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
use git::GitTools;
use memory::MemoryTools;
use mira::MiraTools;
use shell::ShellTools;
use test::TestTools;
use web::WebTools;

/// Tool executor handles tool invocation and result formatting
///
/// Clone is cheap - uses Arc for shared state.
#[derive(Clone)]
pub struct ToolExecutor {
    /// Working directory for file operations
    pub cwd: std::path::PathBuf,
    /// Project path for artifact storage
    pub project_path: String,
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
        let cwd = std::env::current_dir().unwrap_or_default();
        Self {
            project_path: cwd.to_string_lossy().to_string(),
            cwd,
            semantic: None,
            db: None,
            session: None,
            file_cache: FileCache::new(),
        }
    }

    /// Configure with project path
    pub fn with_project_path(mut self, project_path: String) -> Self {
        self.project_path = project_path;
        self
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

            // Git tools
            "git_status" => self.git_tools().git_status(&args).await,
            "git_diff" => self.git_tools().git_diff(&args).await,
            "git_commit" => self.git_tools().git_commit(&args).await,
            "git_log" => self.git_tools().git_log(&args).await,

            // Test tools
            "run_tests" => self.test_tools().run_tests(&args).await,

            // Artifact tools
            "fetch_artifact" => self.fetch_artifact(&args).await,
            "search_artifact" => self.search_artifact(&args).await,

            _ => Ok(format!("Unknown tool: {}", name)),
        }
    }

    /// Fetch a slice of an artifact
    async fn fetch_artifact(&self, args: &Value) -> Result<String> {
        let Some(db) = &self.db else {
            return Ok("Error: Artifacts unavailable (database disabled)".to_string());
        };

        let artifact_id = args.get("artifact_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("artifact_id required"))?;

        let offset = args.get("offset").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(8192) as usize;

        let store = ArtifactStore::new(db.clone(), self.project_path.clone());
        match store.fetch(artifact_id, offset, limit).await? {
            Some(result) => Ok(serde_json::to_string_pretty(&result)?),
            None => Ok(format!("Error: Artifact not found: {}", artifact_id)),
        }
    }

    /// Search within an artifact
    async fn search_artifact(&self, args: &Value) -> Result<String> {
        let Some(db) = &self.db else {
            return Ok("Error: Artifacts unavailable (database disabled)".to_string());
        };

        let artifact_id = args.get("artifact_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("artifact_id required"))?;

        let query = args.get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("query required"))?;

        let max_results = args.get("max_results").and_then(|v| v.as_u64()).unwrap_or(5) as usize;
        let context_bytes = args.get("context_bytes").and_then(|v| v.as_u64()).unwrap_or(200) as usize;

        let store = ArtifactStore::new(db.clone(), self.project_path.clone());
        match store.search(artifact_id, query, max_results, context_bytes).await? {
            Some(result) => Ok(serde_json::to_string_pretty(&result)?),
            None => Ok(format!("Error: Artifact not found: {}", artifact_id)),
        }
    }

    /// Execute a tool and return rich result with diff information
    ///
    /// For write_file and edit_file, captures before/after content for diff display.
    /// Other tools return simple output without diff info.
    /// Large outputs are automatically stored as artifacts with a preview returned.
    pub async fn execute_rich(&self, name: &str, arguments: &str) -> Result<RichToolResult> {
        let args: Value = serde_json::from_str(arguments)?;

        match name {
            "write_file" => self.file_tools().write_file_rich(&args).await,
            "edit_file" => self.file_tools().edit_file_rich(&args).await,
            // All other tools don't produce diffs
            _ => {
                let output = self.execute(name, arguments).await?;
                let success = !output.starts_with("Error") && !output.contains("Error:");

                // Check if output should be artifacted
                let final_output = self.maybe_artifact(name, &output).await;

                Ok(RichToolResult {
                    success,
                    output: final_output,
                    diff: None,
                })
            }
        }
    }

    /// Conditionally store large output as artifact and return preview
    async fn maybe_artifact(&self, tool_name: &str, output: &str) -> String {
        use crate::artifacts::{ArtifactStore, ARTIFACT_THRESHOLD_BYTES};

        // Skip if no database or output is small enough
        let Some(db) = &self.db else {
            return output.to_string();
        };

        if output.len() <= ARTIFACT_THRESHOLD_BYTES {
            return output.to_string();
        }

        // Only artifact certain tools
        let artifact_tools = ["bash", "grep", "read_file", "git_diff", "git_log", "run_tests"];
        if !artifact_tools.iter().any(|t| tool_name.contains(t)) {
            return output.to_string();
        }

        // Store artifact
        let store = ArtifactStore::new(db.clone(), self.project_path.clone());
        let decision = store.decide(tool_name, output);

        if !decision.should_artifact {
            return output.to_string();
        }

        // Store and return preview with artifact ID
        match store.store(
            "tool_output",
            Some(tool_name),
            None, // tool_call_id not available here
            output,
            decision.contains_secrets,
            decision.secret_reason.as_deref(),
        ).await {
            Ok(artifact_id) => {
                format!(
                    "{}\n\n📦 [artifact_id: {} | {} bytes total | use fetch_artifact/search_artifact for more]",
                    decision.preview,
                    artifact_id,
                    decision.total_bytes
                )
            }
            Err(e) => {
                tracing::warn!("Failed to store artifact: {}", e);
                output.to_string()
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

    fn git_tools(&self) -> GitTools<'_> {
        GitTools { cwd: &self.cwd }
    }

    fn test_tools(&self) -> TestTools<'_> {
        TestTools { cwd: &self.cwd }
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
