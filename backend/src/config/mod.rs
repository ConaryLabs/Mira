// src/config/mod.rs
// Central configuration for Mira backend - refactored into domain modules

pub mod caching;
pub mod helpers;
pub mod llm;
pub mod memory;
pub mod server;
pub mod tools;

use lazy_static::lazy_static;
use serde::{Deserialize, Serialize};

lazy_static! {
    pub static ref CONFIG: MiraConfig = MiraConfig::from_env();
}

/// Main configuration structure - composes all domain configs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MiraConfig {
    // Domain configs (organized structure)
    pub openai: llm::OpenAiConfig,
    pub gpt5: llm::Gpt5Config,
    pub memory: memory::MemoryConfig,
    pub summarization: memory::SummarizationConfig,
    pub qdrant: memory::QdrantConfig,
    pub embedding: memory::EmbeddingConfig,
    pub server: server::ServerConfig,
    pub database: server::DatabaseConfig,
    pub logging: server::LoggingConfig,
    pub session: server::SessionConfig,
    pub tools: tools::ToolsConfig,
    pub json: tools::JsonConfig,
    pub response: tools::ResponseConfig,
    pub request_cache: caching::RequestCacheConfig,
    pub recent_cache: caching::RecentCacheConfig,
    pub retry: caching::RetryConfig,

    // Flat field aliases for backward compatibility
    pub openai_api_key: String,
    pub gpt5_api_key: String,
    pub gpt5_model: String,
    pub gpt5_reasoning: llm::ReasoningEffort,
    pub openai_embedding_model: String,
    pub qdrant_url: String,
    pub qdrant_collection: String,
    pub session_id: String,
    pub enable_chat_tools: bool,
    pub embed_heads: Vec<String>,
    pub context_recent_messages: usize,
    pub context_semantic_matches: usize,
    pub use_rolling_summaries_in_context: bool,
    pub salience_min_for_embed: f32,
    pub embed_code_from_chat: bool,
    pub host: String,
    pub port: u16,
    pub database_url: String,
    pub sqlite_max_connections: u32,
}

impl MiraConfig {
    pub fn from_env() -> Self {
        // Load .env file
        dotenv::dotenv().ok(); // Don't panic if .env doesn't exist (for production)

        let openai = llm::OpenAiConfig::from_env();
        let gpt5 = llm::Gpt5Config::from_env();
        let memory = memory::MemoryConfig::from_env();
        let summarization = memory::SummarizationConfig::from_env();
        let qdrant = memory::QdrantConfig::from_env();
        let embedding = memory::EmbeddingConfig::from_env();
        let server = server::ServerConfig::from_env();
        let database = server::DatabaseConfig::from_env();
        let logging = server::LoggingConfig::from_env();
        let session = server::SessionConfig::from_env();
        let tools = tools::ToolsConfig::from_env();
        let json = tools::JsonConfig::from_env();
        let response = tools::ResponseConfig::from_env();
        let request_cache = caching::RequestCacheConfig::from_env();
        let recent_cache = caching::RecentCacheConfig::from_env();
        let retry = caching::RetryConfig::from_env();

        Self {
            // Flat field aliases (for backward compatibility)
            openai_api_key: openai.api_key.clone(),
            gpt5_api_key: gpt5.api_key.clone(),
            gpt5_model: gpt5.model.clone(),
            gpt5_reasoning: gpt5.default_reasoning_effort.clone(),
            openai_embedding_model: openai.embedding_model.clone(),
            qdrant_url: qdrant.url.clone(),
            qdrant_collection: qdrant.collection.clone(),
            session_id: session.session_id.clone(),
            enable_chat_tools: tools.enable_chat_tools,
            embed_heads: memory.embed_heads.clone(),
            context_recent_messages: memory.context_recent_messages,
            context_semantic_matches: memory.context_semantic_matches,
            use_rolling_summaries_in_context: summarization.use_rolling_in_context,
            salience_min_for_embed: memory.salience_min_for_embed,
            embed_code_from_chat: memory.embed_code_from_chat,
            host: server.host.clone(),
            port: server.port,
            database_url: database.url.clone(),
            sqlite_max_connections: database.max_connections,

            // Domain configs
            openai,
            gpt5,
            memory,
            summarization,
            qdrant,
            embedding,
            server,
            database,
            logging,
            session,
            tools,
            json,
            response,
            request_cache,
            recent_cache,
            retry,
        }
    }

    /// Validate config on startup
    pub fn validate(&self) -> anyhow::Result<()> {
        self.gpt5.validate()?;
        Ok(())
    }

    // ===========================================
    // Backward compatibility accessors
    // ===========================================

    // OpenAI
    pub fn get_openai_key(&self) -> Option<String> {
        Some(self.openai.api_key.clone())
    }

    // JSON
    pub fn get_json_max_tokens(&self) -> usize {
        self.json.max_output_tokens
    }

    // Server
    pub fn bind_address(&self) -> String {
        self.server.bind_address()
    }

    // Memory
    pub fn get_embedding_heads(&self) -> Vec<String> {
        self.memory.get_embedding_heads()
    }

    // Summarization
    pub fn is_rolling_summary_enabled(&self) -> bool {
        self.summarization.is_rolling_enabled()
    }

    pub fn get_rolling_summary_config(&self) -> RollingSummaryConfig {
        RollingSummaryConfig {
            enabled: self.summarization.is_rolling_enabled(),
            rolling_10: self.summarization.rolling_10,
            rolling_100: self.summarization.rolling_100,
            use_in_context: self.summarization.use_rolling_in_context,
            max_age_hours: self.summarization.max_age_hours,
            min_gap: self.summarization.min_gap,
        }
    }

    // Recent cache
    pub fn is_recent_cache_enabled(&self) -> bool {
        self.recent_cache.enabled
    }

    pub fn get_recent_cache_config(&self) -> RecentCacheConfig {
        RecentCacheConfig {
            enabled: self.recent_cache.enabled,
            capacity: self.recent_cache.capacity,
            ttl_seconds: self.recent_cache.ttl_seconds,
            max_per_session: self.recent_cache.max_per_session,
            warmup: self.recent_cache.warmup,
        }
    }
}

impl Default for MiraConfig {
    fn default() -> Self {
        Self::from_env()
    }
}

// Legacy types for backward compatibility
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollingSummaryConfig {
    pub enabled: bool,
    pub rolling_10: bool,
    pub rolling_100: bool,
    pub use_in_context: bool,
    pub max_age_hours: u32,
    pub min_gap: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecentCacheConfig {
    pub enabled: bool,
    pub capacity: usize,
    pub ttl_seconds: u64,
    pub max_per_session: usize,
    pub warmup: bool,
}
