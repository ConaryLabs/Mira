// crates/mira-server/src/llm/context_budget.rs
// Token budget management for non-stateful providers (DeepSeek, Gemini)

use crate::llm::Message;

/// Token budget for non-stateful providers (DeepSeek, Gemini)
/// DeepSeek limit: 131072, we use 110000 as safe margin
pub const CONTEXT_BUDGET: u64 = 110_000;

/// Estimate token count for a string (rough estimate: ~4 chars per token)
pub fn estimate_tokens(text: &str) -> u64 {
    if text.is_empty() {
        return 0;
    }
    // Rough estimate: ~4 characters per token
    (text.len() as f64 / 4.0).ceil() as u64
}

/// Estimate tokens for all messages
pub fn estimate_message_tokens(messages: &[Message]) -> u64 {
    messages
        .iter()
        .map(|m| {
            let content = m.content.as_deref().unwrap_or("");
            let reasoning = m.reasoning_content.as_deref().unwrap_or("");
            estimate_tokens(content) + estimate_tokens(reasoning)
        })
        .sum()
}

/// Truncate messages to fit within the given token budget.
/// Phase 1: removes oldest non-system, non-tool messages.
/// Phase 2: truncates content of oldest tool results (keeps last 3 intact).
pub fn truncate_messages_to_budget(mut messages: Vec<Message>, budget: u64) -> Vec<Message> {
    let total = estimate_message_tokens(&messages);

    if total <= budget {
        return messages;
    }

    let mut current_total = total;

    // Phase 1: Remove oldest non-system, non-tool messages
    while current_total > budget && messages.len() > 1 {
        let mut remove_index = None;
        for (i, msg) in messages.iter().enumerate() {
            if i == 0 {
                continue; // Never remove system message
            }
            if msg.role != "system" && msg.role != "tool" {
                remove_index = Some(i);
                break;
            }
        }

        if let Some(idx) = remove_index {
            let removed = messages.remove(idx);
            let removed_tokens = estimate_tokens(removed.content.as_deref().unwrap_or(""))
                + estimate_tokens(removed.reasoning_content.as_deref().unwrap_or(""));
            current_total = current_total.saturating_sub(removed_tokens);
        } else {
            break;
        }
    }

    // Phase 2: If still over budget, truncate oldest tool result content (keep last 3 intact)
    if current_total > budget {
        let tool_indices: Vec<usize> = messages
            .iter()
            .enumerate()
            .filter(|(_, m)| m.role == "tool")
            .map(|(i, _)| i)
            .collect();

        let protect_count = 3.min(tool_indices.len());
        let truncatable = &tool_indices[..tool_indices.len() - protect_count];

        for &idx in truncatable {
            if current_total <= budget {
                break;
            }
            if let Some(ref content) = messages[idx].content {
                let old_tokens = estimate_tokens(content);
                let summary = "[tool result truncated to fit context budget]";
                messages[idx].content = Some(summary.to_string());
                let new_tokens = estimate_tokens(summary);
                current_total = current_total.saturating_sub(old_tokens.saturating_sub(new_tokens));
            }
        }
    }

    messages
}

/// Truncate messages using the default CONTEXT_BUDGET constant.
pub fn truncate_messages_to_default_budget(messages: Vec<Message>) -> Vec<Message> {
    truncate_messages_to_budget(messages, CONTEXT_BUDGET)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_estimate_tokens_empty() {
        assert_eq!(estimate_tokens(""), 0);
    }

    #[test]
    fn test_estimate_tokens_short() {
        // ~4 chars per token
        assert_eq!(estimate_tokens("hello"), 2); // "hello" is 5 chars / 4 = 1.25 -> 2
    }

    #[test]
    fn test_estimate_messages_empty() {
        assert_eq!(estimate_message_tokens(&[]), 0);
    }

    #[test]
    fn test_estimate_messages_with_content() {
        let messages = vec![Message::user("hello world")];
        // "hello world" is 11 chars / 4 = 2.75 -> 3
        assert_eq!(estimate_message_tokens(&messages), 3);
    }

    #[test]
    fn test_truncate_no_truncation_needed() {
        let messages = vec![
            Message::system("You are a helpful assistant"),
            Message::user("Hello"),
        ];
        let result = truncate_messages_to_budget(messages.clone(), CONTEXT_BUDGET);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_truncate_preserves_system_message() {
        let large_content = "x".repeat(500_000); // ~125k tokens

        let messages = vec![
            Message::system("You are a helpful assistant"),
            Message::user(&large_content),
        ];

        let result = truncate_messages_to_budget(messages, CONTEXT_BUDGET);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].role, "system");
    }

    #[test]
    fn test_truncate_keeps_minimum_messages() {
        let messages = vec![Message::system("System"), Message::user("User")];

        let result = truncate_messages_to_budget(messages, CONTEXT_BUDGET);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_truncate_with_explicit_budget() {
        let messages = vec![
            Message::system("System"),
            Message::user(&"x".repeat(1000)), // ~250 tokens
        ];
        // Budget of 100 tokens should trigger truncation
        let result = truncate_messages_to_budget(messages, 100);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].role, "system");
    }

    #[test]
    fn test_truncate_tool_results_phase2() {
        let messages = vec![
            Message::system("System"),
            Message::tool_result("1", "x".repeat(200_000)), // ~50k tokens
            Message::tool_result("2", "y".repeat(200_000)), // ~50k tokens
            Message::tool_result("3", "z".repeat(200_000)), // ~50k tokens
            Message::tool_result("4", "w".repeat(200_000)), // ~50k tokens
        ];
        // Budget that forces phase 2 truncation (no non-tool messages to remove)
        let result = truncate_messages_to_budget(messages, 80_000);
        // First tool result should be truncated, last 3 protected
        assert!(result[1].content.as_ref().unwrap().contains("truncated"));
        assert_eq!(result[4].content.as_ref().unwrap().len(), 200_000);
    }

    #[test]
    fn test_default_budget_wrapper() {
        let messages = vec![Message::system("System"), Message::user("Hello")];
        let result = truncate_messages_to_default_budget(messages);
        assert_eq!(result.len(), 2);
    }
}
