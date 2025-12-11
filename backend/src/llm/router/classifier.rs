// src/llm/router/classifier.rs
// Task classification for model routing

use super::config::RouterConfig;
use super::types::{ModelTier, RoutingTask};
use crate::session::CodexSpawnTrigger;

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

    /// Patterns in user messages that indicate code-heavy work suitable for Codex
    const CODEX_MESSAGE_PATTERNS: &'static [&'static str] = &[
        "implement",
        "refactor",
        "fix bug",
        "fix the bug",
        "add feature",
        "create a",
        "build a",
        "write code",
        "write tests",
        "add tests",
        "migrate",
        "update all",
        "change all",
        "rename",
        "convert",
    ];

    /// Minimum confidence for Codex spawn detection
    const CODEX_MIN_CONFIDENCE: f32 = 0.7;

    /// Token threshold for Codex spawn (larger than code tier threshold)
    const CODEX_TOKEN_THRESHOLD: i64 = 100_000;

    /// File count threshold for Codex spawn
    const CODEX_FILE_THRESHOLD: usize = 5;

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

    /// Determine if a task should spawn a background Codex session
    ///
    /// Returns `Some(CodexSpawnTrigger)` if the task is suitable for Codex, `None` otherwise.
    /// Codex sessions are for autonomous, long-running code work that runs in the background.
    pub fn should_spawn_codex(
        &self,
        task: &RoutingTask,
        user_message: &str,
    ) -> Option<CodexSpawnTrigger> {
        // Check for explicit opt-out patterns - user wants direct execution
        let message_lower = user_message.to_lowercase();
        if message_lower.contains("do not delegate")
            || message_lower.contains("don't delegate")
            || message_lower.contains("call the tool directly")
            || message_lower.contains("execute directly")
            || message_lower.contains("immediately use the")
            || message_lower.contains("immediately call")
        {
            return None;
        }

        // Check for agentic operations first (highest priority)
        if let Some(ref op_kind) = task.operation_kind {
            if Self::AGENTIC_OPERATIONS.iter().any(|o| op_kind.contains(o)) {
                return Some(CodexSpawnTrigger::ComplexTask {
                    estimated_tokens: task.estimated_tokens,
                    file_count: task.file_count,
                    operation_kind: Some(op_kind.clone()),
                });
            }
        }

        // Check if marked as long-running
        if task.is_long_running {
            return Some(CodexSpawnTrigger::ComplexTask {
                estimated_tokens: task.estimated_tokens,
                file_count: task.file_count,
                operation_kind: task.operation_kind.clone(),
            });
        }

        // Check for code operations with high complexity
        if let Some(ref op_kind) = task.operation_kind {
            if Self::CODE_OPERATIONS.iter().any(|o| op_kind.contains(o)) {
                // Only spawn Codex if complexity is high enough
                if task.estimated_tokens > Self::CODEX_TOKEN_THRESHOLD
                    || task.file_count >= Self::CODEX_FILE_THRESHOLD
                {
                    return Some(CodexSpawnTrigger::ComplexTask {
                        estimated_tokens: task.estimated_tokens,
                        file_count: task.file_count,
                        operation_kind: Some(op_kind.clone()),
                    });
                }
            }
        }

        // Pattern-based detection from user message
        // (message_lower already computed at start of function)
        let mut detected_patterns: Vec<String> = Vec::new();
        let mut pattern_count = 0;

        for pattern in Self::CODEX_MESSAGE_PATTERNS {
            if message_lower.contains(pattern) {
                detected_patterns.push(pattern.to_string());
                pattern_count += 1;
            }
        }

        // Calculate confidence based on pattern matches and complexity
        if pattern_count > 0 {
            let mut confidence: f32 = 0.5 + (pattern_count as f32 * 0.1);

            // Boost confidence for complexity indicators
            if task.estimated_tokens > self.config.code_token_threshold {
                confidence += 0.15;
            }
            if task.file_count > self.config.code_file_threshold {
                confidence += 0.1;
            }
            // Boost for specific high-signal patterns
            if message_lower.contains("implement") || message_lower.contains("refactor") {
                confidence += 0.1;
            }

            confidence = confidence.min(1.0);

            if confidence >= Self::CODEX_MIN_CONFIDENCE {
                return Some(CodexSpawnTrigger::RouterDetection {
                    confidence,
                    detected_patterns,
                });
            }
        }

        None
    }

    /// Get a human-readable reason for Codex spawn decision
    pub fn codex_spawn_reason(&self, trigger: &CodexSpawnTrigger) -> &'static str {
        match trigger {
            CodexSpawnTrigger::RouterDetection { .. } => "detected code-heavy task patterns",
            CodexSpawnTrigger::UserRequest { .. } => "user explicitly requested",
            CodexSpawnTrigger::ComplexTask { .. } => "complex task requiring extended processing",
        }
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

    #[test]
    fn test_codex_spawn_agentic_operation() {
        let classifier = test_classifier();
        let task = RoutingTask::new().with_operation("full_implementation");

        let trigger = classifier.should_spawn_codex(&task, "Implement the entire auth system");
        assert!(trigger.is_some());
        assert!(matches!(trigger.unwrap(), CodexSpawnTrigger::ComplexTask { .. }));
    }

    #[test]
    fn test_codex_spawn_long_running() {
        let classifier = test_classifier();
        let task = RoutingTask::new().with_long_running(true);

        let trigger = classifier.should_spawn_codex(&task, "Do something");
        assert!(trigger.is_some());
        assert!(matches!(trigger.unwrap(), CodexSpawnTrigger::ComplexTask { .. }));
    }

    #[test]
    fn test_codex_spawn_pattern_detection() {
        let classifier = test_classifier();
        let task = RoutingTask::new();

        // High-signal pattern should trigger
        let trigger = classifier.should_spawn_codex(&task, "Please implement this feature with tests");
        assert!(trigger.is_some());
        if let Some(CodexSpawnTrigger::RouterDetection { confidence, detected_patterns }) = trigger {
            assert!(confidence >= 0.7);
            assert!(detected_patterns.contains(&"implement".to_string()));
        }
    }

    #[test]
    fn test_codex_spawn_no_match() {
        let classifier = test_classifier();
        let task = RoutingTask::new();

        // Simple question should not trigger Codex
        let trigger = classifier.should_spawn_codex(&task, "What does this function do?");
        assert!(trigger.is_none());
    }

    #[test]
    fn test_codex_spawn_complex_code_operation() {
        let classifier = test_classifier();
        // Code operation with high complexity
        let task = RoutingTask::new()
            .with_operation("refactor_multi_file")
            .with_tokens(150_000)
            .with_files(10);

        let trigger = classifier.should_spawn_codex(&task, "Refactor the codebase");
        assert!(trigger.is_some());
        assert!(matches!(trigger.unwrap(), CodexSpawnTrigger::ComplexTask { .. }));
    }
}
