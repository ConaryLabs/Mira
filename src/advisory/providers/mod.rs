//! Advisory Providers - LLM provider implementations
//!
//! Each provider is in its own module for maintainability.
//! Many types/methods are designed for future use (streaming, tools).

#![allow(dead_code)]

mod gpt;
mod opus;
mod gemini;
mod reasoner;

pub use gpt::GptProvider;
pub use opus::OpusProvider;
pub use gemini::GeminiProvider;
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

/// Token usage information
#[derive(Debug, Clone, Default)]
pub struct AdvisoryUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub reasoning_tokens: u32,
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
