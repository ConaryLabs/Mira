// src/config/mod.rs
// Central configuration for Mira backend - structured response edition

use lazy_static::lazy_static;
use serde::{Deserialize, Serialize};
use std::env;

lazy_static! {
    pub static ref CONFIG: MiraConfig = MiraConfig::from_env();
}

/// Main configuration structure for Mira
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MiraConfig {
    // Core LLM Configuration
    pub openai_api_key: String,
    pub openai_base_url: String,
    pub gpt5_model: String,
    pub verbosity: String,
    pub reasoning_effort: String,
    pub max_output_tokens: usize,
    pub debug_logging: bool,

    // Structured Output Configuration
    pub max_json_output_tokens: usize,
    pub enable_json_validation: bool,
    pub max_json_repair_attempts: usize,
    
    // Response Monitoring
    pub token_warning_threshold: usize,
    pub input_token_warning: usize,

    // Database & Storage Configuration
    pub database_url: String,
    pub sqlite_max_connections: usize,

    // Session & User Configuration
    pub session_id: String,
    pub default_persona: String,

    // Memory & History Configuration
    pub history_message_cap: usize,
    pub history_token_limit: usize,
    pub max_retrieval_tokens: usize,
    pub context_recent_messages: usize,
    pub context_semantic_matches: usize,

    // Memory Service Configuration
    pub always_embed_user: bool,
    pub always_embed_assistant: bool,
    pub embed_min_chars: usize,
    pub dedup_sim_threshold: f32,
    pub salience_min_for_embed: f32,
    pub rollup_every: usize,
    
    // Salience threshold
    pub min_salience_for_qdrant: f32,
    
    // Summarization Configuration
    pub enable_summarization: bool,
    pub summary_token_limit: usize,
    pub summary_output_tokens: usize,
    pub summarize_after_messages: usize,

    // Vector Search Configuration
    pub max_vector_results: usize,
    pub enable_vector_search: bool,

    // Tool Configuration
    pub enable_chat_tools: bool,
    pub enable_web_search: bool,
    pub enable_code_interpreter: bool,
    pub enable_file_search: bool,
    pub enable_image_generation: bool,
    pub web_search_max_results: usize,
    pub tool_timeout_seconds: u64,

    // Qdrant Configuration
    pub qdrant_url: String,
    pub qdrant_collection: String,
    pub qdrant_embedding_dim: usize,

    // Server Configuration
    pub host: String,
    pub port: u16,
    pub max_concurrent_embeddings: usize,

    // Timeouts (in seconds)
    pub openai_timeout: u64,
    pub qdrant_timeout: u64,
    pub database_timeout: u64,

    // Logging Configuration
    pub log_level: String,
    pub trace_sql: bool,

    // Robust Memory Feature Configuration
    pub embed_heads: String,
    pub summary_rolling_10: bool,
    pub summary_rolling_100: bool,
    pub use_rolling_summaries_in_context: bool,
    pub rolling_summary_max_age_hours: u32,
    pub rolling_summary_min_gap: usize,

    // Memory Decay Configuration
    pub decay_recent_half_life_days: f32,
    pub decay_gentle_factor: f32,
    pub decay_stronger_factor: f32,
    pub decay_floor: f32,
    pub decay_high_salience_threshold: f32,
    
    // Recall & Context Configuration
    pub recall_recent: usize,
    pub recall_semantic: usize,
    pub recall_k_per_head: usize,
    pub recent_message_limit: usize,
    
    // Response Configuration
    pub max_response_tokens: usize,
    
    // Robustness & Performance Features
    pub api_max_retries: usize,
    pub api_retry_delay_ms: u64,
    pub enable_request_cache: bool,
    pub cache_ttl_seconds: u64,
    
    // Embedding Model Configuration
    pub embed_model: String,
    pub embed_dimensions: usize,
    
    // Recent Cache Configuration
    pub enable_recent_cache: bool,
    pub recent_cache_capacity: usize,
    pub recent_cache_ttl_seconds: u64,
    pub recent_cache_max_per_session: usize,
    pub recent_cache_warmup: bool,
}

/// Parse an environment variable or die trying
fn require_env(key: &str) -> String {
    env::var(key).unwrap_or_else(|_| panic!("{} must be set in .env", key))
}

/// Parse an environment variable as a specific type or die trying
fn require_env_parsed<T: std::str::FromStr>(key: &str) -> T 
where 
    T::Err: std::fmt::Debug 
{
    env::var(key)
        .unwrap_or_else(|_| panic!("{} must be set in .env", key))
        .parse::<T>()
        .unwrap_or_else(|e| panic!("{} must be a valid {}: {:?}", key, std::any::type_name::<T>(), e))
}

impl MiraConfig {
    pub fn from_env() -> Self {
        // Load .env file or die
        dotenv::dotenv().expect("Failed to load .env file - cannot proceed without configuration!");

        // Validate embedding heads includes all 4
        let embed_heads = require_env("MIRA_EMBED_HEADS");
        if !embed_heads.contains("semantic") || 
           !embed_heads.contains("code") || 
           !embed_heads.contains("summary") || 
           !embed_heads.contains("documents") {
            panic!("MIRA_EMBED_HEADS must include all 4 heads: 'semantic,code,summary,documents' - got: '{}'", embed_heads);
        }

        Self {
            // Core LLM Configuration
            openai_api_key: require_env("OPENAI_API_KEY"),
            openai_base_url: require_env("OPENAI_BASE_URL"),
            gpt5_model: require_env("GPT5_MODEL"),
            verbosity: require_env("GPT5_VERBOSITY"),
            reasoning_effort: require_env("GPT5_REASONING_EFFORT"),
            max_output_tokens: require_env_parsed("MIRA_MAX_OUTPUT_TOKENS"),
            debug_logging: require_env_parsed("MIRA_DEBUG_LOGGING"),

            // Structured Output Configuration
            max_json_output_tokens: require_env_parsed("MAX_JSON_OUTPUT_TOKENS"),
            enable_json_validation: require_env_parsed("ENABLE_JSON_VALIDATION"),
            max_json_repair_attempts: require_env_parsed("MAX_JSON_REPAIR_ATTEMPTS"),
            
            // Response Monitoring
            token_warning_threshold: require_env_parsed("TOKEN_WARNING_THRESHOLD"),
            input_token_warning: require_env_parsed("INPUT_TOKEN_WARNING"),
            
            // Database & Storage
            database_url: require_env("DATABASE_URL"),
            sqlite_max_connections: require_env_parsed("MIRA_SQLITE_MAX_CONNECTIONS"),

            // Session & User
            session_id: require_env("MIRA_SESSION_ID"),
            default_persona: require_env("MIRA_DEFAULT_PERSONA"),

            // Memory & History
            history_message_cap: require_env_parsed("MIRA_HISTORY_MESSAGE_CAP"),
            history_token_limit: require_env_parsed("MIRA_HISTORY_TOKEN_LIMIT"),
            max_retrieval_tokens: require_env_parsed("MIRA_MAX_RETRIEVAL_TOKENS"),
            context_recent_messages: require_env_parsed("MIRA_CONTEXT_RECENT_MESSAGES"),
            context_semantic_matches: require_env_parsed("MIRA_CONTEXT_SEMANTIC_MATCHES"),

            // Memory Service
            always_embed_user: require_env_parsed("MEM_ALWAYS_EMBED_USER"),
            always_embed_assistant: require_env_parsed("MEM_ALWAYS_EMBED_ASSISTANT"),
            embed_min_chars: require_env_parsed("MEM_EMBED_MIN_CHARS"),
            dedup_sim_threshold: require_env_parsed("MEM_DEDUP_SIM_THRESHOLD"),
            salience_min_for_embed: require_env_parsed("MEM_SALIENCE_MIN_FOR_EMBED"),
            rollup_every: require_env_parsed("MEM_ROLLUP_EVERY"),
            min_salience_for_qdrant: require_env_parsed("MIN_SALIENCE_FOR_QDRANT"),

            // Summarization
            enable_summarization: require_env_parsed("MIRA_ENABLE_SUMMARIZATION"),
            summary_token_limit: require_env_parsed("MIRA_SUMMARY_TOKEN_LIMIT"),
            summary_output_tokens: require_env_parsed("MIRA_SUMMARY_OUTPUT_TOKENS"),
            summarize_after_messages: require_env_parsed("MIRA_SUMMARIZE_AFTER_MESSAGES"),

            // Vector Search
            max_vector_results: require_env_parsed("MIRA_MAX_VECTOR_RESULTS"),
            enable_vector_search: require_env_parsed("MIRA_ENABLE_VECTOR_SEARCH"),

            // Tools
            enable_chat_tools: require_env_parsed("MIRA_ENABLE_CHAT_TOOLS"),
            enable_web_search: require_env_parsed("MIRA_ENABLE_WEB_SEARCH"),
            enable_code_interpreter: require_env_parsed("MIRA_ENABLE_CODE_INTERPRETER"),
            enable_file_search: require_env_parsed("MIRA_ENABLE_FILE_SEARCH"),
            enable_image_generation: require_env_parsed("MIRA_ENABLE_IMAGE_GENERATION"),
            web_search_max_results: require_env_parsed("MIRA_WEB_SEARCH_MAX_RESULTS"),
            tool_timeout_seconds: require_env_parsed("MIRA_TOOL_TIMEOUT_SECONDS"),

            // Qdrant
            qdrant_url: require_env("QDRANT_URL"),
            qdrant_collection: require_env("QDRANT_COLLECTION"),
            qdrant_embedding_dim: require_env_parsed("QDRANT_EMBEDDING_DIM"),

            // Server
            host: require_env("MIRA_HOST"),
            port: require_env_parsed("MIRA_PORT"),
            max_concurrent_embeddings: require_env_parsed("MIRA_MAX_CONCURRENT_EMBEDDINGS"),

            // Timeouts
            openai_timeout: require_env_parsed("OPENAI_TIMEOUT"),
            qdrant_timeout: require_env_parsed("QDRANT_TIMEOUT"),
            database_timeout: require_env_parsed("DATABASE_TIMEOUT"),

            // Logging
            log_level: require_env("MIRA_LOG_LEVEL"),
            trace_sql: require_env_parsed("MIRA_TRACE_SQL"),

            // Robust Memory - already validated above
            embed_heads,
            summary_rolling_10: require_env_parsed("MIRA_SUMMARY_ROLLING_10"),
            summary_rolling_100: require_env_parsed("MIRA_SUMMARY_ROLLING_100"),
            use_rolling_summaries_in_context: require_env_parsed("MIRA_USE_ROLLING_SUMMARIES_IN_CONTEXT"),
            rolling_summary_max_age_hours: require_env_parsed("MIRA_ROLLING_SUMMARY_MAX_AGE_HOURS"),
            rolling_summary_min_gap: require_env_parsed("MIRA_ROLLING_SUMMARY_MIN_GAP"),

            // Memory Decay
            decay_recent_half_life_days: require_env_parsed("MIRA_DECAY_RECENT_HALF_LIFE_DAYS"),
            decay_gentle_factor: require_env_parsed("MIRA_DECAY_GENTLE_FACTOR"),
            decay_stronger_factor: require_env_parsed("MIRA_DECAY_STRONGER_FACTOR"),
            decay_floor: require_env_parsed("MIRA_DECAY_FLOOR"),
            decay_high_salience_threshold: require_env_parsed("MIRA_DECAY_HIGH_SALIENCE_THRESHOLD"),
            
            // Recall & Context
            recall_recent: require_env_parsed("MIRA_RECALL_RECENT"),
            recall_semantic: require_env_parsed("MIRA_RECALL_SEMANTIC"),
            recall_k_per_head: require_env_parsed("MIRA_RECALL_K_PER_HEAD"),
            recent_message_limit: require_env_parsed("MIRA_RECENT_MESSAGE_LIMIT"),
            
            // Response Configuration
            max_response_tokens: require_env_parsed("MIRA_MAX_RESPONSE_TOKENS"),
            
            // Robustness & Performance Features
            api_max_retries: require_env_parsed("MIRA_API_MAX_RETRIES"),
            api_retry_delay_ms: require_env_parsed("MIRA_API_RETRY_DELAY_MS"),
            enable_request_cache: require_env_parsed("MIRA_ENABLE_REQUEST_CACHE"),
            cache_ttl_seconds: require_env_parsed("MIRA_CACHE_TTL_SECONDS"),
            
            // Embedding Model Configuration
            embed_model: require_env("MIRA_EMBED_MODEL"),
            embed_dimensions: require_env_parsed("MIRA_EMBED_DIMENSIONS"),
            
            // Recent Cache Configuration
            enable_recent_cache: require_env_parsed("MIRA_ENABLE_RECENT_CACHE"),
            recent_cache_capacity: require_env_parsed("MIRA_RECENT_CACHE_CAPACITY"),
            recent_cache_ttl_seconds: require_env_parsed("MIRA_RECENT_CACHE_TTL"),
            recent_cache_max_per_session: require_env_parsed("MIRA_RECENT_CACHE_MAX_PER_SESSION"),
            recent_cache_warmup: require_env_parsed("MIRA_RECENT_CACHE_WARMUP"),
        }
    }

    /// Get verbosity setting for a specific operation type
    pub fn get_verbosity_for(&self, operation: &str) -> &str {
        match operation {
            "classification" | "metadata" => "minimal",
            "summary" => "low",
            _ => &self.verbosity,
        }
    }

    /// Get reasoning effort for a specific operation type
    pub fn get_reasoning_effort_for(&self, operation: &str) -> &str {
        match operation {
            "classification" | "metadata" => "minimal",
            "summary" => "low",
            "complex" => "high",
            _ => &self.reasoning_effort,
        }
    }

    /// Get max tokens for JSON operations
    pub fn get_json_max_tokens(&self) -> usize {
        self.max_json_output_tokens
    }

    /// Get the bind address for the server
    pub fn bind_address(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }

    /// Check if rolling summaries are enabled
    pub fn rolling_summaries_enabled(&self) -> bool {
        self.summary_rolling_10 || self.summary_rolling_100
    }

    /// Get the list of enabled embedding heads
    pub fn get_embedding_heads(&self) -> Vec<String> {
        self.embed_heads
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    }

    /// Get list of enabled tools
    pub fn get_enabled_tools(&self) -> Vec<String> {
        let mut tools = Vec::new();
        if self.enable_web_search {
            tools.push("web_search".to_string());
        }
        if self.enable_code_interpreter {
            tools.push("code_interpreter".to_string());
        }
        if self.enable_file_search {
            tools.push("file_search".to_string());
        }
        if self.enable_image_generation {
            tools.push("image_generation".to_string());
        }
        tools
    }

    /// Check if any tools are enabled
    pub fn has_tools_enabled(&self) -> bool {
        self.enable_chat_tools && (
            self.enable_web_search ||
            self.enable_code_interpreter ||
            self.enable_file_search ||
            self.enable_image_generation
        )
    }

    /// Get rolling summary configuration for debugging
    pub fn get_rolling_summary_config(&self) -> RollingSummaryConfig {
        RollingSummaryConfig {
            enabled: self.rolling_summaries_enabled(),
            rolling_10: self.summary_rolling_10,
            rolling_100: self.summary_rolling_100,
            use_in_context: self.use_rolling_summaries_in_context,
            max_age_hours: self.rolling_summary_max_age_hours,
            min_gap: self.rolling_summary_min_gap,
        }
    }
    
    /// Get embedding model name
    pub fn get_embed_model(&self) -> &str {
        &self.embed_model
    }
    
    /// Get embedding dimensions
    pub fn get_embed_dimensions(&self) -> usize {
        self.embed_dimensions
    }
    
    /// Check if recent cache is enabled
    pub fn is_recent_cache_enabled(&self) -> bool {
        self.enable_recent_cache
    }
    
    /// Get recent cache configuration
    pub fn get_recent_cache_config(&self) -> RecentCacheConfig {
        RecentCacheConfig {
            enabled: self.enable_recent_cache,
            capacity: self.recent_cache_capacity,
            ttl_seconds: self.recent_cache_ttl_seconds,
            max_per_session: self.recent_cache_max_per_session,
            warmup: self.recent_cache_warmup,
        }
    }
}

/// Configuration structures for monitoring
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollingSummaryConfig {
    pub enabled: bool,
    pub rolling_10: bool,
    pub rolling_100: bool,
    pub use_in_context: bool,
    pub max_age_hours: u32,
    pub min_gap: usize,
}

/// Recent cache configuration structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecentCacheConfig {
    pub enabled: bool,
    pub capacity: usize,
    pub ttl_seconds: u64,
    pub max_per_session: usize,
    pub warmup: bool,
}

impl Default for MiraConfig {
    fn default() -> Self {
        Self::from_env()
    }
}
