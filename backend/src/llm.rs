// src/llm.rs
// LLM types and embedding provider for vector search
// In MCP mode, Claude Code handles chat LLM calls. Mira provides:
// - EmbeddingHead: Qdrant collection identifiers
// - EmbeddingProvider: Vector embedding for semantic search
// - Message types: For internal analysis features (pattern detection, etc.)

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Embedding head identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EmbeddingHead {
    Conversation,
    Code,
    Git,
}

impl EmbeddingHead {
    pub fn as_collection_name(&self) -> &str {
        match self {
            EmbeddingHead::Conversation => "mira_conversation",
            EmbeddingHead::Code => "mira_code",
            EmbeddingHead::Git => "mira_git",
        }
    }

    pub fn from_collection_name(name: &str) -> Option<Self> {
        match name {
            "mira_conversation" | "conversation" => Some(Self::Conversation),
            "mira_code" | "code" => Some(Self::Code),
            "mira_git" | "git" => Some(Self::Git),
            _ => None,
        }
    }

    pub fn all() -> Vec<Self> {
        vec![Self::Conversation, Self::Code, Self::Git]
    }
}

impl Default for EmbeddingHead {
    fn default() -> Self {
        Self::Conversation
    }
}

impl std::fmt::Display for EmbeddingHead {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl EmbeddingHead {
    pub fn as_str(&self) -> &'static str {
        match self {
            EmbeddingHead::Conversation => "conversation",
            EmbeddingHead::Code => "code",
            EmbeddingHead::Git => "git",
        }
    }
}

/// Chat message for LLM calls (stub - Claude Code handles actual LLM calls)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
}

impl Message {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".to_string(),
            content: content.into(),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: content.into(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".to_string(),
            content: content.into(),
        }
    }
}

/// LLM response (stub)
#[derive(Debug, Clone)]
pub struct LlmResponse {
    pub content: String,
    pub usage: TokenUsage,
}

impl LlmResponse {
    pub fn new(content: String) -> Self {
        Self {
            content,
            usage: TokenUsage::default(),
        }
    }
}

/// Token usage tracking (stub)
#[derive(Debug, Clone, Default)]
pub struct TokenUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

/// LLM Provider trait (stub - returns errors since Claude Code handles LLM calls)
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// Chat with optional system prompt
    async fn chat(&self, messages: Vec<Message>, system: String) -> anyhow::Result<LlmResponse>;

    /// Simple chat without system prompt
    async fn chat_simple(&self, messages: Vec<Message>) -> anyhow::Result<LlmResponse> {
        self.chat(messages, String::new()).await
    }

    async fn chat_with_context(&self, _context: &str, messages: Vec<Message>) -> anyhow::Result<LlmResponse> {
        self.chat_simple(messages).await
    }
}

/// Embedding Provider trait
#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    async fn embed(&self, text: &str) -> anyhow::Result<Vec<f32>>;
    async fn embed_batch(&self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>>;
    fn dimensions(&self) -> usize;
}

/// Stub LLM provider that returns errors (Claude Code handles actual LLM calls)
pub struct StubLlmProvider;

#[async_trait]
impl LlmProvider for StubLlmProvider {
    async fn chat(&self, _messages: Vec<Message>, _system: String) -> anyhow::Result<LlmResponse> {
        Err(anyhow::anyhow!("LLM calls not supported in power suit mode - use Claude Code"))
    }
}

/// OpenAI embedding provider (can be used for vector search)
pub struct OpenAIEmbeddingProvider {
    api_key: String,
    model: String,
    dimensions: usize,
    client: reqwest::Client,
}

impl OpenAIEmbeddingProvider {
    pub fn new(api_key: String, model: String, dimensions: usize) -> Self {
        Self {
            api_key,
            model,
            dimensions,
            client: reqwest::Client::new(),
        }
    }

    pub fn from_config(config: &crate::config::MiraConfig) -> Self {
        Self::new(
            config.openai.api_key.clone(),
            config.openai.embedding_model.clone(),
            config.openai.embedding_dimensions,
        )
    }
}

#[async_trait]
impl EmbeddingProvider for OpenAIEmbeddingProvider {
    async fn embed(&self, text: &str) -> anyhow::Result<Vec<f32>> {
        let response = self
            .client
            .post("https://api.openai.com/v1/embeddings")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&serde_json::json!({
                "model": self.model,
                "input": text,
                "dimensions": self.dimensions
            }))
            .send()
            .await?;

        let json: serde_json::Value = response.json().await?;
        let embedding = json["data"][0]["embedding"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("Invalid embedding response"))?
            .iter()
            .filter_map(|v| v.as_f64().map(|f| f as f32))
            .collect();

        Ok(embedding)
    }

    async fn embed_batch(&self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
        let response = self
            .client
            .post("https://api.openai.com/v1/embeddings")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&serde_json::json!({
                "model": self.model,
                "input": texts,
                "dimensions": self.dimensions
            }))
            .send()
            .await?;

        let json: serde_json::Value = response.json().await?;
        let embeddings = json["data"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("Invalid embedding response"))?
            .iter()
            .map(|item| {
                item["embedding"]
                    .as_array()
                    .unwrap_or(&vec![])
                    .iter()
                    .filter_map(|v| v.as_f64().map(|f| f as f32))
                    .collect()
            })
            .collect();

        Ok(embeddings)
    }

    fn dimensions(&self) -> usize {
        self.dimensions
    }
}

/// Type alias for Arc-wrapped providers
pub type ArcLlmProvider = Arc<dyn LlmProvider>;
pub type ArcEmbeddingProvider = Arc<dyn EmbeddingProvider>;
