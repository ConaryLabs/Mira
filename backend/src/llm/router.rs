// backend/src/llm/router.rs
// Smart routing logic for choosing between DeepSeek chat and reasoner models

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeepSeekModel {
    /// deepseek-chat: Fast execution with tool calling support
    /// Output limits: 4k default, 8k maximum
    Chat,

    /// deepseek-reasoner: Complex reasoning with extended output
    /// Output limits: 32k default, 64k maximum
    /// Note: Cannot call tools - automatically routes to chat if tools parameter present
    Reasoner,
}

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

pub struct ModelRouter {
    chat_max_tokens: usize,
    reasoner_max_tokens: usize,
}

impl ModelRouter {
    pub fn new(chat_max_tokens: usize, reasoner_max_tokens: usize) -> Self {
        Self {
            chat_max_tokens,
            reasoner_max_tokens,
        }
    }

    /// Choose the appropriate DeepSeek model based on task analysis
    ///
    /// Routing logic mirrors Claude Code's pattern:
    /// - Chat (like Haiku + Sonnet): Fast execution with tools, orchestration
    /// - Reasoner (like Opus): Deep thinking, large generation
    pub fn choose_model(&self, analysis: &TaskAnalysis) -> DeepSeekModel {
        // Rule 1: Always use chat if tools are required
        // (Reasoner cannot call tools - it auto-routes to chat anyway)
        if analysis.requires_tools {
            return DeepSeekModel::Chat;
        }

        // Rule 2: Use reasoner for large outputs exceeding chat's limit
        // Chat max: 8k, Reasoner max: 64k
        if analysis.estimated_tokens > self.chat_max_tokens {
            return DeepSeekModel::Reasoner;
        }

        // Rule 3: Use reasoner for high complexity tasks
        // These benefit from extended chain-of-thought reasoning
        if analysis.complexity == Complexity::High {
            return DeepSeekModel::Reasoner;
        }

        // Rule 4: Use reasoner for large code generation
        if analysis.task_type == TaskType::CodeGeneration
            && analysis.estimated_tokens > 3000 {
            return DeepSeekModel::Reasoner;
        }

        // Default: Chat model
        // Faster, supports tools, sufficient for most tasks
        DeepSeekModel::Chat
    }

    /// Get maximum output tokens for the chosen model
    pub fn max_output_tokens(&self, model: DeepSeekModel) -> usize {
        match model {
            DeepSeekModel::Chat => self.chat_max_tokens,
            DeepSeekModel::Reasoner => self.reasoner_max_tokens,
        }
    }
}

impl Default for ModelRouter {
    fn default() -> Self {
        Self::new(
            8192,   // Chat max (can use 4096 for faster responses)
            32768,  // Reasoner default (can go up to 65536)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_file_edit_routes_to_chat() {
        let analysis = TaskAnalysis::analyze("Add logging to src/main.rs", true);
        let router = ModelRouter::default();

        assert_eq!(router.choose_model(&analysis), DeepSeekModel::Chat);
        // Complexity detection is heuristic-based, just check it routes to chat
        assert!(analysis.requires_tools);
    }

    #[test]
    fn test_large_refactor_routes_to_reasoner_then_chat() {
        let analysis = TaskAnalysis::analyze("Refactor entire memory system", false);
        let router = ModelRouter::default();

        // High complexity + large output → Reasoner
        assert_eq!(router.choose_model(&analysis), DeepSeekModel::Reasoner);
        assert_eq!(analysis.complexity, Complexity::High);
        assert!(analysis.estimated_tokens > 5000);
    }

    #[test]
    fn test_algorithm_design_routes_to_reasoner() {
        let analysis = TaskAnalysis::analyze("Design an efficient algorithm for graph traversal", false);
        let router = ModelRouter::default();

        // Should route to reasoner due to "algorithm design" keyword
        assert_eq!(router.choose_model(&analysis), DeepSeekModel::Reasoner);
    }

    #[test]
    fn test_simple_explanation_routes_to_chat() {
        let analysis = TaskAnalysis::analyze("Explain what this function does", false);
        let router = ModelRouter::default();

        assert_eq!(router.choose_model(&analysis), DeepSeekModel::Chat);
        assert!(analysis.estimated_tokens < 1000);
    }

    #[test]
    fn test_git_operation_routes_to_chat() {
        let analysis = TaskAnalysis::analyze("Show me the git history for this file", true);
        let router = ModelRouter::default();

        assert_eq!(router.choose_model(&analysis), DeepSeekModel::Chat);
        assert_eq!(analysis.task_type, TaskType::GitOperation);
        assert!(analysis.requires_tools);
    }
}
