// src/operations/engine/simple_mode.rs
// Simple Mode - Fast path for trivial requests that don't need full orchestration
// Inspired by Claude Code's single-loop architecture

use anyhow::Result;
use serde_json::Value;
use tracing::info;

use crate::llm::provider::gpt5::Gpt5Provider;
use crate::llm::provider::Message;

/// Detects if a request is "simple" and can skip full orchestration
pub struct SimpleModeDetector;

impl SimpleModeDetector {
    /// Check if request is simple enough for fast path
    pub fn is_simple(user_content: &str) -> bool {
        let content_lower = user_content.to_lowercase();
        let word_count = user_content.split_whitespace().count();

        // Simple if:
        // 1. Very short (< 15 words)
        // 2. Doesn't mention code generation keywords
        // 3. Doesn't mention file operations
        // 4. Doesn't mention refactoring/debugging

        if word_count > 15 {
            return false;
        }

        // Keywords that indicate complex work
        let complex_keywords = [
            "create", "generate", "write", "build", "implement",
            "refactor", "fix", "debug", "change", "update",
            "add feature", "new feature", "modify", "rewrite",
            "file", "function", "class", "component",
        ];

        for keyword in complex_keywords {
            if content_lower.contains(keyword) {
                return false;
            }
        }

        // Simple question keywords
        let simple_keywords = [
            "what", "how", "why", "when", "where",
            "explain", "describe", "show", "tell", "help",
            "?", // Questions are usually simple
        ];

        for keyword in simple_keywords {
            if content_lower.contains(keyword) {
                return true;
            }
        }

        // Default to complex if unsure
        false
    }

    /// Get confidence score (0.0 = definitely complex, 1.0 = definitely simple)
    pub fn simplicity_score(user_content: &str) -> f64 {
        let content_lower = user_content.to_lowercase();
        let word_count = user_content.split_whitespace().count();

        let mut score: f64 = 0.5; // Start neutral

        // Length bonus (shorter = simpler)
        if word_count < 5 {
            score += 0.3;
        } else if word_count < 10 {
            score += 0.15;
        } else if word_count > 20 {
            score -= 0.3;
        }

        // Question mark = likely simple
        if content_lower.contains('?') {
            score += 0.2;
        }

        // Command words = complex
        if content_lower.starts_with("create ")
            || content_lower.starts_with("generate ")
            || content_lower.starts_with("write ")
            || content_lower.starts_with("implement ") {
            score -= 0.5;
        }

        // Information words = simple
        if content_lower.starts_with("what ")
            || content_lower.starts_with("how ")
            || content_lower.starts_with("explain ")
            || content_lower.starts_with("show ") {
            score += 0.3;
        }

        score.clamp(0.0, 1.0)
    }
}

/// Simple mode executor - minimal overhead path
pub struct SimpleModeExecutor {
    gpt5: Gpt5Provider,
}

impl SimpleModeExecutor {
    pub fn new(gpt5: Gpt5Provider) -> Self {
        Self { gpt5 }
    }

    /// Execute simple request without full orchestration
    ///
    /// Skips:
    /// - Operation tracking
    /// - Lifecycle events
    /// - Context loading (uses minimal system prompt)
    /// - Tool delegation (no tools available)
    /// - Artifact management
    ///
    /// Returns: Direct text response from GPT-5
    pub async fn execute_simple(&self, user_content: &str) -> Result<String> {
        info!("[SIMPLE MODE] Fast path execution");

        let system_prompt = "You are Mira, an AI coding assistant. \
            Provide concise, helpful answers. \
            If the question requires code generation or file operations, \
            inform the user that this requires a more detailed request.".to_string();

        let messages = vec![Message::user(user_content.to_string())];

        // Simple chat - no tools
        let response = self
            .gpt5
            .create_with_tools(messages, system_prompt, vec![], None)
            .await?;

        Ok(response.content)
    }

    /// Execute with minimal tools (read-only operations)
    pub async fn execute_with_readonly_tools(
        &self,
        user_content: &str,
        tools: Vec<Value>,
    ) -> Result<String> {
        info!("[SIMPLE MODE] Fast path with read-only tools");

        let system_prompt = "You are Mira, an AI coding assistant. \
            You have access to read-only file operations. \
            Provide concise answers. If code generation is needed, \
            inform the user to make a more detailed request.".to_string();

        let messages = vec![Message::user(user_content.to_string())];

        // Chat with minimal tools
        let response = self
            .gpt5
            .create_with_tools(messages, system_prompt, tools, None)
            .await?;

        Ok(response.content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_detection() {
        // Simple requests
        assert!(SimpleModeDetector::is_simple("What is Rust?"));
        assert!(SimpleModeDetector::is_simple("How does async work?"));
        assert!(SimpleModeDetector::is_simple("Explain closures"));
        assert!(SimpleModeDetector::is_simple("help me understand this"));

        // Complex requests
        assert!(!SimpleModeDetector::is_simple("Create a new authentication system"));
        assert!(!SimpleModeDetector::is_simple("Generate a REST API for users"));
        assert!(!SimpleModeDetector::is_simple("Refactor the error handling in src/main.rs"));
        assert!(!SimpleModeDetector::is_simple("Write unit tests for the payment module"));
    }

    #[test]
    fn test_simplicity_scores() {
        // Very simple
        assert!(SimpleModeDetector::simplicity_score("What is this?") > 0.7);
        assert!(SimpleModeDetector::simplicity_score("How?") > 0.8);

        // Moderately simple
        assert!(SimpleModeDetector::simplicity_score("Explain async await") > 0.5);

        // Complex
        assert!(SimpleModeDetector::simplicity_score("Create a new feature") < 0.3);
        assert!(SimpleModeDetector::simplicity_score("Generate comprehensive API documentation for all modules") < 0.2);
    }
}
