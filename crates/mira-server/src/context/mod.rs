// crates/mira-server/src/context/mod.rs
// Proactive context injection for Mira

use std::sync::Arc;
use crate::db::Database;
use crate::embeddings::Embeddings;

mod semantic;
mod file_aware;
mod task_aware;
mod budget;
mod cache;

pub use semantic::SemanticInjector;
pub use file_aware::FileAwareInjector;
pub use task_aware::TaskAwareInjector;
pub use budget::BudgetManager;
pub use cache::InjectionCache;

// Context injector trait for proactive context injection (future expansion)
// #[async_trait::async_trait]
// pub trait ContextInjector {
//     /// Inject relevant context based on user message
//     /// Returns a string of context to be injected into the conversation
//     async fn inject_context(&self, user_message: &str, session_id: &str) -> String;
//
//     /// Inject context related to specific file paths mentioned in the message
//     async fn inject_file_context(&self, file_paths: Vec<&str>) -> String;
//
//     /// Inject context related to active tasks
//     async fn inject_task_context(&self, task_ids: Vec<i64>) -> String;
// }

/// Main context injection manager
pub struct ContextInjectionManager {
    db: Arc<Database>,
    semantic_injector: SemanticInjector,
    file_injector: FileAwareInjector,
    task_injector: TaskAwareInjector,
    budget_manager: BudgetManager,
    cache: InjectionCache,
}

impl ContextInjectionManager {
    pub fn new(db: Arc<Database>, embeddings: Option<Arc<Embeddings>>) -> Self {
        Self {
            db: db.clone(),
            semantic_injector: SemanticInjector::new(db.clone(), embeddings),
            file_injector: FileAwareInjector::new(db.clone()),
            task_injector: TaskAwareInjector::new(db.clone()),
            budget_manager: BudgetManager::new(),
            cache: InjectionCache::new(),
        }
    }

    /// Get project ID and path for the current session (if any)
    async fn get_project_info(&self) -> (Option<i64>, Option<String>) {
        // Try to get last active project from database
        match self.db.get_last_active_project() {
            Ok(Some(path)) => {
                match self.db.get_or_create_project(&path, None) {
                    Ok((id, _name)) => (Some(id), Some(path)),
                    Err(_) => (None, None),
                }
            }
            _ => (None, None),
        }
    }

    /// Check if message is a simple command that doesn't need context injection
    fn is_simple_command(&self, message: &str) -> bool {
        let trimmed = message.trim();

        // Very short messages (1-2 words)
        let word_count = trimmed.split_whitespace().count();
        if word_count <= 2 {
            return true;
        }

        // Slash commands (Claude Code commands)
        if trimmed.starts_with('/') {
            return true;
        }

        // URLs
        if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
            return true;
        }

        // File paths (absolute or relative with extensions)
        if trimmed.contains('/') && (trimmed.ends_with(".rs") || trimmed.ends_with(".toml") ||
            trimmed.ends_with(".json") || trimmed.ends_with(".md") || trimmed.ends_with(".txt")) {
            return true;
        }

        let lower = trimmed.to_lowercase();

        // Common command prefixes that don't need context
        let simple_prefixes = ["git", "cargo", "ls", "cd", "pwd", "echo", "cat", "rm", "mkdir",
            "touch", "mv", "cp", "npm", "yarn", "docker", "kubectl", "ps", "grep", "find", "which"];
        if simple_prefixes.iter().any(|&prefix| lower.starts_with(prefix)) {
            return true;
        }

        // Questions about Claude Code itself (not about the codebase)
        let claude_questions = ["how do i use claude code", "can claude code", "does claude code",
            "what is claude code", "where is claude code"];
        if claude_questions.iter().any(|&q| lower.contains(q)) {
            return true;
        }

        false
    }


    /// Main entry point for proactive context injection
    pub async fn get_context_for_message(&self, user_message: &str, session_id: &str) -> String {
        // Skip injection for simple commands to avoid token overflow
        if self.is_simple_command(user_message) {
            return String::new();
        }

        // Skip very short messages (< 30 chars) and very long messages (> 500 chars, likely code paste)
        let msg_len = user_message.trim().len();
        if msg_len < 30 || msg_len > 500 {
            return String::new();
        }

        // Skip injection 50% of the time deterministically based on message hash
        let hash = user_message.bytes().fold(0u32, |acc, b| acc.wrapping_add(b as u32));
        if hash % 2 != 0 { // 50% chance to proceed
            return String::new();
        }

        // Only inject for messages related to code or the project
        let lower = user_message.to_lowercase();
        let code_keywords = [
            // Code structure
            "function", "struct", "class", "module", "import", "export", "variable", "constant",
            "type", "interface", "trait", "impl", "def", "fn", "method", "property", "attribute",
            "enum", "const", "let", "var", "field", "member",
            // Questions
            "where is", "how does", "show me", "what is", "explain", "find", "search", "look for",
            "locate", "where are", "how to", "help with",
            // Implementation
            "implement", "refactor", "fix", "bug", "error", "issue", "problem", "debug", "test",
            "optimize", "performance", "memory", "concurrent", "async", "thread", "parallel",
            // Codebase concepts
            "api", "endpoint", "route", "handler", "controller", "service", "repository", "dao",
            "middleware", "auth", "authentication", "authorization", "database", "db", "query",
            "schema", "migration", "config", "configuration", "setting", "environment",
        ];

        // Check if message contains any code-related keyword
        let has_code_keyword = code_keywords.iter().any(|&kw| lower.contains(kw));

        // Also check for file extensions or paths mentioned
        let has_file_mention = lower.contains(".rs") || lower.contains(".toml") ||
            lower.contains(".json") || lower.contains(".md") || lower.contains(".txt") ||
            lower.contains(".py") || lower.contains(".js") || lower.contains(".ts") ||
            (lower.contains('/') && (lower.contains("src/") || lower.contains("crates/")));

        if !has_code_keyword && !has_file_mention {
            return String::new();
        }

        // Check cache first
        if let Some(cached) = self.cache.get(user_message).await {
            return cached;
        }

        // Get project info for scoping search
        let (project_id, project_path) = self.get_project_info().await;

        // Collect context from different injectors
        let mut contexts = Vec::new();

        // Semantic context
        let semantic_context = self.semantic_injector.inject_context(
            user_message,
            session_id,
            project_id,
            project_path.as_deref(),
        ).await;
        if !semantic_context.is_empty() {
            contexts.push(semantic_context);
        }

        // File mention context
        let file_paths = self.file_injector.extract_file_mentions(user_message);
        if !file_paths.is_empty() {
            let file_context = self.file_injector.inject_file_context(file_paths).await;
            if !file_context.is_empty() {
                contexts.push(file_context);
            }
        }

        // Task context (if any active tasks)
        let task_ids = self.task_injector.get_active_task_ids().await;
        if !task_ids.is_empty() {
            let task_context = self.task_injector.inject_task_context(task_ids).await;
            if !task_context.is_empty() {
                contexts.push(task_context);
            }
        }

        // Apply budget management
        let final_context = self.budget_manager.apply_budget(contexts);

        // Cache the result
        self.cache.put(user_message, final_context.clone()).await;

        final_context
    }
}