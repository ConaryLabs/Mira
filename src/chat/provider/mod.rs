//! Provider abstraction for DeepSeek chat
//!
//! DeepSeek V3.2 as the primary model with:
//! - Unified streaming interface
//! - Tool calling support
//! - Reasoning capabilities

mod capabilities;
mod deepseek;
mod types;

pub use capabilities::{Capabilities, StateMode, UsageReporting};
pub use deepseek::DeepSeekProvider;
pub use types::*;

use anyhow::Result;
use async_trait::async_trait;
use tokio::sync::mpsc;

/// Unified provider trait for LLM backends
#[async_trait]
pub trait Provider: Send + Sync {
    /// Get provider capabilities
    fn capabilities(&self) -> &Capabilities;

    /// Create a streaming chat completion
    async fn create_stream(
        &self,
        request: ChatRequest,
    ) -> Result<mpsc::Receiver<StreamEvent>>;

    /// Create a non-streaming chat completion
    async fn create(&self, request: ChatRequest) -> Result<ChatResponse>;

    /// Continue a conversation with tool results (streaming)
    async fn continue_with_tools_stream(
        &self,
        request: ToolContinueRequest,
    ) -> Result<mpsc::Receiver<StreamEvent>>;

    /// Get the provider name for logging
    fn name(&self) -> &'static str;
}

/// Model specification with limits and features
#[derive(Debug, Clone)]
pub struct ModelSpec {
    pub id: String,
    pub display_name: String,
    pub max_context_tokens: u32,
    pub max_output_tokens: u32,
    pub default_output_tokens: u32,
    pub supports_tools: bool,
    pub supports_reasoning: bool,
    pub input_cost_per_million: f64,
    pub output_cost_per_million: f64,
}

impl ModelSpec {
    /// GPT-5.2 specification
    pub fn gpt_5_2() -> Self {
        Self {
            id: "gpt-5.2".into(),
            display_name: "GPT-5.2".into(),
            max_context_tokens: 400_000,
            max_output_tokens: 16_000,
            default_output_tokens: 4_000,
            supports_tools: true,
            supports_reasoning: true,
            input_cost_per_million: 2.50,  // with caching can be much lower
            output_cost_per_million: 10.00,
        }
    }

    /// DeepSeek Chat specification
    pub fn deepseek_chat() -> Self {
        Self {
            id: "deepseek-chat".into(),
            display_name: "DeepSeek Chat".into(),
            max_context_tokens: 128_000,
            max_output_tokens: 8_000,
            default_output_tokens: 4_000,
            supports_tools: true,
            supports_reasoning: false,
            input_cost_per_million: 0.27,
            output_cost_per_million: 0.41,
        }
    }

    /// DeepSeek Reasoner specification
    pub fn deepseek_reasoner() -> Self {
        Self {
            id: "deepseek-reasoner".into(),
            display_name: "DeepSeek Reasoner".into(),
            max_context_tokens: 128_000,
            max_output_tokens: 64_000,
            default_output_tokens: 32_000,
            supports_tools: false,  // Reasoner has no tool support
            supports_reasoning: true,
            input_cost_per_million: 0.55,
            output_cost_per_million: 2.19,
        }
    }

    /// Calculate cost for a request
    pub fn calculate_cost(&self, input_tokens: u32, output_tokens: u32) -> f64 {
        let input_cost = (input_tokens as f64 / 1_000_000.0) * self.input_cost_per_million;
        let output_cost = (output_tokens as f64 / 1_000_000.0) * self.output_cost_per_million;
        input_cost + output_cost
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_specs() {
        let gpt = ModelSpec::gpt_5_2();
        assert_eq!(gpt.max_context_tokens, 400_000);
        assert!(gpt.supports_tools);

        let ds_chat = ModelSpec::deepseek_chat();
        assert_eq!(ds_chat.max_output_tokens, 8_000);
        assert!(ds_chat.supports_tools);

        let ds_reason = ModelSpec::deepseek_reasoner();
        assert_eq!(ds_reason.max_output_tokens, 64_000);
        assert!(!ds_reason.supports_tools);
    }

    #[test]
    fn test_cost_calculation() {
        let ds = ModelSpec::deepseek_chat();
        // 1M input, 100k output
        let cost = ds.calculate_cost(1_000_000, 100_000);
        // 0.27 + 0.041 = 0.311
        assert!((cost - 0.311).abs() < 0.001);
    }
}
