//! Tool definitions and executor for Gemini 3 Pro (Orchestrator mode)
//!
//! Studio is an orchestrator - it doesn't write code, Claude Code does.
//! Tools here are read-only file ops + management/intelligence tools:
//! - File operations (read, glob, grep) - READ ONLY
//! - Memory (remember, recall)
//! - Mira power armor (task, goal, correction, store_decision, record_rejected_approach)
//! - Council (consult other AI models)
//! - Code/Git intelligence
//!
//! REMOVED (Claude Code handles these via MCP):
//! - write_file, edit_file, bash, run_tests, git_commit
//!
//! REMOVED (replaced by Gemini built-in tools):
//! - web_search, web_fetch -> google_search, code_execution, url_context

pub mod build;
pub mod code_intel;
mod council;
mod definitions;
pub mod documents;
mod file;
mod git;
pub mod git_intel;
pub mod index;
mod memory;
mod mira;
mod orchestration;
pub mod proactive;
mod tool_defs;
pub mod types;

use anyhow::Result;
use serde_json::Value;
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

pub use definitions::get_tools;
pub use types::{DiffInfo, RichToolResult};

use crate::chat::provider::ToolDefinition;
use crate::chat::server::types::ToolCategory;

/// Get the category for a tool (for UI filtering)
pub fn tool_category(name: &str) -> ToolCategory {
    match name {
        // File operations (read-only)
        "read_file" | "glob" | "grep" | "list_files" | "search" => {
            ToolCategory::File
        }
        // Memory operations
        "remember" | "recall" => ToolCategory::Memory,
        // Git operations (read-only)
        "git_status" | "git_diff" | "git_log" | "get_recent_commits"
        | "search_commits" | "find_cochange_patterns" => ToolCategory::Git,
        // Mira power armor + council + intelligence + orchestration
        "task" | "goal" | "correction" | "store_decision" | "record_rejected_approach"
        | "get_symbols" | "get_call_graph" | "semantic_code_search" | "get_related_files"
        | "get_codebase_style" | "find_similar_fixes" | "record_error_fix" | "build"
        | "document" | "index" | "get_proactive_context" | "council" | "ask_gpt" | "ask_opus"
        | "ask_gemini" | "ask_deepseek"
        | "view_claude_activity" | "send_instruction" | "list_instructions" | "cancel_instruction"
        => ToolCategory::Mira,
        // Artifact tools
        "fetch_artifact" | "search_artifact" => ToolCategory::Other,
        // Default
        _ => ToolCategory::Other,
    }
}

/// Generate a human-readable summary for a tool call
pub fn tool_summary(name: &str, args: &Value) -> String {
    fn truncate(s: &str, max_len: usize) -> String {
        if s.len() <= max_len {
            s.to_string()
        } else {
            format!("{}...", &s[..max_len.saturating_sub(3)])
        }
    }

    fn get_str<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
        args.get(key).and_then(|v| v.as_str())
    }

    match name {
        // File operations (read-only)
        "read_file" => {
            let path = get_str(args, "path").unwrap_or("file");
            format!("Reading {}", truncate(path, 50))
        }
        "glob" => {
            let pattern = get_str(args, "pattern").unwrap_or("*");
            format!("Finding {}", truncate(pattern, 40))
        }
        "grep" => {
            let pattern = get_str(args, "pattern").unwrap_or("");
            format!("Searching for \"{}\"", truncate(pattern, 30))
        }

        // Memory
        "remember" => {
            let content = get_str(args, "content").unwrap_or("");
            format!("Storing: {}", truncate(content, 40))
        }
        "recall" => {
            let query = get_str(args, "query").unwrap_or("");
            format!("Recalling \"{}\"", truncate(query, 40))
        }

        // Git (read-only)
        "git_status" => "Checking git status".to_string(),
        "git_diff" => {
            let path = get_str(args, "path");
            match path {
                Some(p) => format!("Git diff: {}", truncate(p, 40)),
                None => "Git diff".to_string(),
            }
        }
        "git_log" => "Git log".to_string(),

        // Mira tools
        "task" => {
            let action = get_str(args, "action").unwrap_or("manage");
            format!("Task: {}", action)
        }
        "goal" => {
            let action = get_str(args, "action").unwrap_or("manage");
            format!("Goal: {}", action)
        }
        "council" => "Consulting council".to_string(),
        "ask_gpt" => "Asking GPT-5.2".to_string(),
        "ask_opus" => "Asking Opus 4.5".to_string(),
        "ask_gemini" => "Asking Gemini 3 Pro".to_string(),
        "ask_deepseek" => "Asking DeepSeek Reasoner".to_string(),

        // Code intelligence
        "get_symbols" => {
            let path = get_str(args, "file_path").unwrap_or("file");
            format!("Getting symbols: {}", truncate(path, 40))
        }
        "semantic_code_search" => {
            let query = get_str(args, "query").unwrap_or("");
            format!("Code search: {}", truncate(query, 40))
        }

        // Orchestration tools
        "view_claude_activity" => "Viewing Claude Code activity".to_string(),
        "send_instruction" => {
            let instruction = get_str(args, "instruction").unwrap_or("");
            format!("Sending instruction: {}", truncate(instruction, 40))
        }
        "list_instructions" => "Listing instruction queue".to_string(),
        "cancel_instruction" => {
            let id = get_str(args, "instruction_id").unwrap_or("");
            format!("Cancelling instruction: {}", id)
        }

        // Default: just show tool name
        _ => name.to_string(),
    }
}

/// Get tool definitions in Provider-compatible format (for DeepSeek, etc.)
pub fn get_tool_definitions() -> Vec<ToolDefinition> {
    get_tools().into_iter().map(|t| ToolDefinition {
        name: t.name,
        description: t.description.unwrap_or_default(),
        parameters: t.parameters,
    }).collect()
}

use crate::core::{ArtifactStore, SemanticSearch};
use crate::chat::session::SessionManager;

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

use build::BuildTools;
use code_intel::CodeIntelTools;
use council::CouncilTools;
use documents::DocumentTools;
use file::FileTools;
use git::GitTools;
use git_intel::GitIntelTools;
use index::IndexTools;
use memory::MemoryTools;
use mira::MiraTools;
use orchestration::OrchestrationTools;
use proactive::ProactiveTools;

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

impl Default for ToolExecutor {
    fn default() -> Self {
        Self::new()
    }
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
    ///
    /// NOTE: Orchestrator mode - write/edit/shell/test tools have been removed.
    /// Claude Code handles those via MCP.
    pub async fn execute(&self, name: &str, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;

        match name {
            // File operations (read-only)
            "read_file" => self.file_tools().read_file(&args).await,
            "glob" => self.file_tools().glob(&args).await,
            "grep" => self.file_tools().grep(&args).await,

            // Memory
            "remember" => self.memory_tools().remember(&args).await,
            "recall" => self.memory_tools().recall(&args).await,

            // Mira power armor tools
            "task" => self.mira_tools().task(&args).await,
            "goal" => self.mira_tools().goal(&args).await,
            "correction" => self.mira_tools().correction(&args).await,
            "store_decision" => self.mira_tools().store_decision(&args).await,
            "record_rejected_approach" => self.mira_tools().record_rejected_approach(&args).await,

            // Git tools (read-only)
            "git_status" => self.git_tools().git_status(&args).await,
            "git_diff" => self.git_tools().git_diff(&args).await,
            "git_log" => self.git_tools().git_log(&args).await,

            // Artifact tools
            "fetch_artifact" => self.fetch_artifact(&args).await,
            "search_artifact" => self.search_artifact(&args).await,

            // Council tools - consult other AI models
            "council" => {
                let message = args.get("message").and_then(|v| v.as_str()).unwrap_or("");
                let context = args.get("context").and_then(|v| v.as_str());
                CouncilTools::council(message, context).await
            }
            "ask_gpt" => {
                let message = args.get("message").and_then(|v| v.as_str()).unwrap_or("");
                let context = args.get("context").and_then(|v| v.as_str());
                CouncilTools::ask_gpt(message, context).await
            }
            "ask_opus" => {
                let message = args.get("message").and_then(|v| v.as_str()).unwrap_or("");
                let context = args.get("context").and_then(|v| v.as_str());
                CouncilTools::ask_opus(message, context).await
            }
            "ask_gemini" => {
                let message = args.get("message").and_then(|v| v.as_str()).unwrap_or("");
                let context = args.get("context").and_then(|v| v.as_str());
                CouncilTools::ask_gemini(message, context).await
            }
            "ask_deepseek" => {
                let message = args.get("message").and_then(|v| v.as_str()).unwrap_or("");
                let context = args.get("context").and_then(|v| v.as_str());
                CouncilTools::ask_deepseek(message, context).await
            }

            // Code intelligence tools
            "get_symbols" => self.code_intel_tools().get_symbols(&args).await,
            "get_call_graph" => self.code_intel_tools().get_call_graph(&args).await,
            "semantic_code_search" => self.code_intel_tools().semantic_code_search(&args).await,
            "get_related_files" => self.code_intel_tools().get_related_files(&args).await,
            "get_codebase_style" => self.code_intel_tools().get_codebase_style(&args).await,

            // Git intelligence tools
            "get_recent_commits" => self.git_intel_tools().get_recent_commits(&args).await,
            "search_commits" => self.git_intel_tools().search_commits(&args).await,
            "find_cochange_patterns" => self.git_intel_tools().find_cochange_patterns(&args).await,
            "find_similar_fixes" => self.git_intel_tools().find_similar_fixes(&args).await,
            "record_error_fix" => self.git_intel_tools().record_error_fix(&args).await,

            // Build tracking tools
            "build" => self.build_tools().build(&args).await,

            // Document tools
            "document" => self.document_tools().document(&args).await,

            // Index tools
            "index" => self.index_tools().index(&args).await,

            // Proactive context
            "get_proactive_context" => self.proactive_tools().get_proactive_context(&args).await,

            // Orchestration tools (Studio -> Claude Code)
            "view_claude_activity" => self.orchestration_tools().view_claude_activity(&args).await,
            "send_instruction" => self.orchestration_tools().send_instruction(&args).await,
            "list_instructions" => self.orchestration_tools().list_instructions(&args).await,
            "cancel_instruction" => self.orchestration_tools().cancel_instruction(&args).await,

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

    /// Execute a tool and return rich result
    ///
    /// Orchestrator mode - no write/edit tools, so no diff info.
    /// Large outputs are automatically stored as artifacts with a preview returned.
    pub async fn execute_rich(&self, name: &str, arguments: &str) -> Result<RichToolResult> {
        let output = self.execute(name, arguments).await?;
        // Check for error at the start only - content may contain "Error" strings
        // (e.g., search results discussing error handling)
        let success = !output.starts_with("Error") && !output.starts_with("error:");

        // Check if output should be artifacted
        let final_output = self.maybe_artifact(name, &output).await;

        Ok(RichToolResult {
            success,
            output: final_output,
            diff: None,
        })
    }

    /// Conditionally store large output as artifact and return preview
    async fn maybe_artifact(&self, tool_name: &str, output: &str) -> String {
        use crate::core::{ArtifactStore, ARTIFACT_THRESHOLD_BYTES};

        // Skip if no database or output is small enough
        let Some(db) = &self.db else {
            return output.to_string();
        };

        if output.len() <= ARTIFACT_THRESHOLD_BYTES {
            return output.to_string();
        }

        // Only artifact certain tools (read-only tools with potentially large output)
        let artifact_tools = ["grep", "read_file", "git_diff", "git_log"];
        if !artifact_tools.iter().any(|t| tool_name.contains(t)) {
            return output.to_string();
        }

        // Store artifact
        let store = ArtifactStore::new(db.clone(), self.project_path.clone());
        let decision = store.decide(tool_name, output);

        if !decision.should_artifact {
            return output.to_string();
        }

        // Store (with dedupe) and return preview with artifact ID
        match store.store_deduped(
            "tool_output",
            Some(tool_name),
            None, // tool_call_id not available here
            output,
            decision.contains_secrets,
            decision.secret_kind.as_deref(),
        ).await {
            Ok((artifact_id, was_dedupe)) => {
                let dedupe_note = if was_dedupe { " (cached)" } else { "" };
                format!(
                    "{}\n\n📦 [artifact_id: {}{} | {} bytes total | use fetch_artifact/search_artifact for more]",
                    decision.preview,
                    artifact_id,
                    dedupe_note,
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

    fn code_intel_tools(&self) -> CodeIntelTools<'_> {
        CodeIntelTools {
            cwd: &self.cwd,
            db: &self.db,
            semantic: &self.semantic,
        }
    }

    fn git_intel_tools(&self) -> GitIntelTools<'_> {
        GitIntelTools {
            cwd: &self.cwd,
            db: &self.db,
            semantic: &self.semantic,
        }
    }

    fn build_tools(&self) -> BuildTools<'_> {
        BuildTools {
            cwd: &self.cwd,
            db: &self.db,
        }
    }

    fn document_tools(&self) -> DocumentTools<'_> {
        DocumentTools {
            cwd: &self.cwd,
            db: &self.db,
            semantic: &self.semantic,
        }
    }

    fn index_tools(&self) -> IndexTools<'_> {
        IndexTools {
            cwd: &self.cwd,
            db: &self.db,
            semantic: &self.semantic,
        }
    }

    fn proactive_tools(&self) -> ProactiveTools<'_> {
        ProactiveTools {
            cwd: &self.cwd,
            db: &self.db,
            semantic: &self.semantic,
        }
    }

    fn orchestration_tools(&self) -> OrchestrationTools<'_> {
        OrchestrationTools {
            db: &self.db,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_executor_read_file() {
        let executor = ToolExecutor::new();
        let result = executor
            .execute("read_file", r#"{"path": "Cargo.toml"}"#)
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_executor_glob() {
        let executor = ToolExecutor::new();
        let result = executor
            .execute("glob", r#"{"pattern": "*.toml"}"#)
            .await;
        assert!(result.is_ok());
        assert!(result.unwrap().contains("Cargo.toml"));
    }

    #[tokio::test]
    async fn test_removed_tools_return_unknown() {
        let executor = ToolExecutor::new();

        // These tools were removed in orchestrator mode
        let removed = ["write_file", "edit_file", "bash", "run_tests", "git_commit"];
        for tool in removed {
            let result = executor.execute(tool, "{}").await.unwrap();
            assert!(result.contains("Unknown tool"), "Tool {} should be unknown", tool);
        }
    }
}
