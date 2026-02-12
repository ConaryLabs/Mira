// crates/mira-server/src/context/mod.rs
// Proactive context injection for Mira

use crate::db::pool::DatabasePool;
use crate::db::{get_or_create_project_sync, get_server_state_sync};
use crate::embeddings::EmbeddingClient;
use crate::fuzzy::FuzzyCache;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

mod analytics;
mod budget;
mod cache;
mod config;
mod convention;
mod file_aware;
mod goal_aware;
mod semantic;
mod working_context;

pub use analytics::{InjectionAnalytics, InjectionEvent, extract_key_terms};
pub use budget::{
    BudgetEntry, BudgetManager, PRIORITY_CONVENTION, PRIORITY_FILE_AWARE, PRIORITY_GOALS,
    PRIORITY_MEMORY, PRIORITY_PROACTIVE, PRIORITY_REACTIVE, PRIORITY_SEMANTIC, PRIORITY_TASKS,
    PRIORITY_TEAM,
};
pub use cache::InjectionCache;
pub use config::InjectionConfig;
pub use convention::ConventionInjector;
pub use file_aware::FileAwareInjector;
pub use goal_aware::GoalAwareInjector;
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
    Convention,
}

impl InjectionSource {
    fn name(&self) -> &'static str {
        match self {
            Self::Semantic => "semantic",
            Self::FileAware => "files",
            Self::TaskAware => "tasks",
            Self::Convention => "convention",
        }
    }
}

/// Check if message is a simple command that doesn't need context injection.
/// Used by both reactive injection (ContextInjectionManager) and proactive
/// gating (UserPromptSubmit hook) to ensure consistent filtering.
pub fn is_simple_command(message: &str) -> bool {
    let trimmed = message.trim();

    // Very short messages (1 word) that aren't code-related
    let word_count = trimmed.split_whitespace().count();
    if word_count <= 1 {
        return true;
    }
    // 2-word messages: skip only if not code-related
    if word_count == 2 && !is_code_related(trimmed) {
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
    if simple_prefixes.iter().any(|&prefix| {
        lower.starts_with(prefix)
            && (lower.len() == prefix.len() || lower.as_bytes().get(prefix.len()) == Some(&b' '))
    }) {
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

/// Check if message is code-related (contains programming keywords or file mentions)
fn is_code_related(message: &str) -> bool {
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

/// Main context injection manager
pub struct ContextInjectionManager {
    pool: Arc<DatabasePool>,
    semantic_injector: SemanticInjector,
    file_injector: FileAwareInjector,
    goal_injector: GoalAwareInjector,
    convention_injector: ConventionInjector,
    budget_manager: BudgetManager,
    cache: InjectionCache,
    analytics: InjectionAnalytics,
    config: InjectionConfig,
}

impl ContextInjectionManager {
    pub async fn new(
        pool: Arc<DatabasePool>,
        code_pool: Option<Arc<DatabasePool>>,
        embeddings: Option<Arc<EmbeddingClient>>,
        fuzzy: Option<Arc<FuzzyCache>>,
    ) -> Self {
        // Load config from database
        let config = InjectionConfig::load(&pool).await.unwrap_or_default();

        Self {
            pool: pool.clone(),
            semantic_injector: SemanticInjector::new(pool.clone(), code_pool, embeddings, fuzzy),
            file_injector: FileAwareInjector::new(pool.clone()),
            goal_injector: GoalAwareInjector::new(pool.clone()),
            convention_injector: ConventionInjector::new(pool.clone()),
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
        .unwrap_or_else(|e| {
            tracing::warn!("Failed to get project info: {}", e);
            Default::default()
        })
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
        if is_simple_command(user_message) {
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
            let mut hasher = std::hash::DefaultHasher::new();
            user_message.hash(&mut hasher);
            let hash = hasher.finish() % 100;
            let threshold = (self.config.sample_rate * 100.0) as u64;
            if hash >= threshold {
                return InjectionResult::skipped("sampled_out");
            }
        }

        // Check if message is code-related
        if !is_code_related(user_message) {
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

        // Collect context from different injectors with priority scores
        let mut entries: Vec<budget::BudgetEntry> = Vec::new();
        let mut sources = Vec::new();

        // Convention context (based on working modules, not message text)
        if self.config.enable_convention {
            let conv = self
                .convention_injector
                .inject_convention_context(session_id, project_id, project_path.as_deref())
                .await;
            if !conv.is_empty() {
                entries.push(budget::BudgetEntry::new(
                    PRIORITY_CONVENTION,
                    conv,
                    "convention",
                ));
                sources.push(InjectionSource::Convention);
            }
        }

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
                entries.push(budget::BudgetEntry::new(
                    PRIORITY_SEMANTIC,
                    semantic_context,
                    "semantic",
                ));
                sources.push(InjectionSource::Semantic);
            }
        }

        // File mention context
        if self.config.enable_file_aware {
            let file_paths = self.file_injector.extract_file_mentions(user_message);
            if !file_paths.is_empty() {
                let file_context = self.file_injector.inject_file_context(file_paths).await;
                if !file_context.is_empty() {
                    entries.push(budget::BudgetEntry::new(
                        PRIORITY_FILE_AWARE,
                        file_context,
                        "files",
                    ));
                    sources.push(InjectionSource::FileAware);
                }
            }
        }

        // Goal context
        if self.config.enable_task_aware {
            let goal_ids = self.goal_injector.get_active_goal_ids(project_id).await;
            if !goal_ids.is_empty() {
                let goal_context = self.goal_injector.inject_goal_context(goal_ids).await;
                if !goal_context.is_empty() {
                    entries.push(budget::BudgetEntry::new(
                        PRIORITY_GOALS,
                        goal_context,
                        "goals",
                    ));
                    sources.push(InjectionSource::TaskAware);
                }
            }
        }

        // Apply priority-based budget management
        let final_context = self.budget_manager.apply_budget_prioritized(entries);

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
                    key_terms: analytics::extract_key_terms(&final_context),
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

    /// Get injection analytics summary
    pub async fn get_analytics_summary(&self, project_id: Option<i64>) -> String {
        self.analytics.summary(project_id).await
    }

    /// Record feedback on whether injected context was referenced in a response.
    ///
    /// Call this from a PostToolUse or Stop hook with the assistant's response
    /// text. It checks pending injection_feedback rows for the session and
    /// marks them as referenced or not based on keyword overlap.
    pub async fn record_response_feedback(&self, session_id: &str, response_text: &str) {
        self.analytics
            .record_response_feedback(session_id, response_text)
            .await;
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
