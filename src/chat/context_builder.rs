//! Unified Context Builder
//!
//! Single source of truth for what content goes to the LLM and how.
//! Clearly separates CACHED content (system prompt, tools) from FRESH content
//! (conversation messages) to avoid duplication and confusion.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                    CACHED (on Gemini side)                      │
//! │  - System prompt: persona, role, guidelines                     │
//! │  - Mira context: corrections, goals, memories, summaries        │
//! │  - Tool definitions (28 tools)                                  │
//! │  - TTL: 1 hour, invalidated when prompt_hash changes            │
//! └─────────────────────────────────────────────────────────────────┘
//!                              │
//!                              ▼
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                    FRESH (sent every request)                   │
//! │  - Conversation history (from DB recent_messages)               │
//! │  - Current user input                                           │
//! │  - Tool results during agentic loop                             │
//! └─────────────────────────────────────────────────────────────────┘
//! ```

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use crate::chat::context::{build_orchestrator_prompt, format_orchestrator_context, MiraContext};
use crate::chat::provider::{Message, MessageRole, ToolDefinition};
use crate::chat::session::{AssembledContext, Checkpoint};
use crate::chat::tools::get_tool_definitions;

/// Content that should be cached on the LLM side (stable, expensive to send repeatedly)
#[derive(Debug, Clone)]
pub struct CachedContent {
    /// Full system prompt (persona + guidelines + Mira context)
    pub system_prompt: String,
    /// Hash of system prompt for cache invalidation
    pub prompt_hash: u64,
    /// Tool definitions (cached with system prompt)
    pub tools: Vec<ToolDefinition>,
    /// Token estimate for the cached content
    pub estimated_tokens: usize,
}

/// Content that must be sent fresh every request (changes frequently)
#[derive(Debug, Clone)]
pub struct FreshContent {
    /// Conversation history (user/assistant messages)
    pub messages: Vec<Message>,
    /// Current user input
    pub user_input: String,
    /// Token estimate for fresh content
    pub estimated_tokens: usize,
}

/// Metrics for debugging and monitoring context usage
#[derive(Debug, Default)]
pub struct ContextMetrics {
    /// Tokens in system prompt (cached)
    pub system_prompt_tokens: usize,
    /// Tokens in Mira context (corrections, goals, etc.)
    pub mira_context_tokens: usize,
    /// Tokens in tool definitions
    pub tools_tokens: usize,
    /// Tokens in conversation history
    pub history_tokens: usize,
    /// Tokens in current user input
    pub input_tokens: usize,
    /// Total cached tokens
    pub total_cached: usize,
    /// Total fresh tokens
    pub total_fresh: usize,
    /// Whether cache was invalidated this request
    pub cache_invalidated: bool,
    /// Reason for cache invalidation (if any)
    pub invalidation_reason: Option<String>,
}

/// Unified context builder - single source of truth for LLM context
pub struct ContextBuilder {
    /// Base prompt (persona, role, guidelines)
    base_prompt: String,
    /// Mira context blob (corrections, goals, memories, etc.)
    mira_context: String,
    /// Checkpoint context (if resuming)
    checkpoint_context: Option<String>,
    /// Conversation history (from recent_messages)
    history: Vec<Message>,
    /// Current user input
    user_input: String,
    /// Tool definitions
    tools: Vec<ToolDefinition>,
    /// Previous prompt hash (for cache validation)
    previous_hash: Option<u64>,
}

impl ContextBuilder {
    /// Create a new context builder from assembled context
    pub fn new(
        mira_ctx: &MiraContext,
        assembled: &AssembledContext,
        user_input: &str,
    ) -> Self {
        // Build base prompt (persona, role, guidelines, project path)
        let base_prompt = build_orchestrator_prompt(mira_ctx);

        // Build Mira context (corrections, goals, memories, summaries, etc.)
        // NOTE: This explicitly EXCLUDES recent_messages to avoid duplication
        let mira_context = format_orchestrator_context(assembled);

        // Convert recent messages to conversation history
        // These go in FRESH content, not cached
        let history: Vec<Message> = assembled
            .recent_messages
            .iter()
            .map(|m| {
                let role = match m.role.as_str() {
                    "user" => MessageRole::User,
                    "assistant" => MessageRole::Assistant,
                    "tool" => MessageRole::Tool,
                    _ => MessageRole::User,
                };
                Message {
                    role,
                    content: m.content.clone(),
                }
            })
            .collect();

        // Get tool definitions
        let tools = get_tool_definitions();

        Self {
            base_prompt,
            mira_context,
            checkpoint_context: None,
            history,
            user_input: user_input.to_string(),
            tools,
            previous_hash: None,
        }
    }

    /// Create a minimal builder (when session manager isn't available)
    pub fn minimal(mira_ctx: &MiraContext, user_input: &str) -> Self {
        let base_prompt = build_orchestrator_prompt(mira_ctx);
        let tools = get_tool_definitions();

        Self {
            base_prompt,
            mira_context: String::new(),
            checkpoint_context: None,
            history: Vec::new(),
            user_input: user_input.to_string(),
            tools,
            previous_hash: None,
        }
    }

    /// Add checkpoint context for session continuity
    pub fn with_checkpoint(mut self, checkpoint: &Checkpoint) -> Self {
        self.checkpoint_context = Some(format!(
            "# Last Checkpoint\nTask: {}\nLast action: {}\nRemaining: {}\nFiles: {}",
            checkpoint.current_task,
            checkpoint.last_action,
            checkpoint.remaining.as_deref().unwrap_or("none"),
            checkpoint.working_files.join(", ")
        ));
        self
    }

    /// Set previous hash for cache validation tracking
    pub fn with_previous_hash(mut self, hash: u64) -> Self {
        self.previous_hash = Some(hash);
        self
    }

    /// Build the full system prompt (for caching)
    fn build_system_prompt(&self) -> String {
        let mut sections = vec![self.base_prompt.clone()];

        if !self.mira_context.is_empty() {
            sections.push(self.mira_context.clone());
        }

        if let Some(ref cp) = self.checkpoint_context {
            sections.push(cp.clone());
        }

        sections.join("\n\n")
    }

    /// Compute hash of cacheable content
    fn compute_hash(content: &str) -> u64 {
        let mut hasher = DefaultHasher::new();
        content.hash(&mut hasher);
        hasher.finish()
    }

    /// Estimate tokens (rough: 1 token ~ 4 chars)
    fn estimate_tokens(s: &str) -> usize {
        s.len() / 4
    }

    /// Build cached content (system prompt + tools)
    ///
    /// This content should be cached on the LLM side for cost savings.
    /// Only changes when Mira context (corrections, goals, memories) changes.
    pub fn build_cached(&self) -> CachedContent {
        let system_prompt = self.build_system_prompt();
        let prompt_hash = Self::compute_hash(&system_prompt);

        // Estimate tokens for tools
        let tools_json = serde_json::to_string(&self.tools).unwrap_or_default();
        let tools_tokens = Self::estimate_tokens(&tools_json);

        CachedContent {
            estimated_tokens: Self::estimate_tokens(&system_prompt) + tools_tokens,
            system_prompt,
            prompt_hash,
            tools: self.tools.clone(),
        }
    }

    /// Build fresh content (conversation + user input)
    ///
    /// This content is sent with every request and is NOT cached.
    /// Includes conversation history and current user input.
    pub fn build_fresh(&self) -> FreshContent {
        let mut messages = self.history.clone();

        // Deduplicate: only add user input if it's not already the last message
        let already_in_history = messages
            .last()
            .map(|m| m.role == MessageRole::User && m.content == self.user_input)
            .unwrap_or(false);

        if !already_in_history && !self.user_input.is_empty() {
            messages.push(Message {
                role: MessageRole::User,
                content: self.user_input.clone(),
            });
        }

        // Estimate tokens
        let history_tokens: usize = messages.iter().map(|m| Self::estimate_tokens(&m.content)).sum();

        FreshContent {
            messages,
            user_input: self.user_input.clone(),
            estimated_tokens: history_tokens,
        }
    }

    /// Build both cached and fresh content, plus metrics
    ///
    /// This is the main entry point for the chat handler.
    pub fn build(&self) -> (CachedContent, FreshContent, ContextMetrics) {
        let cached = self.build_cached();
        let fresh = self.build_fresh();

        // Check cache invalidation
        let (cache_invalidated, invalidation_reason) = match self.previous_hash {
            Some(prev) if prev != cached.prompt_hash => {
                (true, Some("prompt_hash changed".to_string()))
            }
            None => (false, None),
            _ => (false, None),
        };

        let metrics = ContextMetrics {
            system_prompt_tokens: Self::estimate_tokens(&self.base_prompt),
            mira_context_tokens: Self::estimate_tokens(&self.mira_context),
            tools_tokens: cached.estimated_tokens - Self::estimate_tokens(&cached.system_prompt),
            history_tokens: fresh
                .messages
                .iter()
                .filter(|m| m.content != self.user_input)
                .map(|m| Self::estimate_tokens(&m.content))
                .sum(),
            input_tokens: Self::estimate_tokens(&self.user_input),
            total_cached: cached.estimated_tokens,
            total_fresh: fresh.estimated_tokens,
            cache_invalidated,
            invalidation_reason,
        };

        (cached, fresh, metrics)
    }

    /// Get just the system prompt (for non-caching scenarios)
    pub fn system_prompt(&self) -> String {
        self.build_system_prompt()
    }

    /// Get just the tools
    pub fn tools(&self) -> &[ToolDefinition] {
        &self.tools
    }

    /// Get conversation messages (for continuation requests)
    pub fn conversation(&self) -> Vec<Message> {
        self.build_fresh().messages
    }

    /// Log context breakdown for debugging
    pub fn log_breakdown(&self) {
        let (_cached, fresh, metrics) = self.build();

        tracing::debug!(
            target: "context_builder",
            base_prompt_tokens = metrics.system_prompt_tokens,
            mira_context_tokens = metrics.mira_context_tokens,
            tools_tokens = metrics.tools_tokens,
            history_tokens = metrics.history_tokens,
            input_tokens = metrics.input_tokens,
            total_cached = metrics.total_cached,
            total_fresh = metrics.total_fresh,
            history_count = fresh.messages.len(),
            cache_invalidated = metrics.cache_invalidated,
            "Context breakdown"
        );

        if metrics.cache_invalidated {
            if let Some(reason) = &metrics.invalidation_reason {
                tracing::info!(
                    target: "context_builder",
                    reason = %reason,
                    "Cache invalidated"
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_builder() {
        let mira_ctx = MiraContext::default();
        let builder = ContextBuilder::minimal(&mira_ctx, "Hello");

        let (cached, fresh, metrics) = builder.build();

        assert!(!cached.system_prompt.is_empty());
        assert!(!cached.tools.is_empty());
        assert_eq!(fresh.messages.len(), 1);
        assert_eq!(fresh.messages[0].content, "Hello");
        assert!(!metrics.cache_invalidated);
    }

    #[test]
    fn test_deduplication() {
        use crate::chat::session::ChatMessage;

        let mira_ctx = MiraContext::default();
        let mut assembled = AssembledContext::default();
        assembled.recent_messages.push(ChatMessage {
            id: "test-1".into(),
            role: "user".into(),
            content: "Hello".into(),
            created_at: 0,
        });

        let builder = ContextBuilder::new(&mira_ctx, &assembled, "Hello");
        let fresh = builder.build_fresh();

        // Should NOT duplicate "Hello" - it's already in history
        assert_eq!(fresh.messages.len(), 1);
    }

    #[test]
    fn test_cache_invalidation_detection() {
        let mira_ctx = MiraContext::default();
        let builder1 = ContextBuilder::minimal(&mira_ctx, "Hello");
        let (cached1, _, _) = builder1.build();

        // Same content, same hash
        let builder2 = ContextBuilder::minimal(&mira_ctx, "Hello")
            .with_previous_hash(cached1.prompt_hash);
        let (_, _, metrics2) = builder2.build();
        assert!(!metrics2.cache_invalidated);

        // Different hash should detect invalidation
        let builder3 = ContextBuilder::minimal(&mira_ctx, "Hello")
            .with_previous_hash(12345);
        let (_, _, metrics3) = builder3.build();
        assert!(metrics3.cache_invalidated);
    }
}
