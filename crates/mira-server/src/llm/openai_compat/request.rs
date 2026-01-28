// crates/mira-server/src/llm/openai_compat/request.rs
// OpenAI-compatible chat request builder

use crate::llm::{Message, Tool};
use serde::Serialize;

/// Thinking mode configuration (GLM-specific, ignored by DeepSeek)
#[derive(Debug, Clone, Serialize)]
pub struct ThinkingConfig {
    pub enable_thinking: bool,
    pub budget_tokens: u32,
}

/// Chat completion request (OpenAI-compatible format)
#[derive(Debug, Serialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Tool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<String>, // "auto" | "required" | "none"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<ThinkingConfig>, // GLM-specific
}

impl ChatRequest {
    /// Create a new chat request with required fields
    pub fn new(model: impl Into<String>, messages: Vec<Message>) -> Self {
        Self {
            model: model.into(),
            messages,
            tools: None,
            tool_choice: None,
            max_tokens: None,
            temperature: None,
            thinking: None,
        }
    }

    /// Set tools for function calling
    pub fn with_tools(mut self, tools: Option<Vec<Tool>>) -> Self {
        self.tools = tools;
        if self.tools.is_some() {
            self.tool_choice = Some("auto".into());
        }
        self
    }

    /// Set maximum output tokens
    pub fn with_max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = Some(max_tokens);
        self
    }

    /// Set temperature (0.0 to 2.0)
    pub fn with_temperature(mut self, temperature: f32) -> Self {
        self.temperature = Some(temperature);
        self
    }

    /// Enable GLM thinking mode
    pub fn with_thinking(mut self, enabled: bool, budget_tokens: u32) -> Self {
        if enabled {
            self.thinking = Some(ThinkingConfig {
                enable_thinking: true,
                budget_tokens,
            });
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chat_request_new() {
        let req = ChatRequest::new("test-model", vec![]);
        assert_eq!(req.model, "test-model");
        assert!(req.messages.is_empty());
        assert!(req.tools.is_none());
    }

    #[test]
    fn test_chat_request_builder() {
        let req = ChatRequest::new("model", vec![])
            .with_max_tokens(1000)
            .with_temperature(0.5);
        assert_eq!(req.max_tokens, Some(1000));
        assert_eq!(req.temperature, Some(0.5));
    }

    #[test]
    fn test_chat_request_with_thinking() {
        let req = ChatRequest::new("model", vec![]).with_thinking(true, 8192);
        assert!(req.thinking.is_some());
        let thinking = req.thinking.unwrap();
        assert!(thinking.enable_thinking);
        assert_eq!(thinking.budget_tokens, 8192);
    }
}
