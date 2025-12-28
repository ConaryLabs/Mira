//! Provider abstraction for chat models
//!
//! Supports multiple LLM backends:
//! - Gemini 3 Flash (default, cheap) / Pro (complex reasoning)
//! - GPT-5.2 via Responses API
//! - Unified streaming interface
//! - Tool calling support
//! - Reasoning capabilities

#![allow(dead_code)] // Provider infrastructure (some items for future use)

pub mod batch;
mod capabilities;
pub mod file_search;
mod gemini;
pub mod responses;
mod types;

pub use capabilities::Capabilities;
pub use file_search::{FileSearchClient, FileSearchStore, CustomMetadata, Operation};
pub use batch::{BatchClient, BatchRequest, BatchResponse, BatchJob, BatchState, BatchError, build_batch_request};
pub use gemini::CachedContent;
pub use gemini::FileSearchConfig;
pub use gemini::GeminiChatProvider;
pub use gemini::GeminiModel;
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

    /// Gemini 3 Flash specification (default, cheap, fast)
    pub fn gemini_3_flash() -> Self {
        Self {
            id: "gemini-3-flash-preview".into(),
            display_name: "Gemini 3 Flash".into(),
            max_context_tokens: 1_000_000,
            max_output_tokens: 65_536,
            default_output_tokens: 16_000,
            supports_tools: true,
            supports_reasoning: true,  // Supports thinkingConfig
            input_cost_per_million: 0.50,
            output_cost_per_million: 3.00,
        }
    }

    /// Gemini 3 Pro specification (complex reasoning, advanced planning)
    pub fn gemini_3_pro() -> Self {
        Self {
            id: "gemini-3-pro-preview".into(),
            display_name: "Gemini 3 Pro".into(),
            max_context_tokens: 1_000_000,
            max_output_tokens: 65_536,
            default_output_tokens: 16_000,
            supports_tools: true,
            supports_reasoning: true,  // Supports thinkingConfig
            input_cost_per_million: 2.00,
            output_cost_per_million: 12.00,
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

        let flash = ModelSpec::gemini_3_flash();
        assert_eq!(flash.max_context_tokens, 1_000_000);
        assert!(flash.supports_tools);

        let pro = ModelSpec::gemini_3_pro();
        assert_eq!(pro.max_context_tokens, 1_000_000);
        assert!(pro.supports_tools);
    }

    #[test]
    fn test_cost_calculation() {
        let flash = ModelSpec::gemini_3_flash();
        // 1M input, 100k output
        let cost = flash.calculate_cost(1_000_000, 100_000);
        // 0.50 + 0.30 = 0.80
        assert!((cost - 0.80).abs() < 0.001);
    }
}
