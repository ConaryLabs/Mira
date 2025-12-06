// src/llm/router/classifier.rs
// Task classification for model routing

use super::config::RouterConfig;
use super::types::{ModelTier, RoutingTask};

/// Classifies tasks to determine which model tier to use
pub struct TaskClassifier {
    config: RouterConfig,
}

impl TaskClassifier {
    /// Tools that should always use Fast tier
    const FAST_TOOLS: &'static [&'static str] = &[
        // File listing and exploration
        "list_project_files",
        "list_files",
        "get_file_summary",
        "get_file_structure",
        // Search operations
        "search_codebase",
        "grep_files",
        // Simple metadata
        "count_lines",
        "extract_symbols",
        "summarize_file",
    ];

    /// Tools that should use Voice tier (moderate complexity)
    const VOICE_TOOLS: &'static [&'static str] = &[
        // File reading (moderate context needed)
        "read_project_file",
        "read_file",
        // Simple edits
        "edit_project_file",
        // Basic code operations
        "write_project_file",
        "write_file",
    ];

    /// Operation kinds that require Thinker tier
    const THINKER_OPERATIONS: &'static [&'static str] = &[
        "architecture",
        "refactor",
        "refactor_multi_file",
        "debug_complex",
        "design_pattern",
        "impact_analysis",
        "code_review",
        "security_audit",
    ];

    /// Create a new classifier with config
    pub fn new(config: RouterConfig) -> Self {
        Self { config }
    }

    /// Classify a task to determine which tier to use
    pub fn classify(&self, task: &RoutingTask) -> ModelTier {
        // Check for explicit override first
        if let Some(tier) = task.tier_override {
            return tier;
        }

        // User-facing chat always uses Voice tier
        if task.is_user_facing && task.tool_name.is_none() {
            return ModelTier::Voice;
        }

        // Check tool name for fast-path routing
        if let Some(ref tool_name) = task.tool_name {
            if Self::FAST_TOOLS.iter().any(|t| tool_name.contains(t)) {
                return ModelTier::Fast;
            }

            // Voice tier tools
            if Self::VOICE_TOOLS.iter().any(|t| tool_name.contains(t)) {
                // But bump to Thinker if context is large
                if task.estimated_tokens > self.config.thinker_token_threshold {
                    return ModelTier::Thinker;
                }
                return ModelTier::Voice;
            }
        }

        // Check operation kind for complex operations
        if let Some(ref op_kind) = task.operation_kind {
            if Self::THINKER_OPERATIONS
                .iter()
                .any(|o| op_kind.contains(o))
            {
                return ModelTier::Thinker;
            }
        }

        // Complexity heuristics

        // Large context -> Thinker (needs better reasoning)
        if task.estimated_tokens > self.config.thinker_token_threshold {
            return ModelTier::Thinker;
        }

        // Multiple files -> Thinker (cross-file understanding needed)
        if task.file_count > self.config.thinker_file_threshold {
            return ModelTier::Thinker;
        }

        // Default: Voice tier for balanced cost/quality
        ModelTier::Voice
    }

    /// Get the classification reason for logging
    pub fn classification_reason(&self, task: &RoutingTask) -> &'static str {
        if task.tier_override.is_some() {
            return "explicit override";
        }

        if task.is_user_facing && task.tool_name.is_none() {
            return "user-facing chat";
        }

        if let Some(ref tool_name) = task.tool_name {
            if Self::FAST_TOOLS.iter().any(|t| tool_name.contains(t)) {
                return "fast-tier tool";
            }
            if Self::VOICE_TOOLS.iter().any(|t| tool_name.contains(t)) {
                if task.estimated_tokens > self.config.thinker_token_threshold {
                    return "voice tool with large context";
                }
                return "voice-tier tool";
            }
        }

        if let Some(ref op_kind) = task.operation_kind {
            if Self::THINKER_OPERATIONS
                .iter()
                .any(|o| op_kind.contains(o))
            {
                return "complex operation";
            }
        }

        if task.estimated_tokens > self.config.thinker_token_threshold {
            return "large context";
        }

        if task.file_count > self.config.thinker_file_threshold {
            return "multi-file operation";
        }

        "default"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_classifier() -> TaskClassifier {
        TaskClassifier::new(RouterConfig::default())
    }

    #[test]
    fn test_fast_tier_tools() {
        let classifier = test_classifier();

        let fast_tools = vec![
            "list_project_files",
            "search_codebase",
            "get_file_summary",
            "grep_files",
            "count_lines",
        ];

        for tool in fast_tools {
            let task = RoutingTask::from_tool(tool);
            assert_eq!(
                classifier.classify(&task),
                ModelTier::Fast,
                "Tool {} should be Fast tier",
                tool
            );
        }
    }

    #[test]
    fn test_voice_tier_tools() {
        let classifier = test_classifier();

        let voice_tools = vec![
            "read_project_file",
            "edit_project_file",
            "write_project_file",
        ];

        for tool in voice_tools {
            let task = RoutingTask::from_tool(tool);
            assert_eq!(
                classifier.classify(&task),
                ModelTier::Voice,
                "Tool {} should be Voice tier",
                tool
            );
        }
    }

    #[test]
    fn test_thinker_operations() {
        let classifier = test_classifier();

        let thinker_ops = vec![
            "architecture",
            "refactor_multi_file",
            "debug_complex",
            "code_review",
        ];

        for op in thinker_ops {
            let task = RoutingTask::new().with_operation(op);
            assert_eq!(
                classifier.classify(&task),
                ModelTier::Thinker,
                "Operation {} should be Thinker tier",
                op
            );
        }
    }

    #[test]
    fn test_user_chat_is_voice() {
        let classifier = test_classifier();
        let task = RoutingTask::user_chat();
        assert_eq!(classifier.classify(&task), ModelTier::Voice);
    }

    #[test]
    fn test_large_context_upgrade_to_thinker() {
        let classifier = test_classifier();

        // Voice tool with small context -> Voice
        let task = RoutingTask::from_tool("read_project_file").with_tokens(10_000);
        assert_eq!(classifier.classify(&task), ModelTier::Voice);

        // Voice tool with large context -> Thinker
        let task = RoutingTask::from_tool("read_project_file").with_tokens(100_000);
        assert_eq!(classifier.classify(&task), ModelTier::Thinker);
    }

    #[test]
    fn test_multi_file_upgrade_to_thinker() {
        let classifier = test_classifier();

        // Few files -> default (Voice)
        let task = RoutingTask::new().with_files(2);
        assert_eq!(classifier.classify(&task), ModelTier::Voice);

        // Many files -> Thinker
        let task = RoutingTask::new().with_files(5);
        assert_eq!(classifier.classify(&task), ModelTier::Thinker);
    }

    #[test]
    fn test_explicit_override() {
        let classifier = test_classifier();

        // Fast tool but forced to Thinker
        let task = RoutingTask::from_tool("list_project_files").with_tier(ModelTier::Thinker);
        assert_eq!(classifier.classify(&task), ModelTier::Thinker);
    }
}
