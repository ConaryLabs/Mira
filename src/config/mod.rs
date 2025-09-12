// src/config/mod.rs
// Central configuration for Mira backend with GPT-5 robust memory system

use lazy_static::lazy_static;
use serde::{Deserialize, Serialize};
use std::env;
use std::str::FromStr;

lazy_static! {
    pub static ref CONFIG: MiraConfig = MiraConfig::from_env();
}

/// Main configuration structure for Mira
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MiraConfig {
    // Core LLM Configuration
    pub openai_api_key: Option<String>,
    pub openai_base_url: String,
    pub gpt5_model: String,
    pub verbosity: String,
    pub reasoning_effort: String,
    pub max_output_tokens: usize,
    pub debug_logging: bool,
    pub intent_model: String,

    // Structured Output Configuration
    pub max_json_output_tokens: usize,
    pub enable_json_validation: bool,
    pub max_json_repair_attempts: usize,

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
    pub ws_history_cap: usize,
    pub ws_vector_search_k: usize,
    pub ws_heartbeat_interval: u64,
    pub ws_connection_timeout: u64,
    pub ws_receive_timeout: u64,
    pub history_default_limit: usize,
    pub history_max_limit: usize,
    pub context_recent_messages: usize,
    pub context_semantic_matches: usize,

    // Memory Service Configuration
    pub always_embed_user: bool,
    pub always_embed_assistant: bool,
    pub embed_min_chars: usize,
    pub dedup_sim_threshold: f32,
    pub salience_min_for_embed: u8,
    pub rollup_every: usize,
    
    // Salience threshold - always 0.0 now but kept for compatibility
    pub min_salience_for_qdrant: f32,

    // Memory decay interval configuration
    pub decay_interval_seconds: Option<u64>,
    
    // Summarization Configuration
    pub enable_summarization: bool,
    pub summary_chunk_size: usize,
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
    pub web_search_timeout: u64,
    pub code_interpreter_timeout: u64,
    pub code_interpreter_max_output: usize,
    pub file_search_max_files: usize,
    pub file_search_chunk_size: usize,
    pub image_generation_size: String,
    pub image_generation_quality: String,
    pub image_generation_style: String,
    pub tool_timeout_seconds: u64,

    // Qdrant Configuration
    pub qdrant_url: String,
    pub qdrant_collection: String,
    pub qdrant_embedding_dim: usize,
    pub qdrant_test_url: String,
    pub qdrant_test_collection: String,

    // Git Configuration
    pub git_repos_dir: String,
    pub git_cache_dir: String,
    pub git_max_file_size: usize,

    // Import Configuration
    pub import_sqlite: String,
    pub import_qdrant_url: String,
    pub import_qdrant_collection: String,

    // Persona Configuration
    pub persona: String,
    pub persona_decay_timeout: u64,
    pub session_stale_timeout: u64,

    // Server Configuration (WebSocket Only)
    pub host: String,
    pub port: u16,
    pub rate_limit_chat: usize,
    pub rate_limit_ws: usize,
    pub rate_limit_search: usize,
    pub rate_limit_git: usize,
    pub max_concurrent_embeddings: usize,

    // Timeouts (in seconds)
    pub openai_timeout: u64,
    pub qdrant_timeout: u64,
    pub database_timeout: u64,

    // Logging Configuration
    pub log_level: String,
    pub log_format: String,
    pub trace_sql: bool,

    // Robust Memory Feature Configuration
    pub embed_heads: String,
    pub summary_rolling_10: bool,
    pub summary_rolling_100: bool,
    pub summary_phase_snapshots: bool,
    pub use_rolling_summaries_in_context: bool,
    pub rolling_summary_max_age_hours: u32,
    pub rolling_summary_min_gap: usize,

    // Chunking parameters for embedding heads
    pub embed_semantic_chunk: usize,
    pub embed_semantic_overlap: usize,
    pub embed_code_chunk: usize,
    pub embed_code_overlap: usize,
    pub embed_summary_chunk: usize,
    pub embed_summary_overlap: usize,

    // Memory Decay Configuration
    pub decay_recent_half_life_days: f32,
    pub decay_gentle_factor: f32,
    pub decay_stronger_factor: f32,
    pub decay_floor: f32,
}

/// Helper function to read environment variables with defaults
fn env_var_or<T>(key: &str, default: T) -> T
where
    T: FromStr,
    <T as FromStr>::Err: std::fmt::Debug,
{
    match env::var(key) {
        Ok(val) => val.parse().unwrap_or(default),
        Err(_) => default,
    }
}

impl MiraConfig {
    pub fn from_env() -> Self {
        dotenv::dotenv().ok();

        Self {
            // Core LLM Configuration
            openai_api_key: env::var("OPENAI_API_KEY").ok(),
            openai_base_url: env_var_or("OPENAI_BASE_URL", "https://api.openai.com/v1".to_string()),
            gpt5_model: env_var_or("GPT5_MODEL", "gpt-5".to_string()),
            verbosity: env_var_or("GPT5_VERBOSITY", "medium".to_string()),
            reasoning_effort: env_var_or("GPT5_REASONING_EFFORT", "medium".to_string()),
            max_output_tokens: env_var_or("MIRA_MAX_OUTPUT_TOKENS", 4096),
            debug_logging: env_var_or("MIRA_DEBUG_LOGGING", false),
            intent_model: env_var_or("MIRA_INTENT_MODEL", "gpt-5".to_string()),

            // Structured Output Configuration
            max_json_output_tokens: env_var_or("MAX_JSON_OUTPUT_TOKENS", 2000),
            enable_json_validation: env_var_or("ENABLE_JSON_VALIDATION", true),
            max_json_repair_attempts: env_var_or("MAX_JSON_REPAIR_ATTEMPTS", 3),
            
            // Database & Storage
            database_url: env_var_or("DATABASE_URL", "./mira.sqlite".to_string()),
            sqlite_max_connections: env_var_or("MIRA_SQLITE_MAX_CONNECTIONS", 10),

            // Session & User
            session_id: env_var_or("MIRA_SESSION_ID", "default".to_string()),
            default_persona: env_var_or("MIRA_DEFAULT_PERSONA", "Mira".to_string()),

            // Memory & History
            history_message_cap: env_var_or("MIRA_HISTORY_MESSAGE_CAP", 30),
            history_token_limit: env_var_or("MIRA_HISTORY_TOKEN_LIMIT", 32000),
            max_retrieval_tokens: env_var_or("MIRA_MAX_RETRIEVAL_TOKENS", 20000),
            ws_history_cap: env_var_or("MIRA_WS_HISTORY_CAP", 100),
            ws_vector_search_k: env_var_or("MIRA_WS_VECTOR_SEARCH_K", 5),
            ws_heartbeat_interval: env_var_or("MIRA_WS_HEARTBEAT_INTERVAL", 30),
            ws_connection_timeout: env_var_or("MIRA_WS_CONNECTION_TIMEOUT", 60),
            ws_receive_timeout: env_var_or("MIRA_WS_RECEIVE_TIMEOUT", 30),
            history_default_limit: env_var_or("MIRA_HISTORY_DEFAULT_LIMIT", 30),
            history_max_limit: env_var_or("MIRA_HISTORY_MAX_LIMIT", 100),
            context_recent_messages: env_var_or("MIRA_CONTEXT_RECENT_MESSAGES", 30),
            context_semantic_matches: env_var_or("MIRA_CONTEXT_SEMANTIC_MATCHES", 15),

            // Memory Service
            always_embed_user: env_var_or("MEM_ALWAYS_EMBED_USER", true),
            always_embed_assistant: env_var_or("MEM_ALWAYS_EMBED_ASSISTANT", true),
            embed_min_chars: env_var_or("MEM_EMBED_MIN_CHARS", 6),
            dedup_sim_threshold: env_var_or("MEM_DEDUP_SIM_THRESHOLD", 0.97),
            salience_min_for_embed: env_var_or("MEM_SALIENCE_MIN_FOR_EMBED", 6),
            rollup_every: env_var_or("MEM_ROLLUP_EVERY", 50),
            
            // Always 0.0 now - we save everything
            min_salience_for_qdrant: env_var_or("MIN_SALIENCE_FOR_QDRANT", 0.0),

            // Memory decay interval configuration
            // Default: 3600 seconds (1 hour) if not specified
            decay_interval_seconds: env::var("MIRA_DECAY_INTERVAL_SECONDS")
                .ok()
                .and_then(|s| s.parse::<u64>().ok()),

            // Summarization
            enable_summarization: env_var_or("MIRA_ENABLE_SUMMARIZATION", true),
            summary_chunk_size: env_var_or("MIRA_SUMMARY_CHUNK_SIZE", 10),
            summary_token_limit: env_var_or("MIRA_SUMMARY_TOKEN_LIMIT", 32000),
            summary_output_tokens: env_var_or("MIRA_SUMMARY_OUTPUT_TOKENS", 2048),
            summarize_after_messages: env_var_or("MIRA_SUMMARIZE_AFTER_MESSAGES", 12),

            // Vector Search
            max_vector_results: env_var_or("MIRA_MAX_VECTOR_RESULTS", 5),
            enable_vector_search: env_var_or("MIRA_ENABLE_VECTOR_SEARCH", true),

            // Tools - Updated to use MIRA_ prefix consistently
            enable_chat_tools: env_var_or("MIRA_ENABLE_CHAT_TOOLS", true),
            enable_web_search: env_var_or("MIRA_ENABLE_WEB_SEARCH", true),
            enable_code_interpreter: env_var_or("MIRA_ENABLE_CODE_INTERPRETER", false),
            enable_file_search: env_var_or("MIRA_ENABLE_FILE_SEARCH", true),
            enable_image_generation: env_var_or("MIRA_ENABLE_IMAGE_GENERATION", true),
            web_search_max_results: env_var_or("MIRA_WEB_SEARCH_MAX_RESULTS", 10),
            web_search_timeout: env_var_or("MIRA_WEB_SEARCH_TIMEOUT", 30),
            code_interpreter_timeout: env_var_or("MIRA_CODE_INTERPRETER_TIMEOUT", 60),
            code_interpreter_max_output: env_var_or("MIRA_CODE_INTERPRETER_MAX_OUTPUT", 10000),
            file_search_max_files: env_var_or("MIRA_FILE_SEARCH_MAX_FILES", 20),
            file_search_chunk_size: env_var_or("MIRA_FILE_SEARCH_CHUNK_SIZE", 1000),
            image_generation_size: env_var_or("MIRA_IMAGE_GENERATION_SIZE", "1024x1024".to_string()),
            image_generation_quality: env_var_or("MIRA_IMAGE_GENERATION_QUALITY", "standard".to_string()),
            image_generation_style: env_var_or("MIRA_IMAGE_GENERATION_STYLE", "vivid".to_string()),
            tool_timeout_seconds: env_var_or("MIRA_TOOL_TIMEOUT_SECONDS", 30),

            // Qdrant
            qdrant_url: env_var_or("QDRANT_URL", "http://localhost:6333".to_string()),
            qdrant_collection: env_var_or("QDRANT_COLLECTION", "mira-memory".to_string()),
            qdrant_embedding_dim: env_var_or("QDRANT_EMBEDDING_DIM", 3072),
            qdrant_test_url: env_var_or("QDRANT_TEST_URL", "http://localhost:6334".to_string()),
            qdrant_test_collection: env_var_or("QDRANT_TEST_COLLECTION", "mira-test".to_string()),

            // Git
            git_repos_dir: env_var_or("GIT_REPOS_DIR", "./repos".to_string()),
            git_cache_dir: env_var_or("MIRA_GIT_CACHE_DIR", "/tmp/mira-git-cache".to_string()),
            git_max_file_size: env_var_or("MIRA_GIT_MAX_FILE_SIZE", 10485760),

            // Import
            import_sqlite: env_var_or("MIRA_IMPORT_SQLITE", "mira.sqlite".to_string()),
            import_qdrant_url: env_var_or("MIRA_IMPORT_QDRANT_URL", "http://localhost:6333".to_string()),
            import_qdrant_collection: env_var_or("MIRA_IMPORT_QDRANT_COLLECTION", "mira_memories".to_string()),
            
            // Persona
            persona: env_var_or("MIRA_PERSONA", "Default".to_string()),
            persona_decay_timeout: env_var_or("MIRA_PERSONA_DECAY_TIMEOUT", 60),
            session_stale_timeout: env_var_or("MIRA_SESSION_STALE_TIMEOUT", 30),

            // Server
            host: env_var_or("MIRA_HOST", "0.0.0.0".to_string()),
            port: env_var_or("MIRA_PORT", 3001),
            rate_limit_chat: env_var_or("MIRA_RATE_LIMIT_CHAT", 60),
            rate_limit_ws: env_var_or("MIRA_RATE_LIMIT_WS", 100),
            rate_limit_search: env_var_or("MIRA_RATE_LIMIT_SEARCH", 30),
            rate_limit_git: env_var_or("MIRA_RATE_LIMIT_GIT", 10),
            max_concurrent_embeddings: env_var_or("MIRA_MAX_CONCURRENT_EMBEDDINGS", 10),

            // Timeouts
            openai_timeout: env_var_or("OPENAI_TIMEOUT", 300),
            qdrant_timeout: env_var_or("QDRANT_TIMEOUT", 30),
            database_timeout: env_var_or("DATABASE_TIMEOUT", 10),

            // Logging
            log_level: env_var_or("MIRA_LOG_LEVEL", "info".to_string()),
            log_format: env_var_or("MIRA_LOG_FORMAT", "pretty".to_string()),
            trace_sql: env_var_or("MIRA_TRACE_SQL", false),

            // Robust Memory - always enabled
            embed_heads: env_var_or("MIRA_EMBED_HEADS", "semantic,code,summary".to_string()),
            summary_rolling_10: env_var_or("MIRA_SUMMARY_ROLLING_10", true),
            summary_rolling_100: env_var_or("MIRA_SUMMARY_ROLLING_100", true),
            summary_phase_snapshots: env_var_or("MIRA_SUMMARY_PHASE_SNAPSHOTS", true),
            use_rolling_summaries_in_context: env_var_or("MIRA_USE_ROLLING_SUMMARIES_IN_CONTEXT", true),
            rolling_summary_max_age_hours: env_var_or("MIRA_ROLLING_SUMMARY_MAX_AGE_HOURS", 168),
            rolling_summary_min_gap: env_var_or("MIRA_ROLLING_SUMMARY_MIN_GAP", 3),

            // Chunking
            embed_semantic_chunk: env_var_or("MIRA_EMBED_SEMANTIC_CHUNK", 300),
            embed_semantic_overlap: env_var_or("MIRA_EMBED_SEMANTIC_OVERLAP", 100),
            embed_code_chunk: env_var_or("MIRA_EMBED_CODE_CHUNK", 256),
            embed_code_overlap: env_var_or("MIRA_EMBED_CODE_OVERLAP", 64),
            embed_summary_chunk: env_var_or("MIRA_EMBED_SUMMARY_CHUNK", 600),
            embed_summary_overlap: env_var_or("MIRA_EMBED_SUMMARY_OVERLAP", 200),

            // Memory Decay
            decay_recent_half_life_days: env_var_or("MIRA_DECAY_RECENT_HALF_LIFE_DAYS", 7.0),
            decay_gentle_factor: env_var_or("MIRA_DECAY_GENTLE_FACTOR", 0.98),
            decay_stronger_factor: env_var_or("MIRA_DECAY_STRONGER_FACTOR", 0.93),
            decay_floor: env_var_or("MIRA_DECAY_FLOOR", 0.01),
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

    /// Check if robust memory features are enabled (always true now)
    pub fn is_robust_memory_enabled(&self) -> bool {
        true
    }

    /// Check if rolling summaries are enabled
    pub fn rolling_summaries_enabled(&self) -> bool {
        self.summary_rolling_10 || self.summary_rolling_100
    }

    /// Check if 10-message rolling summaries are enabled
    pub fn rolling_10_enabled(&self) -> bool {
        self.summary_rolling_10
    }

    /// Check if 100-message rolling summaries are enabled
    pub fn rolling_100_enabled(&self) -> bool {
        self.summary_rolling_100
    }

    /// Check if snapshot summaries are enabled
    pub fn snapshot_summaries_enabled(&self) -> bool {
        self.summary_phase_snapshots
    }

    /// Check if rolling summaries should be used in context
    pub fn should_use_rolling_summaries_in_context(&self) -> bool {
        self.use_rolling_summaries_in_context
    }

    /// Get the list of enabled embedding heads
    pub fn get_embedding_heads(&self) -> Vec<String> {
        self.embed_heads
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    }

    /// Get chunk size for a specific embedding head
    pub fn get_chunk_size_for_head(&self, head: &str) -> usize {
        match head {
            "semantic" => self.embed_semantic_chunk,
            "code" => self.embed_code_chunk,
            "summary" => self.embed_summary_chunk,
            _ => self.embed_semantic_chunk,
        }
    }

    /// Get chunk overlap for a specific embedding head
    pub fn get_chunk_overlap_for_head(&self, head: &str) -> usize {
        match head {
            "semantic" => self.embed_semantic_overlap,
            "code" => self.embed_code_overlap,
            "summary" => self.embed_summary_overlap,
            _ => self.embed_semantic_overlap,
        }
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
            rolling_10: self.rolling_10_enabled(),
            rolling_100: self.rolling_100_enabled(),
            snapshots: self.snapshot_summaries_enabled(),
            use_in_context: self.should_use_rolling_summaries_in_context(),
            max_age_hours: self.rolling_summary_max_age_hours,
            min_gap: self.rolling_summary_min_gap,
        }
    }
}

/// Configuration structures for monitoring
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollingSummaryConfig {
    pub enabled: bool,
    pub rolling_10: bool,
    pub rolling_100: bool,
    pub snapshots: bool,
    pub use_in_context: bool,
    pub max_age_hours: u32,
    pub min_gap: usize,
}

impl Default for MiraConfig {
    fn default() -> Self {
        Self::from_env()
    }
}
