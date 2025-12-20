//! Conductor configuration
//!
//! Tunable parameters for the orchestration layer.

use std::time::Duration;

/// Configuration for the Conductor
#[derive(Debug, Clone)]
pub struct ConductorConfig {
    // === Context Budget ===
    /// Maximum tokens per DeepSeek turn (context budget)
    pub turn_budget_tokens: u32,

    /// Maximum tokens for AST context per turn
    pub ast_budget_tokens: u32,

    /// Maximum tokens for conversation history per turn
    pub history_budget_tokens: u32,

    // === Retry Limits ===
    /// Maximum planning attempts before escalating
    pub max_planning_attempts: u32,

    /// Maximum tool call retries per step
    pub max_tool_retries: u32,

    /// Maximum total steps in a plan
    pub max_plan_steps: u32,

    // === Timeouts ===
    /// Timeout for planning phase
    pub planning_timeout: Duration,

    /// Timeout for a single execution step
    pub step_timeout: Duration,

    /// Total timeout for the entire task
    pub task_timeout: Duration,

    // === Output Limits ===
    /// Maximum output tokens for Reasoner (64k limit)
    pub reasoner_max_output: u32,

    /// Maximum output tokens for Chat (8k limit)
    pub chat_max_output: u32,

    // === Diff Settings ===
    /// Require diff format for file edits
    pub require_diff_edits: bool,

    /// Maximum file size (bytes) before requiring AST outline
    pub large_file_threshold: usize,

    // === Escalation ===
    /// Auto-escalate to GPT-5.2 on failure
    pub auto_escalate: bool,

    /// Escalate if task mentions these keywords
    pub escalate_keywords: Vec<String>,

    // === Cost Tracking ===
    /// Target cost savings vs GPT-5.2 (0.94 = 94% savings)
    pub target_cost_savings: f64,

    /// Abort if estimated cost exceeds this (dollars)
    pub cost_abort_threshold: f64,
}

impl Default for ConductorConfig {
    fn default() -> Self {
        Self {
            // 12k tokens per turn - fits comfortably in 128k context
            // with room for system prompt, history, and output
            turn_budget_tokens: 12_000,
            ast_budget_tokens: 4_000,
            history_budget_tokens: 3_000,

            // Conservative retry limits
            max_planning_attempts: 3,
            max_tool_retries: 2,
            max_plan_steps: 20,

            // Reasonable timeouts
            planning_timeout: Duration::from_secs(120),
            step_timeout: Duration::from_secs(60),
            task_timeout: Duration::from_secs(600),

            // Model-specific output limits
            reasoner_max_output: 64_000,
            chat_max_output: 8_000,

            // Diff settings - enforce efficiency
            require_diff_edits: true,
            large_file_threshold: 500, // ~500 lines

            // Escalation settings
            auto_escalate: true,
            escalate_keywords: vec![
                "architect".into(),
                "refactor entire".into(),
                "security audit".into(),
                "production deployment".into(),
            ],

            // Cost targets
            target_cost_savings: 0.94, // 94% savings vs GPT-5.2
            cost_abort_threshold: 1.0, // Abort if >$1
        }
    }
}

impl ConductorConfig {
    /// Create a config optimized for maximum cost savings
    pub fn penny_pincher() -> Self {
        Self {
            turn_budget_tokens: 8_000, // Even smaller turns
            max_planning_attempts: 2,
            max_tool_retries: 1,
            auto_escalate: false, // Never escalate
            target_cost_savings: 0.98,
            cost_abort_threshold: 0.25,
            ..Default::default()
        }
    }

    /// Create a config that balances cost and reliability
    pub fn balanced() -> Self {
        Self::default()
    }

    /// Create a config optimized for reliability (more escalation)
    pub fn reliable() -> Self {
        Self {
            turn_budget_tokens: 16_000,
            max_planning_attempts: 5,
            max_tool_retries: 3,
            auto_escalate: true,
            target_cost_savings: 0.80, // Accept 80% savings
            cost_abort_threshold: 5.0,
            ..Default::default()
        }
    }

    /// Calculate the usable context per turn
    /// System prompt + history + AST + user input + buffer
    pub fn usable_context(&self) -> u32 {
        // Reserve space for:
        // - System prompt: ~2k
        // - Tool definitions: ~3k
        // - Output buffer: ~2k
        const RESERVED: u32 = 7_000;
        self.turn_budget_tokens.saturating_sub(RESERVED)
    }

    /// Check if a task should auto-escalate based on keywords
    pub fn should_escalate_task(&self, task: &str) -> bool {
        let task_lower = task.to_lowercase();
        self.escalate_keywords
            .iter()
            .any(|kw| task_lower.contains(&kw.to_lowercase()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = ConductorConfig::default();
        assert_eq!(config.turn_budget_tokens, 12_000);
        assert_eq!(config.max_planning_attempts, 3);
        assert!(config.auto_escalate);
    }

    #[test]
    fn test_usable_context() {
        let config = ConductorConfig::default();
        // 12k - 7k reserved = 5k usable
        assert_eq!(config.usable_context(), 5_000);
    }

    #[test]
    fn test_escalate_keywords() {
        let config = ConductorConfig::default();
        assert!(config.should_escalate_task("Please architect the new system"));
        assert!(config.should_escalate_task("Do a security audit"));
        assert!(!config.should_escalate_task("Fix this bug"));
    }

    #[test]
    fn test_penny_pincher() {
        let config = ConductorConfig::penny_pincher();
        assert_eq!(config.turn_budget_tokens, 8_000);
        assert!(!config.auto_escalate);
    }
}
