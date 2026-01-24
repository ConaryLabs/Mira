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
    messages.iter().map(|m| {
        let content = m.content.as_deref().unwrap_or("");
        let reasoning = m.reasoning_content.as_deref().unwrap_or("");
        estimate_tokens(content) + estimate_tokens(reasoning)
    }).sum()
}

/// Truncate messages to fit budget
/// Returns truncated messages list (oldest non-system messages removed first)
pub fn truncate_messages_to_budget(mut messages: Vec<Message>) -> Vec<Message> {
    let total = estimate_message_tokens(&messages);

    if total <= CONTEXT_BUDGET {
        return messages;
    }

    let mut current_total = total;

    // Remove oldest non-system messages until we fit the budget
    // Keep at least system message + last user message
    while current_total > CONTEXT_BUDGET && messages.len() > 1 {
        // Find the oldest non-system, non-tool message (after system at index 0)
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
            let removed_tokens = estimate_tokens(
                removed.content.as_deref().unwrap_or("")
            ) + estimate_tokens(
                removed.reasoning_content.as_deref().unwrap_or("")
            );
            current_total = current_total.saturating_sub(removed_tokens);
        } else {
            break;
        }
    }

    messages
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
        let messages = vec![
            Message::user("hello world"),
        ];
        // "hello world" is 11 chars / 4 = 2.75 -> 3
        assert_eq!(estimate_message_tokens(&messages), 3);
    }

    #[test]
    fn test_truncate_no_truncation_needed() {
        let messages = vec![
            Message::system("You are a helpful assistant"),
            Message::user("Hello"),
        ];
        let result = truncate_messages_to_budget(messages.clone());
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_truncate_preserves_system_message() {
        // Create a large message that would exceed budget
        // Need ~450k characters to exceed 110k token budget
        let large_content = "x".repeat(500_000); // ~125k tokens

        let messages = vec![
            Message::system("You are a helpful assistant"),
            Message::user(&large_content),
        ];

        let result = truncate_messages_to_budget(messages);
        // System message should be preserved, user message removed
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].role, "system");
    }

    #[test]
    fn test_truncate_keeps_minimum_messages() {
        let messages = vec![
            Message::system("System"),
            Message::user("User"),
        ];

        let result = truncate_messages_to_budget(messages);
        // Should keep both if under budget
        assert_eq!(result.len(), 2);
    }
}
