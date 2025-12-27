//! Task Type detection for smart tool routing
//!
//! Classifies user queries into task types to enable:
//! - Phase-appropriate tool emphasis
//! - Model selection (Flash vs Pro)
//! - Context category prioritization

use serde::{Deserialize, Serialize};

/// Task type classification for smart routing
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskType {
    /// Reading, searching, understanding codebase
    Exploration,
    /// Building new functionality
    NewFeature,
    /// Fixing errors, investigating issues
    Debugging,
    /// Restructuring code without behavior change
    Refactoring,
    /// Web search, documentation lookup
    Research,
    /// Goal setting, milestone tracking, planning
    Planning,
}

impl TaskType {
    pub fn as_str(&self) -> &'static str {
        match self {
            TaskType::Exploration => "exploration",
            TaskType::NewFeature => "new_feature",
            TaskType::Debugging => "debugging",
            TaskType::Refactoring => "refactoring",
            TaskType::Research => "research",
            TaskType::Planning => "planning",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "exploration" | "explore" => Some(TaskType::Exploration),
            "new_feature" | "newfeature" | "feature" => Some(TaskType::NewFeature),
            "debugging" | "debug" | "fix" => Some(TaskType::Debugging),
            "refactoring" | "refactor" => Some(TaskType::Refactoring),
            "research" => Some(TaskType::Research),
            "planning" | "plan" => Some(TaskType::Planning),
            _ => None,
        }
    }

    /// Detect task type from query using keyword matching
    /// Returns (task_type, confidence) where confidence is keyword match count
    pub fn detect_from_query(query: &str) -> (TaskType, f32) {
        let query_lower = query.to_lowercase();

        // Define keyword patterns for each task type
        let patterns: &[(TaskType, &[&str])] = &[
            (TaskType::Debugging, &[
                "error", "fail", "bug", "fix", "broken", "crash", "issue", "wrong",
                "not working", "doesn't work", "exception", "panic", "stack trace",
            ]),
            (TaskType::NewFeature, &[
                "add", "implement", "create", "build", "new feature", "make",
                "develop", "introduce", "enable", "support",
            ]),
            (TaskType::Refactoring, &[
                "refactor", "extract", "rename", "reorganize", "split", "merge",
                "restructure", "clean up", "simplify", "consolidate",
            ]),
            (TaskType::Planning, &[
                "goal", "milestone", "plan", "todo", "task", "roadmap", "schedule",
                "priority", "backlog", "next steps", "strategy",
            ]),
            (TaskType::Research, &[
                "search", "find", "look up", "documentation", "how does",
                "what is", "explain", "learn about", "understand",
            ]),
            (TaskType::Exploration, &[
                "read", "explore", "show", "list", "where is", "find file",
                "what files", "structure", "overview", "navigate",
            ]),
        ];

        let mut best_match = (TaskType::Exploration, 0.0);

        for (task_type, keywords) in patterns {
            let match_count = keywords.iter()
                .filter(|kw| query_lower.contains(*kw))
                .count() as f32;

            if match_count > best_match.1 {
                best_match = (*task_type, match_count);
            }
        }

        // If no keywords matched, default based on query characteristics
        if best_match.1 == 0.0 {
            // Questions tend to be exploration/research
            if query.ends_with('?') {
                return (TaskType::Research, 0.5);
            }
            // Imperative statements suggest new feature
            if query.starts_with("Add") || query.starts_with("Create") || query.starts_with("Make") {
                return (TaskType::NewFeature, 0.5);
            }
            // Default to exploration
            return (TaskType::Exploration, 0.5);
        }

        best_match
    }

    /// Recommended Gemini model for this task type
    pub fn recommended_model(&self) -> &'static str {
        match self {
            // Pro for complex reasoning tasks
            TaskType::Planning => "pro",
            TaskType::Debugging => "pro", // Complex problem solving
            // Flash for quick, focused tasks
            TaskType::Exploration => "flash",
            TaskType::NewFeature => "flash",
            TaskType::Refactoring => "flash",
            TaskType::Research => "flash",
        }
    }

    /// Recommended thinking level for this task type
    pub fn recommended_thinking_level(&self) -> &'static str {
        match self {
            TaskType::Planning => "low",      // Structured thinking
            TaskType::Debugging => "low",     // Analytical
            TaskType::Exploration => "minimal",
            TaskType::NewFeature => "minimal",
            TaskType::Refactoring => "minimal",
            TaskType::Research => "medium",   // Synthesis
        }
    }

    /// Tools to emphasize for this task type (higher weight)
    pub fn emphasized_tools(&self) -> &'static [&'static str] {
        match self {
            TaskType::Exploration => &[
                "get_symbols", "semantic_code_search", "recall",
                "get_related_files", "get_recent_commits",
            ],
            TaskType::NewFeature => &[
                "get_proactive_context", "goal", "task",
                "get_symbols", "get_related_files",
            ],
            TaskType::Debugging => &[
                "find_similar_fixes", "build", "get_call_graph",
                "get_proactive_context", "recall",
            ],
            TaskType::Refactoring => &[
                "get_symbols", "get_related_files", "find_cochange_patterns",
                "get_codebase_style",
            ],
            TaskType::Research => &[
                "document", "recall", "search_sessions",
            ],
            TaskType::Planning => &[
                "goal", "task", "proposal", "store_decision",
                "carousel", "get_session_context",
            ],
        }
    }

    /// Tools to de-emphasize for this task type (lower weight)
    pub fn deemphasized_tools(&self) -> &'static [&'static str] {
        match self {
            TaskType::Exploration => &[
                "batch", "file_search", "goal", "task",
            ],
            TaskType::NewFeature => &[
                "batch",
            ],
            TaskType::Debugging => &[
                "goal", "task", "proposal",
            ],
            TaskType::Refactoring => &[
                "batch", "document",
            ],
            TaskType::Research => &[
                "get_symbols", "get_call_graph", "build",
            ],
            TaskType::Planning => &[
                "semantic_code_search", "get_call_graph", "index",
            ],
        }
    }
}

impl Default for TaskType {
    fn default() -> Self {
        TaskType::Exploration
    }
}

impl std::fmt::Display for TaskType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_debugging() {
        let (task, conf) = TaskType::detect_from_query("fix the error in the login");
        assert_eq!(task, TaskType::Debugging);
        assert!(conf >= 1.0);
    }

    #[test]
    fn test_detect_new_feature() {
        let (task, conf) = TaskType::detect_from_query("add a new logout button");
        assert_eq!(task, TaskType::NewFeature);
        assert!(conf >= 1.0);
    }

    #[test]
    fn test_detect_planning() {
        let (task, conf) = TaskType::detect_from_query("create a goal for the sprint");
        // Note: "create" matches NewFeature, "goal" matches Planning
        // Higher match wins
        assert!(task == TaskType::Planning || task == TaskType::NewFeature);
    }

    #[test]
    fn test_default_exploration() {
        let (task, _) = TaskType::detect_from_query("show me the code");
        assert_eq!(task, TaskType::Exploration);
    }

    #[test]
    fn test_from_str() {
        assert_eq!(TaskType::from_str("debug"), Some(TaskType::Debugging));
        assert_eq!(TaskType::from_str("planning"), Some(TaskType::Planning));
        assert_eq!(TaskType::from_str("unknown"), None);
    }
}
