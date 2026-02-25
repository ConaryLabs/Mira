// crates/mira-server/src/context/budget.rs
// Token budget management for context injection with priority-based sorting

// --- Named priority constants (highest to lowest) ---
// Used by UserPromptSubmit hook and ContextInjectionManager to assign
// importance to each context source. The budget manager sorts entries
// by priority descending, so higher values survive truncation.

/// Priority for team context (highest -- collaboration is critical)
pub const PRIORITY_TEAM: f32 = 0.95;
/// Priority for coding conventions
pub const PRIORITY_CONVENTION: f32 = 0.9;
/// Priority for reactive/recalled context
pub const PRIORITY_REACTIVE: f32 = 0.75;
/// Priority for semantic search results
pub const PRIORITY_SEMANTIC: f32 = 0.7;
/// Priority for pending tasks
pub const PRIORITY_TASKS: f32 = 0.65;
/// Priority for active goals
pub const PRIORITY_GOALS: f32 = 0.6;
/// Priority for file-aware context
pub const PRIORITY_FILE_AWARE: f32 = 0.4;

/// Result of applying a budget to prioritized entries.
/// Tracks which sources were kept and which were dropped.
#[derive(Debug, Clone)]
pub struct BudgetResult {
    pub content: String,
    pub kept_sources: Vec<String>,
    pub dropped_sources: Vec<String>,
}

/// A single context entry with priority metadata for budget allocation.
/// Higher priority entries are kept when the budget is tight.
#[derive(Debug, Clone)]
pub struct BudgetEntry {
    /// Priority score (0.0 to 1.0). Higher = more important.
    pub priority: f32,
    /// The context content to inject.
    pub content: String,
    /// Human-readable source label (e.g. "convention", "semantic", "team").
    pub source: String,
}

impl BudgetEntry {
    pub fn new(priority: f32, content: String, source: impl Into<String>) -> Self {
        Self {
            priority: priority.clamp(0.0, 1.0),
            content,
            source: source.into(),
        }
    }
}

pub struct BudgetManager {
    max_chars: usize,
}

impl Default for BudgetManager {
    fn default() -> Self {
        Self::new()
    }
}

impl BudgetManager {
    pub fn new() -> Self {
        Self {
            max_chars: 3000, // ~750 tokens - cheap on DeepSeek, gives context room
        }
    }

    /// Create with custom character limit
    pub fn with_limit(max_chars: usize) -> Self {
        Self { max_chars }
    }

    /// Apply token budget to prioritized context entries.
    /// Entries are sorted by priority descending before applying the char limit,
    /// so the most important context is kept when truncation is needed.
    pub fn apply_budget_prioritized(&self, entries: Vec<BudgetEntry>) -> BudgetResult {
        // Filter out empty entries and sort by priority descending
        let mut entries: Vec<BudgetEntry> = entries
            .into_iter()
            .filter(|e| !e.content.is_empty())
            .collect();
        entries.sort_by(|a, b| {
            b.priority
                .partial_cmp(&a.priority)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        if entries.is_empty() {
            return BudgetResult {
                content: String::new(),
                kept_sources: Vec::new(),
                dropped_sources: Vec::new(),
            };
        }

        let mut result = String::new();
        let mut kept_sources: Vec<String> = Vec::new();
        let mut dropped_sources: Vec<String> = Vec::new();
        let mut truncated = false;

        for entry in entries {
            if result.len() + entry.content.len() + 2 > self.max_chars {
                dropped_sources.push(entry.source.clone());
                truncated = true;
                continue;
            }
            if !result.is_empty() {
                result.push_str("\n\n");
            }
            result.push_str(&entry.content);
            kept_sources.push(entry.source.clone());
        }

        // Always append a truncation notice when any entries were dropped
        if truncated && !dropped_sources.is_empty() {
            result.push_str(&format!(
                "\n\n[Context truncated: dropped {}]",
                dropped_sources.join(", ")
            ));
        }

        BudgetResult {
            content: result,
            kept_sources,
            dropped_sources,
        }
    }

    /// Apply token budget to plain string contexts (insertion-order, no sorting).
    /// Backward-compatible wrapper for callers that don't use priorities.
    pub fn apply_budget(&self, contexts: Vec<String>) -> String {
        let entries: Vec<BudgetEntry> = contexts
            .into_iter()
            .enumerate()
            .map(|(i, content)| BudgetEntry {
                // Assign decreasing priority to preserve insertion order
                priority: 1.0 - (i as f32 * 0.001),
                content,
                source: String::new(),
            })
            .collect();
        self.apply_budget_prioritized(entries).content
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_budget_manager_default() {
        let manager = BudgetManager::new();
        assert_eq!(manager.max_chars, 3000);
    }

    #[test]
    fn test_budget_manager_custom_limit() {
        let manager = BudgetManager::with_limit(500);
        assert_eq!(manager.max_chars, 500);
    }

    #[test]
    fn test_apply_budget_empty() {
        let manager = BudgetManager::new();
        let result = manager.apply_budget(vec![]);
        assert!(result.is_empty());
    }

    #[test]
    fn test_apply_budget_filters_empty_strings() {
        let manager = BudgetManager::new();
        let contexts = vec!["".to_string(), "valid context".to_string(), "".to_string()];
        let result = manager.apply_budget(contexts);
        assert_eq!(result, "valid context");
    }

    #[test]
    fn test_apply_budget_single_context() {
        let manager = BudgetManager::new();
        let contexts = vec!["Single context".to_string()];
        let result = manager.apply_budget(contexts);
        assert_eq!(result, "Single context");
    }

    #[test]
    fn test_apply_budget_multiple_contexts() {
        let manager = BudgetManager::new();
        let contexts = vec![
            "First context".to_string(),
            "Second context".to_string(),
            "Third context".to_string(),
        ];
        let result = manager.apply_budget(contexts);
        assert_eq!(result, "First context\n\nSecond context\n\nThird context");
    }

    #[test]
    fn test_apply_budget_truncation() {
        let manager = BudgetManager::with_limit(80);
        let contexts = vec![
            "Short".to_string(),                                    // 5 chars
            "Medium length text".to_string(), // 18 chars, total 25 with separator
            "This won't fit because it's too long".to_string(), // 36 chars, 25+2+36=63 <= 80, fits
            "This definitely exceeds the budget limit".to_string(), // 40 chars, 63+2+40=105 > 80, truncate
        ];
        let result = manager.apply_budget(contexts);
        assert!(result.contains("Short"));
        assert!(result.contains("Medium length text"));
        assert!(result.contains("This won't fit"));
        // Truncation notice is always appended when entries are dropped
        assert!(result.contains("[Context truncated: dropped"));
        assert!(!result.contains("This definitely exceeds the budget"));
    }

    #[test]
    fn test_apply_budget_within_limit() {
        // The check is result.len() + context.len() + 2 > max_chars
        // So with limit=25 and context=19 chars: 0 + 19 + 2 = 21 <= 25, fits
        let manager = BudgetManager::with_limit(25);
        let contexts = vec!["Exactly twenty char".to_string()]; // 19 chars
        let result = manager.apply_budget(contexts);
        assert_eq!(result, "Exactly twenty char");
    }

    #[test]
    fn test_apply_budget_truncation_with_message() {
        // Truncation message is always added when entries are dropped
        let manager = BudgetManager::with_limit(100);
        let contexts = vec![
            "Short".to_string(),
            "This entry is deliberately made very long so it cannot possibly fit within the remaining budget that we have allocated for context injection purposes".to_string(),
        ];
        let result = manager.apply_budget(contexts);
        assert!(result.contains("Short"));
        assert!(result.contains("[Context truncated: dropped"));
    }

    #[test]
    fn test_apply_budget_truncation_always_shows_message() {
        // Truncation message is always shown when entries are dropped
        let manager = BudgetManager::with_limit(50);
        let contexts = vec![
            "This is a forty char long context!!!".to_string(), // 36 chars
            "Second".to_string(),                               // 6 chars, 36 + 2 + 6 = 44, fits
            "Third exceeds limit".to_string(), // 19 chars, 44 + 2 + 19 = 65 > 50, truncate
        ];
        let result = manager.apply_budget(contexts);
        assert!(result.contains("forty char"));
        assert!(result.contains("Second"));
        assert!(!result.contains("Third exceeds"));
        // Truncation notice is always appended when entries are dropped
        assert!(result.contains("[Context truncated: dropped"));
    }

    // --- Priority-based tests ---

    #[test]
    fn test_prioritized_empty() {
        let manager = BudgetManager::new();
        let result = manager.apply_budget_prioritized(vec![]);
        assert!(result.content.is_empty());
        assert!(result.kept_sources.is_empty());
        assert!(result.dropped_sources.is_empty());
    }

    #[test]
    fn test_prioritized_filters_empty_content() {
        let manager = BudgetManager::new();
        let entries = vec![
            BudgetEntry::new(0.9, String::new(), "empty"),
            BudgetEntry::new(0.5, "valid".to_string(), "test"),
        ];
        let result = manager.apply_budget_prioritized(entries);
        assert_eq!(result.content, "valid");
        assert_eq!(result.kept_sources, vec!["test"]);
    }

    #[test]
    fn test_prioritized_sorts_by_priority() {
        let manager = BudgetManager::new();
        let entries = vec![
            BudgetEntry::new(0.3, "low priority".to_string(), "low"),
            BudgetEntry::new(0.9, "high priority".to_string(), "high"),
            BudgetEntry::new(0.6, "mid priority".to_string(), "mid"),
        ];
        let result = manager.apply_budget_prioritized(entries);
        // High priority should come first
        let high_pos = result.content.find("high priority").unwrap();
        let mid_pos = result.content.find("mid priority").unwrap();
        let low_pos = result.content.find("low priority").unwrap();
        assert!(high_pos < mid_pos);
        assert!(mid_pos < low_pos);
        assert_eq!(result.kept_sources, vec!["high", "mid", "low"]);
    }

    #[test]
    fn test_prioritized_truncation_drops_low_priority() {
        let manager = BudgetManager::with_limit(60);
        let entries = vec![
            BudgetEntry::new(0.3, "low priority content here".to_string(), "low"),
            BudgetEntry::new(0.9, "high priority content".to_string(), "high"),
        ];
        let result = manager.apply_budget_prioritized(entries);
        // High priority should be included
        assert!(result.content.contains("high priority content"));
        // Low priority should be truncated (21 + 2 + 25 = 48 <= 60, actually fits)
        // Let's adjust: make budget tighter
        let manager = BudgetManager::with_limit(30);
        let entries = vec![
            BudgetEntry::new(0.3, "low priority content here".to_string(), "low"),
            BudgetEntry::new(0.9, "high priority content".to_string(), "high"),
        ];
        let result = manager.apply_budget_prioritized(entries);
        assert!(result.content.contains("high priority content"));
        assert!(!result.content.contains("low priority content"));
        assert_eq!(result.dropped_sources, vec!["low"]);
    }

    #[test]
    fn test_prioritized_truncation_message() {
        // "important" = 9 chars. The long entry won't fit, so it gets dropped.
        let manager = BudgetManager::with_limit(100);
        let entries = vec![
            BudgetEntry::new(0.9, "important".to_string(), "high"),
            BudgetEntry::new(0.1, "this is definitely too long to fit and it keeps going and going until it exceeds our budget".to_string(), "low"),
        ];
        let result = manager.apply_budget_prioritized(entries);
        assert!(result.content.contains("important"));
        assert!(result.content.contains("[Context truncated: dropped low]"));
        assert_eq!(result.kept_sources, vec!["high"]);
        assert_eq!(result.dropped_sources, vec!["low"]);
    }
}
