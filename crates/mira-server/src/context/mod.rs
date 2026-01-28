// crates/mira-server/src/context/mod.rs
// Proactive context injection for Mira

use crate::db::pool::DatabasePool;
use crate::db::{get_or_create_project_sync, get_server_state_sync};
use crate::embeddings::EmbeddingClient;
use std::sync::Arc;

mod analytics;
mod budget;
mod cache;
mod config;
mod file_aware;
mod goal_aware;
mod semantic;

pub use analytics::{InjectionAnalytics, InjectionEvent};
pub use budget::BudgetManager;
pub use cache::InjectionCache;
pub use config::InjectionConfig;
pub use file_aware::FileAwareInjector;
pub use goal_aware::{GoalAwareInjector, TaskAwareInjector};
pub use semantic::SemanticInjector;

/// Result of context injection with metadata for MCP notification
#[derive(Debug, Clone, serde::Serialize)]
pub struct InjectionResult {
    /// The injected context string (empty if nothing injected)
    pub context: String,
    /// Sources that contributed to the context
    pub sources: Vec<InjectionSource>,
    /// Whether injection was skipped and why
    pub skip_reason: Option<String>,
    /// Cache hit?
    pub from_cache: bool,
}

impl InjectionResult {
    #[cfg(test)]
    fn empty() -> Self {
        Self {
            context: String::new(),
            sources: Vec::new(),
            skip_reason: None,
            from_cache: false,
        }
    }

    fn skipped(reason: &str) -> Self {
        Self {
            context: String::new(),
            sources: Vec::new(),
            skip_reason: Some(reason.to_string()),
            from_cache: false,
        }
    }

    /// Check if any context was injected
    pub fn has_context(&self) -> bool {
        !self.context.is_empty()
    }

    /// Format as a notification summary
    pub fn summary(&self) -> String {
        if self.context.is_empty() {
            return String::new();
        }

        let sources: Vec<&str> = self.sources.iter().map(|s| s.name()).collect();
        format!(
            "Injected {} chars from: {}{}",
            self.context.len(),
            sources.join(", "),
            if self.from_cache { " (cached)" } else { "" }
        )
    }
}

/// Source of injected context
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum InjectionSource {
    Semantic,
    FileAware,
    TaskAware,
}

impl InjectionSource {
    fn name(&self) -> &'static str {
        match self {
            Self::Semantic => "semantic",
            Self::FileAware => "files",
            Self::TaskAware => "tasks",
        }
    }
}

/// Main context injection manager
pub struct ContextInjectionManager {
    pool: Arc<DatabasePool>,
    semantic_injector: SemanticInjector,
    file_injector: FileAwareInjector,
    task_injector: TaskAwareInjector,
    budget_manager: BudgetManager,
    cache: InjectionCache,
    analytics: InjectionAnalytics,
    config: InjectionConfig,
}

impl ContextInjectionManager {
    pub async fn new(pool: Arc<DatabasePool>, embeddings: Option<Arc<EmbeddingClient>>) -> Self {
        // Load config from database
        let config = InjectionConfig::load(&pool).await.unwrap_or_default();

        Self {
            pool: pool.clone(),
            semantic_injector: SemanticInjector::new(pool.clone(), embeddings),
            file_injector: FileAwareInjector::new(pool.clone()),
            task_injector: TaskAwareInjector::new(pool.clone()),
            budget_manager: BudgetManager::with_limit(config.max_chars),
            cache: InjectionCache::new(),
            analytics: InjectionAnalytics::new(pool.clone()),
            config,
        }
    }

    /// Get current configuration
    pub fn config(&self) -> &InjectionConfig {
        &self.config
    }

    /// Update configuration
    pub async fn set_config(&mut self, config: InjectionConfig) {
        if let Err(e) = config.save(&self.pool).await {
            tracing::warn!("Failed to save injection config: {}", e);
        }
        self.budget_manager = BudgetManager::with_limit(config.max_chars);
        self.config = config;
    }

    /// Get project ID and path for the current session (if any)
    async fn get_project_info(&self) -> (Option<i64>, Option<String>) {
        let pool = self.pool.clone();
        pool.interact(move |conn| {
                // Get last active project path from server state
                let path = get_server_state_sync(conn, "active_project_path")
                    .map_err(|e| anyhow::anyhow!("{}", e))?;

                if let Some(path) = path {
                    let (id, _name) = get_or_create_project_sync(conn, &path, None)
                        .map_err(|e| anyhow::anyhow!("{}", e))?;
                    Ok((Some(id), Some(path)))
                } else {
                    Ok((None, None))
                }
            })
            .await
            .unwrap_or_default()
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
        if trimmed.contains('/')
            && (trimmed.ends_with(".rs")
                || trimmed.ends_with(".toml")
                || trimmed.ends_with(".json")
                || trimmed.ends_with(".md")
                || trimmed.ends_with(".txt"))
        {
            return true;
        }

        let lower = trimmed.to_lowercase();

        // Common command prefixes that don't need context
        let simple_prefixes = [
            "git", "cargo", "ls", "cd", "pwd", "echo", "cat", "rm", "mkdir", "touch", "mv", "cp",
            "npm", "yarn", "docker", "kubectl", "ps", "grep", "find", "which",
        ];
        if simple_prefixes
            .iter()
            .any(|&prefix| lower.starts_with(prefix))
        {
            return true;
        }

        // Questions about Claude Code itself (not about the codebase)
        let claude_questions = [
            "how do i use claude code",
            "can claude code",
            "does claude code",
            "what is claude code",
            "where is claude code",
        ];
        if claude_questions.iter().any(|&q| lower.contains(q)) {
            return true;
        }

        false
    }

    /// Check if message is code-related
    fn is_code_related(&self, message: &str) -> bool {
        let lower = message.to_lowercase();
        let code_keywords = [
            // Code structure
            "function",
            "struct",
            "class",
            "module",
            "import",
            "export",
            "variable",
            "constant",
            "type",
            "interface",
            "trait",
            "impl",
            "def",
            "fn",
            "method",
            "property",
            "attribute",
            "enum",
            "const",
            "let",
            "var",
            "field",
            "member",
            // Questions
            "where is",
            "how does",
            "show me",
            "what is",
            "explain",
            "find",
            "search",
            "look for",
            "locate",
            "where are",
            "how to",
            "help with",
            // Implementation
            "implement",
            "refactor",
            "fix",
            "bug",
            "error",
            "issue",
            "problem",
            "debug",
            "test",
            "optimize",
            "performance",
            "memory",
            "concurrent",
            "async",
            "thread",
            "parallel",
            // Codebase concepts
            "api",
            "endpoint",
            "route",
            "handler",
            "controller",
            "service",
            "repository",
            "dao",
            "middleware",
            "auth",
            "authentication",
            "authorization",
            "database",
            "db",
            "query",
            "schema",
            "migration",
            "config",
            "configuration",
            "setting",
            "environment",
        ];

        let has_code_keyword = code_keywords.iter().any(|&kw| lower.contains(kw));

        let has_file_mention = lower.contains(".rs")
            || lower.contains(".toml")
            || lower.contains(".json")
            || lower.contains(".md")
            || lower.contains(".txt")
            || lower.contains(".py")
            || lower.contains(".js")
            || lower.contains(".ts")
            || (lower.contains('/') && (lower.contains("src/") || lower.contains("crates/")));

        has_code_keyword || has_file_mention
    }

    /// Main entry point for proactive context injection
    /// Returns both the context string and metadata about what was injected
    pub async fn get_context_for_message(
        &self,
        user_message: &str,
        session_id: &str,
    ) -> InjectionResult {
        // Check if injection is enabled
        if !self.config.enabled {
            return InjectionResult::skipped("disabled");
        }

        // Skip injection for simple commands
        if self.is_simple_command(user_message) {
            return InjectionResult::skipped("simple_command");
        }

        // Skip very short or very long messages
        let msg_len = user_message.trim().len();
        if msg_len < self.config.min_message_len {
            return InjectionResult::skipped("too_short");
        }
        if msg_len > self.config.max_message_len {
            return InjectionResult::skipped("too_long");
        }

        // Probabilistic skip based on sample rate
        if self.config.sample_rate < 1.0 {
            let hash = user_message
                .bytes()
                .fold(0u32, |acc, b| acc.wrapping_add(b as u32));
            let threshold = (self.config.sample_rate * 100.0) as u32;
            if hash % 100 >= threshold {
                return InjectionResult::skipped("sampled_out");
            }
        }

        // Check if message is code-related
        if !self.is_code_related(user_message) {
            return InjectionResult::skipped("not_code_related");
        }

        // Check cache first
        if let Some(cached) = self.cache.get(user_message).await {
            return InjectionResult {
                context: cached,
                sources: vec![], // We don't track sources for cached results
                skip_reason: None,
                from_cache: true,
            };
        }

        // Get project info for scoping search
        let (project_id, project_path) = self.get_project_info().await;

        // Collect context from different injectors
        let mut contexts = Vec::new();
        let mut sources = Vec::new();

        // Semantic context
        if self.config.enable_semantic {
            let semantic_context = self
                .semantic_injector
                .inject_context(
                    user_message,
                    session_id,
                    project_id,
                    project_path.as_deref(),
                )
                .await;
            if !semantic_context.is_empty() {
                contexts.push(semantic_context);
                sources.push(InjectionSource::Semantic);
            }
        }

        // File mention context
        if self.config.enable_file_aware {
            let file_paths = self.file_injector.extract_file_mentions(user_message);
            if !file_paths.is_empty() {
                let file_context = self.file_injector.inject_file_context(file_paths).await;
                if !file_context.is_empty() {
                    contexts.push(file_context);
                    sources.push(InjectionSource::FileAware);
                }
            }
        }

        // Task context
        if self.config.enable_task_aware {
            let task_ids = self.task_injector.get_active_task_ids().await;
            if !task_ids.is_empty() {
                let task_context = self.task_injector.inject_task_context(task_ids).await;
                if !task_context.is_empty() {
                    contexts.push(task_context);
                    sources.push(InjectionSource::TaskAware);
                }
            }
        }

        // Apply budget management
        let final_context = self.budget_manager.apply_budget(contexts);

        // Cache the result
        self.cache.put(user_message, final_context.clone()).await;

        // Record analytics
        if !final_context.is_empty() {
            self.analytics
                .record(InjectionEvent {
                    session_id: session_id.to_string(),
                    project_id,
                    sources: sources.clone(),
                    context_len: final_context.len(),
                    message_preview: user_message.chars().take(50).collect(),
                })
                .await;
        }

        InjectionResult {
            context: final_context,
            sources,
            skip_reason: None,
            from_cache: false,
        }
    }

    /// Legacy method for backwards compatibility - returns just the context string
    pub async fn get_context_string(&self, user_message: &str, session_id: &str) -> String {
        self.get_context_for_message(user_message, session_id)
            .await
            .context
    }

    /// Get injection analytics summary
    pub async fn get_analytics_summary(&self, project_id: Option<i64>) -> String {
        self.analytics.summary(project_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_injection_result_summary() {
        let result = InjectionResult {
            context: "Some context here".to_string(),
            sources: vec![InjectionSource::Semantic, InjectionSource::TaskAware],
            skip_reason: None,
            from_cache: false,
        };

        let summary = result.summary();
        assert!(summary.contains("17 chars"));
        assert!(summary.contains("semantic"));
        assert!(summary.contains("tasks"));
    }

    #[test]
    fn test_injection_result_empty() {
        let result = InjectionResult::empty();
        assert!(!result.has_context());
        assert!(result.summary().is_empty());
    }

    #[test]
    fn test_injection_result_skipped() {
        let result = InjectionResult::skipped("too_short");
        assert!(!result.has_context());
        assert_eq!(result.skip_reason, Some("too_short".to_string()));
    }
}
