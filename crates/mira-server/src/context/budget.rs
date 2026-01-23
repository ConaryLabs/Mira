// crates/mira-server/src/context/budget.rs
// Token budget management for context injection

pub struct BudgetManager {
    max_chars: usize,
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

    /// Apply token budget to collected contexts
    pub fn apply_budget(&self, contexts: Vec<String>) -> String {
        // Filter out empty contexts
        let non_empty: Vec<String> = contexts.into_iter().filter(|c| !c.is_empty()).collect();

        if non_empty.is_empty() {
            return String::new();
        }

        // Simple concatenation with character limit
        let mut result = String::new();
        for context in non_empty {
            if result.len() + context.len() + 2 > self.max_chars {
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
            result.push_str(&context);
        }

        result
    }
}