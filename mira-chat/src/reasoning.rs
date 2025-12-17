//! Task complexity classifier for reasoning effort routing
//!
//! Maps user queries to appropriate reasoning effort levels:
//! - none: Tool execution, simple queries
//! - low: Code navigation, file search
//! - medium: Code understanding, standard edits
//! - high: Complex refactoring, architecture decisions
//! - xhigh: Critical debugging, deep analysis

/// Reasoning effort levels
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReasoningEffort {
    None,
    Low,
    Medium,
    High,
    XHigh,
}

impl ReasoningEffort {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::XHigh => "xhigh",
        }
    }

    /// Map effort for 5.2 - cap at high, low minimum
    pub fn effort_for_model(&self) -> &'static str {
        match self {
            Self::None => "low",
            Self::XHigh => "high",
            _ => self.as_str(),
        }
    }
}

impl std::fmt::Display for ReasoningEffort {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Classify task complexity based on user input
pub fn classify(input: &str) -> ReasoningEffort {
    let input_lower = input.to_lowercase();

    // XHigh: Deep debugging, critical analysis
    if contains_any(&input_lower, &[
        "deadlock", "race condition", "memory leak", "segfault",
        "critical", "production bug", "urgent", "security vulnerability",
        "why is this", "explain why", "root cause",
    ]) {
        return ReasoningEffort::XHigh;
    }

    // High: Complex refactoring, architecture
    if contains_any(&input_lower, &[
        "refactor", "redesign", "architect", "restructure",
        "implement", "add feature", "create", "build",
        "migrate", "upgrade", "rewrite",
    ]) {
        return ReasoningEffort::High;
    }

    // Medium: Code understanding, standard edits
    if contains_any(&input_lower, &[
        "explain", "what does", "how does", "understand",
        "fix", "update", "change", "modify", "edit",
        "add", "remove", "delete",
    ]) {
        return ReasoningEffort::Medium;
    }

    // Low: Navigation, search
    if contains_any(&input_lower, &[
        "find", "search", "where", "locate", "list",
        "show", "display", "print", "grep",
    ]) {
        return ReasoningEffort::Low;
    }

    // None: Direct tool execution
    if contains_any(&input_lower, &[
        "read", "cat", "ls", "pwd", "git status",
        "run", "execute", "open",
    ]) {
        return ReasoningEffort::None;
    }

    // Default to medium for unknown queries
    ReasoningEffort::Medium
}

fn contains_any(text: &str, patterns: &[&str]) -> bool {
    patterns.iter().any(|p| text.contains(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_xhigh() {
        assert_eq!(classify("why is this deadlocking"), ReasoningEffort::XHigh);
        assert_eq!(classify("critical production bug"), ReasoningEffort::XHigh);
    }

    #[test]
    fn test_classify_high() {
        assert_eq!(classify("refactor the auth module"), ReasoningEffort::High);
        assert_eq!(classify("implement user authentication"), ReasoningEffort::High);
    }

    #[test]
    fn test_classify_medium() {
        assert_eq!(classify("fix the login bug"), ReasoningEffort::Medium);
        assert_eq!(classify("explain this function"), ReasoningEffort::Medium);
    }

    #[test]
    fn test_classify_low() {
        assert_eq!(classify("find all rust files"), ReasoningEffort::Low);
        assert_eq!(classify("where is the config"), ReasoningEffort::Low);
    }

    #[test]
    fn test_classify_none() {
        assert_eq!(classify("read src/main.rs"), ReasoningEffort::None);
        assert_eq!(classify("git status"), ReasoningEffort::None);
    }
}
