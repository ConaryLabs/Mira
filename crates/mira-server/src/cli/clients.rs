// crates/mira-server/src/cli/clients.rs
// Client initialization helpers for embeddings

use mira::config::{ApiKeys, EmbeddingsConfig};
use mira::db::pool::DatabasePool;
use mira::embeddings::EmbeddingClient;
use std::sync::Arc;
use tracing::info;

/// Get embeddings client with database pool for usage tracking (using pre-loaded config)
pub fn get_embeddings_from_config(
    api_keys: &ApiKeys,
    embeddings_config: &EmbeddingsConfig,
    pool: Option<Arc<DatabasePool>>,
    http_client: reqwest::Client,
) -> Option<Arc<EmbeddingClient>> {
    let client = EmbeddingClient::from_config_with_http_client(
        api_keys,
        embeddings_config,
        pool,
        http_client,
    )?;

    // Log the configured model
    info!(
        "Embeddings enabled (model: {}, {} dimensions)",
        client.model_name(),
        client.dimensions()
    );

    Some(Arc::new(client))
}

/// Get embeddings client with database pool for usage tracking
///
/// Environment variables:
/// - MIRA_EMBEDDING_DIMENSIONS: Output dimensions (default: 1536)
/// - OPENAI_API_KEY: Required for OpenAI embeddings
///
/// Note: Prefer get_embeddings_from_config() to avoid duplicate env reads
pub fn get_embeddings_with_pool(
    pool: Option<Arc<DatabasePool>>,
    http_client: reqwest::Client,
) -> Option<Arc<EmbeddingClient>> {
    get_embeddings_from_config(
        &ApiKeys::from_env(),
        &EmbeddingsConfig::from_env(),
        pool,
        http_client,
    )
}
