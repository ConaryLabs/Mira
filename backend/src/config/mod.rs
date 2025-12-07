// src/config/mod.rs
// Central configuration for Mira backend - OpenAI GPT-5.1 powered

pub mod caching;
pub mod helpers;
pub mod llm;
pub mod memory;
pub mod server;
pub mod testing;
pub mod tools;

use lazy_static::lazy_static;
use serde::{Deserialize, Serialize};

use crate::system::{SystemContext, SystemDetector};

lazy_static! {
    pub static ref CONFIG: MiraConfig = MiraConfig::from_env();
    pub static ref SYSTEM_CONTEXT: SystemContext = SystemDetector::detect();
}

/// Main configuration structure - composes all domain configs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MiraConfig {
    // Domain configs (organized structure)
    pub gemini: llm::GeminiConfig,
    pub openai: llm::OpenAIConfig,
    pub context_budget: llm::ContextBudgetConfig,
    pub memory: memory::MemoryConfig,
    pub summarization: memory::SummarizationConfig,
    pub qdrant: memory::QdrantConfig,
    pub embedding: memory::EmbeddingConfig,
    pub server: server::ServerConfig,
    pub database: server::DatabaseConfig,
    pub logging: server::LoggingConfig,
    pub rate_limit: server::RateLimitConfig,
    pub tools: tools::ToolsConfig,
    pub json: tools::JsonConfig,
    pub response: tools::ResponseConfig,
    pub recent_cache: caching::RecentCacheConfig,
    pub testing: testing::TestingConfig,

    // Flat field aliases for backward compatibility
    pub google_api_key: String,
    pub openai_api_key: String,
    pub gemini_model: String,
    pub gemini_embedding_model: String,
    pub gemini_thinking: llm::ThinkingLevel,
    pub qdrant_url: String,
    pub qdrant_collection: String,
    pub enable_chat_tools: bool,
    pub embed_heads: Vec<String>,
    pub context_recent_messages: usize,
    pub context_semantic_matches: usize,
    pub llm_message_history_limit: usize,
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

        let gemini = llm::GeminiConfig::from_env();
        let openai = llm::OpenAIConfig::from_env();
        let context_budget = llm::ContextBudgetConfig::from_env();
        let memory = memory::MemoryConfig::from_env();
        let summarization = memory::SummarizationConfig::from_env();
        let qdrant = memory::QdrantConfig::from_env();
        let embedding = memory::EmbeddingConfig::from_env();
        let server = server::ServerConfig::from_env();
        let database = server::DatabaseConfig::from_env();
        let logging = server::LoggingConfig::from_env();
        let rate_limit = server::RateLimitConfig::from_env();
        let tools = tools::ToolsConfig::from_env();
        let json = tools::JsonConfig::from_env();
        let response = tools::ResponseConfig::from_env();
        let recent_cache = caching::RecentCacheConfig::from_env();
        let testing = testing::TestingConfig::from_env();

        Self {
            // Flat field aliases
            google_api_key: gemini.api_key.clone(),
            openai_api_key: openai.api_key.clone(),
            gemini_model: gemini.model.clone(),
            gemini_embedding_model: gemini.embedding_model.clone(),
            gemini_thinking: gemini.default_thinking_level.clone(),
            qdrant_url: qdrant.url.clone(),
            qdrant_collection: qdrant.collection.clone(),
            enable_chat_tools: tools.enable_chat_tools,
            embed_heads: memory.embed_heads.clone(),
            context_recent_messages: memory.context_recent_messages,
            context_semantic_matches: memory.context_semantic_matches,
            llm_message_history_limit: memory.llm_message_history_limit,
            use_rolling_summaries_in_context: summarization.use_rolling_in_context,
            salience_min_for_embed: memory.salience_min_for_embed,
            embed_code_from_chat: memory.embed_code_from_chat,
            host: server.host.clone(),
            port: server.port,
            database_url: database.url.clone(),
            sqlite_max_connections: database.max_connections,

            // Domain configs
            gemini,
            openai,
            context_budget,
            memory,
            summarization,
            qdrant,
            embedding,
            server,
            database,
            logging,
            rate_limit,
            tools,
            json,
            response,
            recent_cache,
            testing,
        }
    }

    /// Validate config on startup
    pub fn validate(&self) -> anyhow::Result<()> {
        self.openai.validate()?;
        Ok(())
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

    // Recent cache
    pub fn is_recent_cache_enabled(&self) -> bool {
        self.recent_cache.enabled
    }
}

impl Default for MiraConfig {
    fn default() -> Self {
        Self::from_env()
    }
}
