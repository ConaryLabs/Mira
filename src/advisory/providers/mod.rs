//! Advisory Providers - LLM provider implementations
//!
//! Each provider is in its own module for maintainability.
//! Many types/methods are designed for future use (streaming, tools).

#![allow(dead_code)]

mod gpt;
mod opus;
mod gemini;
mod reasoner;

pub use gpt::{GptProvider, ResponsesInputItem};
pub use opus::{OpusProvider, OpusInputItem, OpusToolUse, AnthropicResponseBlock};
pub use gemini::{
    GeminiProvider, GeminiInputItem, GeminiContent, GeminiPart, GeminiPartResponse,
    GeminiFunctionCallResponse, GeminiTextPart, GeminiFunctionCallPart, GeminiFunctionCall,
    GeminiFunctionResponsePart, GeminiFunctionResponse,
};
pub use reasoner::ReasonerProvider;

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

// ============================================================================
// Constants
// ============================================================================

pub const DOTENV_PATH: &str = "/home/peter/Mira/.env";
pub const DEFAULT_TIMEOUT_SECS: u64 = 60;
pub const REASONER_TIMEOUT_SECS: u64 = 180;

// ============================================================================
// Core Types
// ============================================================================

/// Available advisory models
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AdvisoryModel {
    Gpt52,
    Opus45,
    Gemini3Pro,
    DeepSeekReasoner,
}

impl AdvisoryModel {
    pub fn as_str(&self) -> &'static str {
        match self {
            AdvisoryModel::Gpt52 => "gpt-5.2",
            AdvisoryModel::Opus45 => "opus-4.5",
            AdvisoryModel::Gemini3Pro => "gemini-3-pro",
            AdvisoryModel::DeepSeekReasoner => "deepseek-reasoner",
        }
    }

    /// Parse model from provider string
    pub fn from_provider_str(s: &str) -> Option<Self> {
        match s {
            "gpt-5.2" => Some(AdvisoryModel::Gpt52),
            "opus-4.5" => Some(AdvisoryModel::Opus45),
            "gemini-3-pro" => Some(AdvisoryModel::Gemini3Pro),
            "deepseek-reasoner" => Some(AdvisoryModel::DeepSeekReasoner),
            _ => None,
        }
    }

    /// Get display name for UI
    pub fn display_name(&self) -> &'static str {
        match self {
            AdvisoryModel::Gpt52 => "GPT-5.2",
            AdvisoryModel::Opus45 => "Opus 4.5",
            AdvisoryModel::Gemini3Pro => "Gemini 3 Pro",
            AdvisoryModel::DeepSeekReasoner => "DeepSeek Reasoner",
        }
    }

    /// Get color for UI (hex without #)
    pub fn color(&self) -> &'static str {
        match self {
            AdvisoryModel::Gpt52 => "10a37f",        // OpenAI green
            AdvisoryModel::Opus45 => "d97706",       // Anthropic orange/amber
            AdvisoryModel::Gemini3Pro => "4285f4",   // Google blue
            AdvisoryModel::DeepSeekReasoner => "6366f1", // Indigo
        }
    }

    /// Get metadata as JSON for API responses
    pub fn metadata(&self) -> serde_json::Value {
        serde_json::json!({
            "id": self.as_str(),
            "display_name": self.display_name(),
            "color": self.color(),
        })
    }

    /// Get all council models (excludes DeepSeek which is the moderator)
    pub fn council_models() -> Vec<Self> {
        vec![Self::Gpt52, Self::Gemini3Pro, Self::Opus45]
    }

    /// Get model metadata map for all council models
    pub fn council_metadata() -> serde_json::Value {
        let mut map = serde_json::Map::new();
        for model in Self::council_models() {
            map.insert(model.as_str().to_string(), model.metadata());
        }
        serde_json::Value::Object(map)
    }
}

/// Request to an advisory provider
#[derive(Debug, Clone)]
pub struct AdvisoryRequest {
    /// The message/question
    pub message: String,
    /// System prompt / instructions
    pub system: Option<String>,
    /// Previous conversation turns (for multi-turn)
    pub history: Vec<AdvisoryMessage>,
    /// Enable tool calling (if supported by provider)
    pub enable_tools: bool,
}

impl AdvisoryRequest {
    /// Create a simple request without tools
    pub fn simple(message: String) -> Self {
        Self {
            message,
            system: None,
            history: vec![],
            enable_tools: false,
        }
    }

    /// Create a request with tools enabled
    pub fn with_tools(message: String) -> Self {
        Self {
            message,
            system: None,
            history: vec![],
            enable_tools: true,
        }
    }
}

/// A message in advisory conversation history
#[derive(Debug, Clone)]
pub struct AdvisoryMessage {
    pub role: AdvisoryRole,
    pub content: String,
}

/// Role in advisory conversation
#[derive(Debug, Clone, Copy)]
pub enum AdvisoryRole {
    User,
    Assistant,
}

/// Response from an advisory provider
#[derive(Debug, Clone)]
pub struct AdvisoryResponse {
    pub text: String,
    pub usage: Option<AdvisoryUsage>,
    pub model: AdvisoryModel,
    /// Tool calls requested by the model (if any)
    pub tool_calls: Vec<ToolCallRequest>,
}

/// A tool call request from the model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallRequest {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

/// Token usage information with cache tracking
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct AdvisoryUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub reasoning_tokens: u32,
    /// Tokens read from cache (charged at discounted rate)
    pub cache_read_tokens: u32,
    /// Tokens written to cache (charged at premium rate for some providers)
    pub cache_write_tokens: u32,
}

impl AdvisoryUsage {
    /// Calculate cost in USD for this usage based on the model
    pub fn cost_usd(&self, model: AdvisoryModel) -> f64 {
        model.calculate_cost(self)
    }
}

// ============================================================================
// Pricing (as of 2025-12-25)
// ============================================================================

/// Pricing per 1M tokens in USD
#[derive(Debug, Clone, Copy)]
pub struct ModelPricing {
    pub input_per_m: f64,
    pub output_per_m: f64,
    /// Cached input tokens rate (per 1M)
    pub cache_read_per_m: f64,
    /// Cache write rate (per 1M) - some providers charge extra for cache creation
    pub cache_write_per_m: f64,
    /// Reasoning/thinking tokens rate (per 1M) - usually same as output
    pub reasoning_per_m: f64,
}

impl AdvisoryModel {
    /// Get pricing for this model (as of 2025-12-25)
    pub fn pricing(&self) -> ModelPricing {
        match self {
            // GPT-5.2: $1.75 input, $14 output, 90% cache discount
            // Source: https://openai.com/api/pricing/
            AdvisoryModel::Gpt52 => ModelPricing {
                input_per_m: 1.75,
                output_per_m: 14.0,
                cache_read_per_m: 0.175,  // 90% discount
                cache_write_per_m: 1.75,   // same as input
                reasoning_per_m: 14.0,     // reasoning billed as output
            },
            // Gemini 3 Pro: $2 input, $12 output (≤200K context)
            // Source: https://ai.google.dev/gemini-api/docs/pricing
            AdvisoryModel::Gemini3Pro => ModelPricing {
                input_per_m: 2.0,
                output_per_m: 12.0,
                cache_read_per_m: 2.0,     // no cache discount in preview
                cache_write_per_m: 2.0,
                reasoning_per_m: 12.0,
            },
            // DeepSeek V3.2 (reasoner): $0.28 input (miss), $0.42 output, $0.028 cache hit
            // Source: https://api-docs.deepseek.com/quick_start/pricing
            AdvisoryModel::DeepSeekReasoner => ModelPricing {
                input_per_m: 0.28,
                output_per_m: 0.42,
                cache_read_per_m: 0.028,   // 90% cache discount
                cache_write_per_m: 0.28,
                reasoning_per_m: 0.42,
            },
            // Claude Opus 4.5: $5 input, $25 output, 90% cache read discount
            // Source: https://docs.anthropic.com/en/docs/about-claude/pricing
            AdvisoryModel::Opus45 => ModelPricing {
                input_per_m: 5.0,
                output_per_m: 25.0,
                cache_read_per_m: 0.5,     // 90% discount (0.1x)
                cache_write_per_m: 6.25,   // 1.25x for 5-min cache
                reasoning_per_m: 25.0,
            },
        }
    }

    /// Calculate cost in USD for given usage
    pub fn calculate_cost(&self, usage: &AdvisoryUsage) -> f64 {
        let pricing = self.pricing();
        let m = 1_000_000.0;

        // Regular input tokens (excluding cached)
        let regular_input = usage.input_tokens.saturating_sub(usage.cache_read_tokens);
        let input_cost = (regular_input as f64 / m) * pricing.input_per_m;

        // Cache read tokens
        let cache_read_cost = (usage.cache_read_tokens as f64 / m) * pricing.cache_read_per_m;

        // Cache write tokens
        let cache_write_cost = (usage.cache_write_tokens as f64 / m) * pricing.cache_write_per_m;

        // Output tokens
        let output_cost = (usage.output_tokens as f64 / m) * pricing.output_per_m;

        // Reasoning tokens (if separate from output)
        let reasoning_cost = (usage.reasoning_tokens as f64 / m) * pricing.reasoning_per_m;

        input_cost + cache_read_cost + cache_write_cost + output_cost + reasoning_cost
    }
}

/// Streaming event from advisory provider
#[derive(Debug, Clone)]
pub enum AdvisoryEvent {
    /// Text content delta
    TextDelta(String),
    /// Reasoning/thinking delta (for reasoning models)
    ReasoningDelta(String),
    /// Usage information
    Usage(AdvisoryUsage),
    /// Stream complete
    Done,
    /// Error occurred
    Error(String),
}

/// Provider capabilities
#[derive(Debug, Clone)]
pub struct AdvisoryCapabilities {
    pub supports_streaming: bool,
    pub supports_reasoning: bool,
    pub supports_tools: bool,
    pub max_context_tokens: u32,
    pub max_output_tokens: u32,
}

// ============================================================================
// Provider Trait
// ============================================================================

/// Core advisory provider trait
#[async_trait]
pub trait AdvisoryProvider: Send + Sync {
    /// Provider name for logging/identification
    fn name(&self) -> &'static str;

    /// Which model this provider represents
    fn model(&self) -> AdvisoryModel;

    /// Get provider capabilities
    fn capabilities(&self) -> &AdvisoryCapabilities;

    /// Create non-streaming response (blocks until complete)
    async fn complete(&self, request: AdvisoryRequest) -> Result<AdvisoryResponse>;

    /// Stream response with events sent to the provided channel
    /// Returns the full text when complete
    async fn stream(
        &self,
        request: AdvisoryRequest,
        tx: mpsc::Sender<AdvisoryEvent>,
    ) -> Result<String>;
}

// ============================================================================
// Environment helpers
// ============================================================================

pub fn get_env_var(name: &str) -> Option<String> {
    // First try env var
    if let Ok(val) = std::env::var(name) {
        return Some(val);
    }

    // Fallback: read from .env file
    if let Ok(contents) = std::fs::read_to_string(DOTENV_PATH) {
        let prefix = format!("{}=", name);
        for line in contents.lines() {
            if let Some(value) = line.strip_prefix(&prefix) {
                return Some(value.trim().to_string());
            }
        }
    }

    None
}
