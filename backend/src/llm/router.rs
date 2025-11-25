// backend/src/llm/router.rs
// Smart routing logic for choosing GPT 5.1 reasoning effort

use serde::{Deserialize, Serialize};
use crate::llm::provider::ReasoningEffort;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Complexity {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskType {
    FileOperation,
    GitOperation,
    CodeGeneration,
    Analysis,
    Conversation,
    Refactoring,
}

#[derive(Debug, Clone)]
pub struct TaskAnalysis {
    pub requires_tools: bool,
    pub estimated_tokens: usize,
    pub complexity: Complexity,
    pub task_type: TaskType,
}

impl TaskAnalysis {
    /// Analyze a user message to determine task characteristics
    pub fn analyze(message: &str, has_tools: bool) -> Self {
        let message_lower = message.to_lowercase();

        // Determine if tools are required
        let requires_tools = has_tools || Self::detect_tool_requirement(&message_lower);

        // Estimate output tokens based on keywords and message length
        let estimated_tokens = Self::estimate_output_tokens(&message_lower);

        // Determine task complexity
        let complexity = Self::determine_complexity(&message_lower);

        // Classify task type
        let task_type = Self::classify_task_type(&message_lower);

        Self {
            requires_tools,
            estimated_tokens,
            complexity,
            task_type,
        }
    }

    fn detect_tool_requirement(message: &str) -> bool {
        // Keywords that suggest tool usage
        let tool_keywords = [
            "read", "write", "edit", "create", "modify", "update",
            "git", "commit", "branch", "diff",
            "search", "find", "locate",
            "analyze", "check", "inspect",
        ];

        tool_keywords.iter().any(|kw| message.contains(kw))
    }

    fn estimate_output_tokens(message: &str) -> usize {
        // Heuristic: estimate based on keywords and message characteristics
        let mut estimate = 1000; // Base estimate

        // Large output indicators
        if message.contains("refactor entire") || message.contains("implement") {
            estimate += 5000;
        }

        if message.contains("multiple files") || message.contains("all files") {
            estimate += 3000;
        }

        if message.contains("complete implementation") {
            estimate += 4000;
        }

        // Specific file generation
        if message.contains("generate") && (message.contains("file") || message.contains("module")) {
            estimate += 2000;
        }

        // Analysis tends to be smaller
        if message.contains("explain") || message.contains("what is") || message.contains("how does") {
            estimate = 500;
        }

        estimate
    }

    fn determine_complexity(message: &str) -> Complexity {
        // High complexity indicators
        let high_indicators = [
            "refactor entire", "redesign", "architecture",
            "implement system", "complex", "sophisticated",
            "optimize", "algorithm design", "algorithm for",
            "design an", "design a",
        ];

        if high_indicators.iter().any(|ind| message.contains(ind)) {
            return Complexity::High;
        }

        // Low complexity indicators
        let low_indicators = [
            "simple", "quick", "just", "only",
            "add a line", "change", "fix typo",
        ];

        if low_indicators.iter().any(|ind| message.contains(ind)) {
            return Complexity::Low;
        }

        // Default to medium
        Complexity::Medium
    }

    fn classify_task_type(message: &str) -> TaskType {
        if message.contains("refactor") {
            TaskType::Refactoring
        } else if message.contains("git") || message.contains("commit") || message.contains("branch") {
            TaskType::GitOperation
        } else if message.contains("read") || message.contains("write") || message.contains("edit") {
            TaskType::FileOperation
        } else if message.contains("generate") || message.contains("implement") || message.contains("create") {
            TaskType::CodeGeneration
        } else if message.contains("analyze") || message.contains("explain") || message.contains("what") {
            TaskType::Analysis
        } else {
            TaskType::Conversation
        }
    }
}

/// Routes tasks to appropriate GPT 5.1 reasoning effort levels
pub struct ReasoningRouter {
    default_effort: ReasoningEffort,
}

impl ReasoningRouter {
    pub fn new(default_effort: ReasoningEffort) -> Self {
        Self { default_effort }
    }

    /// Choose the appropriate reasoning effort based on task analysis
    ///
    /// Routing logic:
    /// - Minimum: Simple tasks, quick responses
    /// - Medium: Standard tasks, balanced reasoning
    /// - High: Complex tasks requiring deep thinking
    pub fn choose_effort(&self, analysis: &TaskAnalysis) -> ReasoningEffort {
        // High complexity tasks need high reasoning effort
        if analysis.complexity == Complexity::High {
            return ReasoningEffort::High;
        }

        // Large code generation benefits from high effort
        if analysis.task_type == TaskType::CodeGeneration
            && analysis.estimated_tokens > 3000 {
            return ReasoningEffort::High;
        }

        // Refactoring benefits from medium-to-high effort
        if analysis.task_type == TaskType::Refactoring {
            return ReasoningEffort::Medium;
        }

        // Simple tasks can use minimum effort
        if analysis.complexity == Complexity::Low {
            return ReasoningEffort::Minimum;
        }

        // Default: use configured default
        self.default_effort.clone()
    }
}

impl Default for ReasoningRouter {
    fn default() -> Self {
        Self::new(ReasoningEffort::Medium)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_file_edit_low_complexity() {
        let analysis = TaskAnalysis::analyze("Just add logging to src/main.rs", true);
        let router = ReasoningRouter::default();

        assert_eq!(router.choose_effort(&analysis), ReasoningEffort::Minimum);
        assert!(analysis.requires_tools);
    }

    #[test]
    fn test_large_refactor_high_effort() {
        let analysis = TaskAnalysis::analyze("Refactor entire memory system", false);
        let router = ReasoningRouter::default();

        // High complexity -> High effort
        assert_eq!(router.choose_effort(&analysis), ReasoningEffort::High);
        assert_eq!(analysis.complexity, Complexity::High);
    }

    #[test]
    fn test_algorithm_design_high_effort() {
        let analysis = TaskAnalysis::analyze("Design an efficient algorithm for graph traversal", false);
        let router = ReasoningRouter::default();

        assert_eq!(router.choose_effort(&analysis), ReasoningEffort::High);
    }

    #[test]
    fn test_simple_explanation_minimum_effort() {
        let analysis = TaskAnalysis::analyze("Just explain what this function does", false);
        let router = ReasoningRouter::default();

        assert_eq!(router.choose_effort(&analysis), ReasoningEffort::Minimum);
    }

    #[test]
    fn test_git_operation_default_effort() {
        let analysis = TaskAnalysis::analyze("Show me the git history for this file", true);
        let router = ReasoningRouter::default();

        assert_eq!(router.choose_effort(&analysis), ReasoningEffort::Medium);
        assert_eq!(analysis.task_type, TaskType::GitOperation);
    }
}
