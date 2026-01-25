// crates/mira-server/src/cli/clients.rs
// Client initialization helpers for embeddings and LLM providers

use mira::db::pool::DatabasePool;
use mira::embeddings::EmbeddingClient;
use mira::llm::DeepSeekClient;
use std::sync::Arc;
use tracing::info;

/// Get embeddings client if API key is available
/// Supports multiple providers via MIRA_EMBEDDING_PROVIDER env var (openai, google)
#[allow(dead_code)]
pub fn get_embeddings(http_client: reqwest::Client) -> Option<Arc<EmbeddingClient>> {
    get_embeddings_with_pool(None, http_client)
}

/// Get embeddings client with database pool for usage tracking
///
/// Environment variables:
/// - MIRA_EMBEDDING_DIMENSIONS: Output dimensions (default: 768)
/// - MIRA_EMBEDDING_TASK_TYPE: Task type (RETRIEVAL_DOCUMENT, SEMANTIC_SIMILARITY, etc.)
/// - GEMINI_API_KEY: Required for Gemini embeddings
/// - GOOGLE_API_KEY: Alternative to GEMINI_API_KEY
pub fn get_embeddings_with_pool(pool: Option<Arc<DatabasePool>>, http_client: reqwest::Client) -> Option<Arc<EmbeddingClient>> {
    let client = EmbeddingClient::from_env_with_http_client(pool.clone(), http_client)?;

    // Log the configured model
    info!(
        "Embeddings enabled (model: {}, {} dimensions)",
        client.model_name(),
        client.dimensions()
    );

    Some(Arc::new(client))
}

/// Get DeepSeek client if API key is available
pub fn get_deepseek(http_client: reqwest::Client) -> Option<Arc<DeepSeekClient>> {
    std::env::var("DEEPSEEK_API_KEY")
        .ok()
        .filter(|k| !k.trim().is_empty())
        .map(|key| Arc::new(DeepSeekClient::with_http_client(key, "deepseek-reasoner".into(), http_client)))
}

/// Get DeepSeek chat client for simple summarization tasks (cost-optimized)
pub fn get_deepseek_chat(http_client: reqwest::Client) -> Option<Arc<DeepSeekClient>> {
    std::env::var("DEEPSEEK_API_KEY")
        .ok()
        .filter(|k| !k.trim().is_empty())
        .map(|key| Arc::new(DeepSeekClient::with_http_client(key, "deepseek-chat".into(), http_client)))
}

/// Format token count with K/M suffix
pub fn format_tokens(tokens: i64) -> String {
    if tokens >= 1_000_000 {
        format!("{:.1}M", tokens as f64 / 1_000_000.0)
    } else if tokens >= 1_000 {
        format!("{:.1}K", tokens as f64 / 1_000.0)
    } else {
        tokens.to_string()
    }
}
