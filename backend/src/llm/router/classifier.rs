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

    /// Operation kinds that require Code tier (code-focused complex tasks)
    const CODE_OPERATIONS: &'static [&'static str] = &[
        // Complex reasoning operations
        "architecture",
        "refactor",
        "refactor_multi_file",
        "debug_complex",
        "design_pattern",
        "impact_analysis",
        "code_review",
        "security_audit",
        // Code-specific operations
        "code_generation",
        "test_generation",
        "implement_feature",
        "fix_bug",
    ];

    /// Operation kinds that require Agentic tier (long-running autonomous tasks)
    const AGENTIC_OPERATIONS: &'static [&'static str] = &[
        "full_implementation",
        "migration",
        "large_refactor",
        "codebase_modernization",
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

        // Long-running tasks always go to Agentic tier
        if task.is_long_running {
            return ModelTier::Agentic;
        }

        // Check for agentic operations first
        if let Some(ref op_kind) = task.operation_kind {
            if Self::AGENTIC_OPERATIONS.iter().any(|o| op_kind.contains(o)) {
                return ModelTier::Agentic;
            }
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
                // But bump to Code if context is large
                if task.estimated_tokens > self.config.code_token_threshold {
                    return ModelTier::Code;
                }
                return ModelTier::Voice;
            }
        }

        // Check operation kind for code-focused operations
        if let Some(ref op_kind) = task.operation_kind {
            if Self::CODE_OPERATIONS.iter().any(|o| op_kind.contains(o)) {
                return ModelTier::Code;
            }
        }

        // Complexity heuristics

        // Large context -> Code (needs better reasoning)
        if task.estimated_tokens > self.config.code_token_threshold {
            return ModelTier::Code;
        }

        // Multiple files -> Code (cross-file understanding needed)
        if task.file_count > self.config.code_file_threshold {
            return ModelTier::Code;
        }

        // Default: Voice tier for balanced cost/quality
        ModelTier::Voice
    }

    /// Get the classification reason for logging
    pub fn classification_reason(&self, task: &RoutingTask) -> &'static str {
        if task.tier_override.is_some() {
            return "explicit override";
        }

        if task.is_long_running {
            return "long-running task";
        }

        if let Some(ref op_kind) = task.operation_kind {
            if Self::AGENTIC_OPERATIONS.iter().any(|o| op_kind.contains(o)) {
                return "agentic operation";
            }
        }

        if task.is_user_facing && task.tool_name.is_none() {
            return "user-facing chat";
        }

        if let Some(ref tool_name) = task.tool_name {
            if Self::FAST_TOOLS.iter().any(|t| tool_name.contains(t)) {
                return "fast-tier tool";
            }
            if Self::VOICE_TOOLS.iter().any(|t| tool_name.contains(t)) {
                if task.estimated_tokens > self.config.code_token_threshold {
                    return "voice tool with large context";
                }
                return "voice-tier tool";
            }
        }

        if let Some(ref op_kind) = task.operation_kind {
            if Self::CODE_OPERATIONS.iter().any(|o| op_kind.contains(o)) {
                return "code operation";
            }
        }

        if task.estimated_tokens > self.config.code_token_threshold {
            return "large context";
        }

        if task.file_count > self.config.code_file_threshold {
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
    fn test_code_operations() {
        let classifier = test_classifier();

        let code_ops = vec![
            "architecture",
            "refactor_multi_file",
            "debug_complex",
            "code_review",
        ];

        for op in code_ops {
            let task = RoutingTask::new().with_operation(op);
            assert_eq!(
                classifier.classify(&task),
                ModelTier::Code,
                "Operation {} should be Code tier",
                op
            );
        }
    }

    #[test]
    fn test_agentic_operations() {
        let classifier = test_classifier();

        let agentic_ops = vec![
            "full_implementation",
            "migration",
            "large_refactor",
            "codebase_modernization",
        ];

        for op in agentic_ops {
            let task = RoutingTask::new().with_operation(op);
            assert_eq!(
                classifier.classify(&task),
                ModelTier::Agentic,
                "Operation {} should be Agentic tier",
                op
            );
        }
    }

    #[test]
    fn test_long_running_flag() {
        let classifier = test_classifier();

        // Even a simple task becomes Agentic when marked as long-running
        let task = RoutingTask::new().with_long_running(true);
        assert_eq!(classifier.classify(&task), ModelTier::Agentic);
    }

    #[test]
    fn test_user_chat_is_voice() {
        let classifier = test_classifier();
        let task = RoutingTask::user_chat();
        assert_eq!(classifier.classify(&task), ModelTier::Voice);
    }

    #[test]
    fn test_large_context_upgrade_to_code() {
        let classifier = test_classifier();

        // Voice tool with small context -> Voice
        let task = RoutingTask::from_tool("read_project_file").with_tokens(10_000);
        assert_eq!(classifier.classify(&task), ModelTier::Voice);

        // Voice tool with large context -> Code
        let task = RoutingTask::from_tool("read_project_file").with_tokens(100_000);
        assert_eq!(classifier.classify(&task), ModelTier::Code);
    }

    #[test]
    fn test_multi_file_upgrade_to_code() {
        let classifier = test_classifier();

        // Few files -> default (Voice)
        let task = RoutingTask::new().with_files(2);
        assert_eq!(classifier.classify(&task), ModelTier::Voice);

        // Many files -> Code
        let task = RoutingTask::new().with_files(5);
        assert_eq!(classifier.classify(&task), ModelTier::Code);
    }

    #[test]
    fn test_explicit_override() {
        let classifier = test_classifier();

        // Fast tool but forced to Code
        let task = RoutingTask::from_tool("list_project_files").with_tier(ModelTier::Code);
        assert_eq!(classifier.classify(&task), ModelTier::Code);

        // Fast tool but forced to Agentic
        let task = RoutingTask::from_tool("list_project_files").with_tier(ModelTier::Agentic);
        assert_eq!(classifier.classify(&task), ModelTier::Agentic);
    }
}
