// src/config/mod.rs
// Central configuration for Mira backend - GPT-5 Only

use lazy_static::lazy_static;
use serde::{Deserialize, Serialize};
use std::env;

lazy_static! {
    pub static ref CONFIG: MiraConfig = MiraConfig::from_env();
}

/// Main configuration structure for Mira
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MiraConfig {
    // ===== GPT-5 RESPONSES API =====
    pub gpt5_api_key: String,
    pub gpt5_model: String,
    pub gpt5_max_tokens: usize,
    pub gpt5_verbosity: String,
    pub gpt5_reasoning: String,
    
    // ===== DEEPSEEK (CODE GENERATION ONLY) =====
    pub use_deepseek_codegen: bool,
    pub deepseek_api_key: String,
    
    // ===== OPENAI (EMBEDDINGS/IMAGES ONLY) =====
    pub openai_api_key: String,
    pub openai_embedding_model: String,
    
    // ===== CORE CONFIGURATION =====
    pub max_output_tokens: usize,
    pub debug_logging: bool,

    // Structured Output
    pub enable_json_validation: bool,
    pub max_json_repair_attempts: usize,
    pub max_json_output_tokens: usize,
    
    // Response Monitoring
    pub token_warning_threshold: usize,
    pub input_token_warning: usize,
    
    // Database & Storage
    pub database_url: String,
    pub sqlite_max_connections: u32,

    // Session & User
    pub session_id: String,
    pub default_persona: String,

    // Memory & History
    pub history_message_cap: usize,
    pub history_token_limit: usize,
    pub max_retrieval_tokens: usize,
    pub context_recent_messages: usize,
    pub context_semantic_matches: usize,

    // Memory Service
    pub always_embed_user: bool,
    pub always_embed_assistant: bool,
    pub embed_min_chars: usize,
    pub dedup_sim_threshold: f32,
    pub salience_min_for_embed: f32,
    pub rollup_every: usize,
    pub min_salience_for_qdrant: f32,

    // Summarization
    pub enable_summarization: bool,
    pub summary_token_limit: usize,
    pub summary_output_tokens: usize,
    pub summarize_after_messages: usize,

    // Vector Search
    pub max_vector_results: usize,
    pub enable_vector_search: bool,

    // Tools
    pub enable_chat_tools: bool,
    pub enable_web_search: bool,
    pub enable_code_interpreter: bool,
    pub enable_file_search: bool,
    pub enable_image_generation: bool,
    pub web_search_max_results: usize,
    pub tool_timeout_seconds: u64,

    // Qdrant
    pub qdrant_url: String,
    pub qdrant_collection: String,
    pub qdrant_embedding_dim: usize,

    // Server
    pub host: String,
    pub port: u16,
    pub max_concurrent_embeddings: usize,

    // Timeouts
    pub openai_timeout: u64,
    pub qdrant_timeout: u64,
    pub database_timeout: u64,

    // Logging
    pub log_level: String,
    pub trace_sql: bool,

    // Robust Memory
    pub embed_heads: Vec<String>,
    pub summary_rolling_10: bool,
    pub summary_rolling_100: bool,
    pub use_rolling_summaries_in_context: bool,
    pub rolling_summary_max_age_hours: u32,
    pub rolling_summary_min_gap: usize,

    // Memory Decay
    pub decay_recent_half_life_days: f32,
    pub decay_gentle_factor: f32,
    pub decay_stronger_factor: f32,
    pub decay_floor: f32,
    pub decay_high_salience_threshold: f32,
    
    // Recall & Context
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

impl MiraConfig {
    pub fn from_env() -> Self {
        // Load .env file
        dotenv::dotenv().ok(); // Don't panic if .env doesn't exist (for production)
        
        // Validate embedding heads includes all 4
        let embed_heads_str = require_env("MIRA_EMBED_HEADS");
        if !embed_heads_str.contains("semantic") || 
           !embed_heads_str.contains("code") || 
           !embed_heads_str.contains("summary") || 
           !embed_heads_str.contains("documents") {
            panic!("MIRA_EMBED_HEADS must include all 4 heads: 'semantic,code,summary,documents' - got: '{}'", embed_heads_str);
        }
        let embed_heads: Vec<String> = embed_heads_str
            .split(',')
            .map(|s| s.trim().to_string())
            .collect();

        Self {
            // ===== GPT-5 RESPONSES API =====
            gpt5_api_key: require_env("GPT5_API_KEY"),
            gpt5_model: env_or("GPT5_MODEL", "gpt-5"),
            gpt5_max_tokens: env_usize("GPT5_MAX_TOKENS", 128000),
            gpt5_verbosity: env_or("GPT5_VERBOSITY", "medium"),
            gpt5_reasoning: env_or("GPT5_REASONING", "medium"),
            
            // ===== DEEPSEEK (CODE GENERATION ONLY) =====
            use_deepseek_codegen: env::var("USE_DEEPSEEK_CODEGEN")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(false),  // Default to false for safety
            deepseek_api_key: env_or("DEEPSEEK_API_KEY", ""),
            
            // ===== OPENAI (EMBEDDINGS/IMAGES) =====
            openai_api_key: require_env("OPENAI_API_KEY"),
            openai_embedding_model: env_or("OPENAI_EMBEDDING_MODEL", "text-embedding-3-large"),
            
            // ===== CORE CONFIGURATION =====
            max_output_tokens: require_env_parsed("MAX_OUTPUT_TOKENS"),
            debug_logging: require_env_parsed("MIRA_DEBUG_LOGGING"),

            // Structured Output
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

            // Robust Memory
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

    /// Validate config on startup
    pub fn validate(&self) -> anyhow::Result<()> {
        // Validate GPT-5 verbosity
        if !["low", "medium", "high"].contains(&self.gpt5_verbosity.as_str()) {
            return Err(anyhow::anyhow!("Invalid GPT5_VERBOSITY: must be low/medium/high"));
        }
        
        // Validate GPT-5 reasoning
        if !["minimal", "low", "medium", "high"].contains(&self.gpt5_reasoning.as_str()) {
            return Err(anyhow::anyhow!("Invalid GPT5_REASONING: must be minimal/low/medium/high"));
        }
        
        Ok(())
    }

    /// Get the OpenAI API key (for embeddings/images)
    pub fn get_openai_key(&self) -> Option<String> {
        Some(self.openai_api_key.clone())
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
    pub fn is_rolling_summary_enabled(&self) -> bool {
        self.summary_rolling_10 || self.summary_rolling_100
    }
    
    /// Get rolling summary configuration
    pub fn get_rolling_summary_config(&self) -> RollingSummaryConfig {
        RollingSummaryConfig {
            enabled: self.is_rolling_summary_enabled(),
            rolling_10: self.summary_rolling_10,
            rolling_100: self.summary_rolling_100,
            use_in_context: self.use_rolling_summaries_in_context,
            max_age_hours: self.rolling_summary_max_age_hours,
            min_gap: self.rolling_summary_min_gap,
        }
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

    /// Get embedding heads from config
    pub fn get_embedding_heads(&self) -> Vec<String> {
        self.embed_heads.clone()
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

// ===== HELPER FUNCTIONS =====

fn require_env(key: &str) -> String {
    env::var(key).unwrap_or_else(|_| panic!("Missing required env var: {}", key))
}

fn env_or(key: &str, default: &str) -> String {
    env::var(key).unwrap_or_else(|_| default.to_string())
}

fn env_usize(key: &str, default: usize) -> usize {
    env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn require_env_parsed<T: std::str::FromStr>(key: &str) -> T 
where
    T::Err: std::fmt::Display,
{
    env::var(key)
        .unwrap_or_else(|_| panic!("Missing required env var: {}", key))
        .parse()
        .unwrap_or_else(|e| panic!("Failed to parse {}: {}", key, e))
}
