// crates/mira-server/src/context/budget.rs
// Token budget management for context injection with priority-based sorting

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
            priority,
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
            max_chars: 1500, // ~375 tokens - balanced for DeepSeek without compaction
        }
    }

    /// Create with custom character limit
    pub fn with_limit(max_chars: usize) -> Self {
        Self { max_chars }
    }

    /// Apply token budget to prioritized context entries.
    /// Entries are sorted by priority descending before applying the char limit,
    /// so the most important context is kept when truncation is needed.
    pub fn apply_budget_prioritized(&self, entries: Vec<BudgetEntry>) -> String {
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
            return String::new();
        }

        let mut result = String::new();
        for entry in entries {
            if result.len() + entry.content.len() + 2 > self.max_chars {
                // Truncate and add ellipsis
                let remaining = self.max_chars.saturating_sub(result.len());
                if remaining > 10 {
                    result.push_str("\n\n[Context truncated due to token limit]");
                }
                break;
            }
            if !result.is_empty() {
                result.push_str("\n\n");
            }
            result.push_str(&entry.content);
        }

        result
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
        self.apply_budget_prioritized(entries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_budget_manager_default() {
        let manager = BudgetManager::new();
        assert_eq!(manager.max_chars, 1500);
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
        let manager = BudgetManager::with_limit(50);
        let contexts = vec![
            "Short".to_string(),                                // 5 chars
            "Medium length text".to_string(),                   // 18 chars, total 25 with separator
            "This won't fit because it's too long".to_string(), // Would exceed limit
        ];
        let result = manager.apply_budget(contexts);
        assert!(result.contains("Short"));
        assert!(result.contains("Medium length text"));
        assert!(result.contains("[Context truncated due to token limit]"));
        assert!(!result.contains("This won't fit"));
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
        // Truncation message is only added if remaining > 10
        // So we need to have at least 11 chars remaining when truncating
        let manager = BudgetManager::with_limit(100);
        let contexts = vec![
            "First context".to_string(),                          // 13 chars
            "Second context".to_string(), // 14 chars, total = 13 + 2 + 14 = 29
            "Third".to_string(),          // 5 chars, total = 29 + 2 + 5 = 36
            "This is a very long fourth context".to_string(), // 35 chars, total = 36 + 2 + 35 = 73 > 100? No, fits
            "Fifth really long context that exceeds".to_string(), // 39 chars, total = 73 + 2 + 39 = 114 > 100, truncate
        ];
        let result = manager.apply_budget(contexts);
        assert!(result.contains("First context"));
        assert!(result.contains("Third"));
        assert!(result.contains("fourth context"));
        // remaining = 100 - 73 = 27, which is > 10, so message is added
        assert!(result.contains("[Context truncated due to token limit]"));
        assert!(!result.contains("Fifth"));
    }

    #[test]
    fn test_apply_budget_truncation_no_message_if_tight() {
        // If remaining <= 10 after the last context that fits, no truncation message
        let manager = BudgetManager::with_limit(50);
        let contexts = vec![
            "This is a forty char long context!!!".to_string(), // 36 chars
            "Second".to_string(),                               // 6 chars, 36 + 2 + 6 = 44, fits
            "Third exceeds limit".to_string(), // 19 chars, 44 + 2 + 19 = 65 > 50, truncate
        ];
        let result = manager.apply_budget(contexts);
        assert!(result.contains("forty char"));
        assert!(result.contains("Second"));
        // remaining = 50 - 44 = 6, which is NOT > 10, so no message
        assert!(!result.contains("[Context truncated"));
        assert!(!result.contains("Third"));
    }

    // --- Priority-based tests ---

    #[test]
    fn test_prioritized_empty() {
        let manager = BudgetManager::new();
        let result = manager.apply_budget_prioritized(vec![]);
        assert!(result.is_empty());
    }

    #[test]
    fn test_prioritized_filters_empty_content() {
        let manager = BudgetManager::new();
        let entries = vec![
            BudgetEntry::new(0.9, String::new(), "empty"),
            BudgetEntry::new(0.5, "valid".to_string(), "test"),
        ];
        let result = manager.apply_budget_prioritized(entries);
        assert_eq!(result, "valid");
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
        let high_pos = result.find("high priority").unwrap();
        let mid_pos = result.find("mid priority").unwrap();
        let low_pos = result.find("low priority").unwrap();
        assert!(high_pos < mid_pos);
        assert!(mid_pos < low_pos);
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
        assert!(result.contains("high priority content"));
        // Low priority should be truncated (21 + 2 + 25 = 48 <= 60, actually fits)
        // Let's adjust: make budget tighter
        let manager = BudgetManager::with_limit(30);
        let entries = vec![
            BudgetEntry::new(0.3, "low priority content here".to_string(), "low"),
            BudgetEntry::new(0.9, "high priority content".to_string(), "high"),
        ];
        let result = manager.apply_budget_prioritized(entries);
        assert!(result.contains("high priority content"));
        assert!(!result.contains("low priority content"));
    }

    #[test]
    fn test_prioritized_truncation_message() {
        let manager = BudgetManager::with_limit(40);
        let entries = vec![
            BudgetEntry::new(0.9, "important".to_string(), "high"),
            BudgetEntry::new(0.1, "this is definitely too long to fit".to_string(), "low"),
        ];
        let result = manager.apply_budget_prioritized(entries);
        assert!(result.contains("important"));
        assert!(result.contains("[Context truncated due to token limit]"));
    }
}
