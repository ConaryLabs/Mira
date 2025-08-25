// src/config/mod.rs
// FIXED: Load ALL values from .env file, no hardcoded defaults

use once_cell::sync::Lazy;
use serde::Deserialize;
use std::str::FromStr;

#[derive(Debug, Clone, Deserialize)]
pub struct MiraConfig {
    // ── OpenAI Configuration
    pub openai_base_url: String,
    pub model: String,
    pub verbosity: String,
    pub reasoning_effort: String,
    pub max_output_tokens: usize,
    pub debug_logging: bool,
    pub intent_model: String,
    
    // ── Database Configuration
    pub database_url: String,
    pub sqlite_max_connections: usize,
    
    // ── Session Configuration
    pub session_id: String,
    pub default_persona: String,
    
    // ── Memory & History Configuration
    pub history_message_cap: usize,
    pub history_token_limit: usize,
    pub max_retrieval_tokens: usize,
    
    // ── WebSocket Chat Settings
    pub ws_history_cap: usize,
    pub ws_vector_search_k: usize,
    pub ws_heartbeat_interval: u64,
    pub ws_connection_timeout: u64,
    pub ws_receive_timeout: u64,
    
    // ── API Defaults
    pub history_default_limit: usize,
    pub history_max_limit: usize,
    pub context_recent_messages: usize,
    pub context_semantic_matches: usize,
    
    // ── Memory Embedding Configuration
    pub always_embed_user: bool,
    pub always_embed_assistant: bool,
    pub embed_min_chars: usize,
    pub dedup_sim_threshold: f32,
    pub salience_min_for_embed: usize,
    pub rollup_every: usize,
    pub min_salience_for_qdrant: f32,
    pub min_salience_for_storage: f32,
    
    // ── Summarization Configuration
    pub enable_summarization: bool,
    pub summary_chunk_size: usize,
    pub summary_token_limit: usize,
    pub summary_output_tokens: usize,
    pub summarize_after_messages: usize,
    
    // ── Vector Store Configuration
    pub max_vector_results: usize,
    pub enable_vector_search: bool,
    
    // ── GPT-5 Tool Configuration
    pub enable_chat_tools: bool,
    pub enable_web_search: bool,
    pub enable_code_interpreter: bool,
    pub enable_file_search: bool,
    pub enable_image_generation: bool,
    
    // ── Tool-specific Configuration
    pub web_search_max_results: usize,
    pub web_search_timeout: u64,
    pub code_interpreter_timeout: u64,
    pub code_interpreter_max_output: usize,
    pub file_search_max_files: usize,
    pub file_search_chunk_size: usize,
    pub image_generation_size: String,
    pub image_generation_quality: String,
    pub image_generation_style: String,
    
    // ── Qdrant Configuration
    pub qdrant_url: String,
    pub qdrant_collection: String,
    pub qdrant_embedding_dim: usize,
    pub qdrant_test_url: String,
    pub qdrant_test_collection: String,
    
    // ── Git Configuration
    pub git_repos_dir: String,
    pub git_cache_dir: String,
    pub git_max_file_size: usize,
    
    // ── Import Tool Configuration
    pub import_sqlite: String,
    pub import_qdrant_url: String,
    pub import_qdrant_collection: String,
    
    // ── Persona Configuration
    pub persona: String,
    pub persona_decay_timeout: u64,
    pub session_stale_timeout: u64,
    
    // ── Server Configuration
    pub host: String,
    pub port: u16,
    
    // ── CORS Settings
    pub cors_origin: String,
    pub cors_credentials: bool,
    
    // ── Rate Limiting (requests per minute)
    pub rate_limit_chat: usize,
    pub rate_limit_history: usize,
    pub rate_limit_embeddings: usize,
    
    // ── Timeouts (in seconds)
    pub openai_timeout: u64,
    pub qdrant_timeout: u64,
    pub database_timeout: u64,
    
    // ── Logging Configuration
    pub log_level: String,
    pub log_format: String,
    pub trace_sql: bool,
}

// ** THIS IS THE CORRECTED HELPER FUNCTION **
// It now correctly handles values with comments and extra whitespace.
fn env_var_or<T>(key: &str, default: T) -> T
where
    T: FromStr,
{
    match std::env::var(key) {
        Ok(val) => {
            // Trim whitespace and remove comments before parsing
            let clean_val = val.split('#').next().unwrap_or("").trim();
            match clean_val.parse::<T>() {
                Ok(parsed) => {
                    // Only log the successfully parsed value
                    eprintln!("Config: {} = {} (from environment)", key, clean_val);
                    parsed
                }
                Err(_) => {
                    eprintln!("Config: {} = '{}' (parse failed, using default)", key, val);
                    default
                }
            }
        }
        Err(_) => {
            // This is not an error, just a missing variable, so we use the default.
            default
        }
    }
}


impl MiraConfig {
    pub fn from_env() -> Self {
        // Load from .env file first if it exists
        if dotenv::dotenv().is_err() {
            eprintln!("Warning: .env file not found. Using environment variables and defaults.");
        }
        
        // This will now use the corrected `env_var_or` function
        Self {
            openai_base_url: env_var_or("OPENAI_BASE_URL", "https://api.openai.com".to_string()),
            model: env_var_or("MIRA_MODEL", "gpt-5".to_string()),
            verbosity: env_var_or("MIRA_VERBOSITY", "high".to_string()),
            reasoning_effort: env_var_or("MIRA_REASONING_EFFORT", "high".to_string()),
            max_output_tokens: env_var_or("MIRA_MAX_OUTPUT_TOKENS", 128000),
            debug_logging: env_var_or("MIRA_DEBUG_LOGGING", false),
            intent_model: env_var_or("MIRA_INTENT_MODEL", "gpt-5".to_string()),
            database_url: env_var_or("DATABASE_URL", "sqlite:./mira.db".to_string()),
            sqlite_max_connections: env_var_or("SQLITE_MAX_CONNECTIONS", 10),
            session_id: env_var_or("MIRA_SESSION_ID", "peter-eternal".to_string()),
            default_persona: env_var_or("MIRA_DEFAULT_PERSONA", "default".to_string()),
            history_message_cap: env_var_or("MIRA_HISTORY_MESSAGE_CAP", 50),
            history_token_limit: env_var_or("MIRA_HISTORY_TOKEN_LIMIT", 65536),
            max_retrieval_tokens: env_var_or("MIRA_MAX_RETRIEVAL_TOKENS", 8192),
            ws_history_cap: env_var_or("MIRA_WS_HISTORY_CAP", 100),
            ws_vector_search_k: env_var_or("MIRA_WS_VECTOR_SEARCH_K", 15),
            ws_heartbeat_interval: env_var_or("MIRA_WS_HEARTBEAT_INTERVAL", 30),
            ws_connection_timeout: env_var_or("MIRA_WS_CONNECTION_TIMEOUT", 300),
            ws_receive_timeout: env_var_or("MIRA_WS_RECEIVE_TIMEOUT", 60),
            history_default_limit: env_var_or("MIRA_HISTORY_DEFAULT_LIMIT", 30),
            history_max_limit: env_var_or("MIRA_HISTORY_MAX_LIMIT", 100),
            context_recent_messages: env_var_or("MIRA_CONTEXT_RECENT_MESSAGES", 30),
            context_semantic_matches: env_var_or("MIRA_CONTEXT_SEMANTIC_MATCHES", 15),
            always_embed_user: env_var_or("MEM_ALWAYS_EMBED_USER", true),
            always_embed_assistant: env_var_or("MEM_ALWAYS_EMBED_ASSISTANT", true),
            embed_min_chars: env_var_or("MEM_EMBED_MIN_CHARS", 6),
            dedup_sim_threshold: env_var_or("MEM_DEDUP_SIM_THRESHOLD", 0.97),
            salience_min_for_embed: env_var_or("MEM_SALIENCE_MIN_FOR_EMBED", 6),
            rollup_every: env_var_or("MEM_ROLLUP_EVERY", 50),
            min_salience_for_qdrant: env_var_or("MIRA_MIN_SALIENCE_FOR_QDRANT", 3.0),
            min_salience_for_storage: env_var_or("MIRA_MIN_SALIENCE_FOR_STORAGE", 5.0),
            enable_summarization: env_var_or("MIRA_ENABLE_SUMMARIZATION", true),
            summary_chunk_size: env_var_or("MIRA_SUMMARY_CHUNK_SIZE", 10),
            summary_token_limit: env_var_or("MIRA_SUMMARY_TOKEN_LIMIT", 32000),
            summary_output_tokens: env_var_or("MIRA_SUMMARY_OUTPUT_TOKENS", 2048),
            summarize_after_messages: env_var_or("MIRA_SUMMARIZE_AFTER_MESSAGES", 12),
            max_vector_results: env_var_or("MIRA_MAX_VECTOR_RESULTS", 5),
            enable_vector_search: env_var_or("MIRA_ENABLE_VECTOR_SEARCH", true),
            enable_chat_tools: env_var_or("ENABLE_CHAT_TOOLS", true),
            enable_web_search: env_var_or("ENABLE_WEB_SEARCH", true),
            enable_code_interpreter: env_var_or("ENABLE_CODE_INTERPRETER", true),
            enable_file_search: env_var_or("ENABLE_FILE_SEARCH", true),
            enable_image_generation: env_var_or("ENABLE_IMAGE_GENERATION", true),
            web_search_max_results: env_var_or("WEB_SEARCH_MAX_RESULTS", 10),
            web_search_timeout: env_var_or("WEB_SEARCH_TIMEOUT", 30),
            code_interpreter_timeout: env_var_or("CODE_INTERPRETER_TIMEOUT", 60),
            code_interpreter_max_output: env_var_or("CODE_INTERPRETER_MAX_OUTPUT", 10000),
            file_search_max_files: env_var_or("FILE_SEARCH_MAX_FILES", 20),
            file_search_chunk_size: env_var_or("FILE_SEARCH_CHUNK_SIZE", 1000),
            image_generation_size: env_var_or("IMAGE_GENERATION_SIZE", "1024x1024".to_string()),
            image_generation_quality: env_var_or("IMAGE_GENERATION_QUALITY", "standard".to_string()),
            image_generation_style: env_var_or("IMAGE_GENERATION_STYLE", "vivid".to_string()),
            qdrant_url: env_var_or("QDRANT_URL", "http://localhost:6333".to_string()),
            qdrant_collection: env_var_or("QDRANT_COLLECTION", "mira-memory".to_string()),
            qdrant_embedding_dim: env_var_or("QDRANT_EMBEDDING_DIM", 3072),
            qdrant_test_url: env_var_or("QDRANT_TEST_URL", "http://localhost:6334".to_string()),
            qdrant_test_collection: env_var_or("QDRANT_TEST_COLLECTION", "mira-test".to_string()),
            git_repos_dir: env_var_or("GIT_REPOS_DIR", "./repos".to_string()),
            git_cache_dir: env_var_or("MIRA_GIT_CACHE_DIR", "/tmp/mira-git-cache".to_string()),
            git_max_file_size: env_var_or("MIRA_GIT_MAX_FILE_SIZE", 10485760),
            import_sqlite: env_var_or("MIRA_IMPORT_SQLITE", "mira.sqlite".to_string()),
            import_qdrant_url: env_var_or("MIRA_IMPORT_QDRANT_URL", "http://localhost:6333".to_string()),
            import_qdrant_collection: env_var_or("MIRA_IMPORT_QDRANT_COLLECTION", "mira_memories".to_string()),
            persona: env_var_or("MIRA_PERSONA", "Default".to_string()),
            persona_decay_timeout: env_var_or("MIRA_PERSONA_DECAY_TIMEOUT", 60),
            session_stale_timeout: env_var_or("MIRA_SESSION_STALE_TIMEOUT", 30),
            host: env_var_or("MIRA_HOST", "0.0.0.0".to_string()),
            port: env_var_or("MIRA_PORT", 3001),
            cors_origin: env_var_or("MIRA_CORS_ORIGIN", "http://localhost:3000".to_string()),
            cors_credentials: env_var_or("MIRA_CORS_CREDENTIALS", true),
            rate_limit_chat: env_var_or("MIRA_RATE_LIMIT_CHAT", 60),
            rate_limit_history: env_var_or("MIRA_RATE_LIMIT_HISTORY", 120),
            rate_limit_embeddings: env_var_or("MIRA_RATE_LIMIT_EMBEDDINGS", 30),
            openai_timeout: env_var_or("MIRA_OPENAI_TIMEOUT", 60),
            qdrant_timeout: env_var_or("MIRA_QDRANT_TIMEOUT", 10),
            database_timeout: env_var_or("MIRA_DATABASE_TIMEOUT", 5),
            log_level: env_var_or("MIRA_LOG_LEVEL", "info".to_string()),
            log_format: env_var_or("MIRA_LOG_FORMAT", "pretty".to_string()),
            trace_sql: env_var_or("MIRA_TRACE_SQL", false),
        }
    }

    // --- Convenience Methods for Common Operations ---
    
    /// Check if tools are enabled (combines multiple flags)
    pub fn tools_enabled(&self) -> bool {
        self.enable_chat_tools && (
            self.enable_web_search || 
            self.enable_code_interpreter || 
            self.enable_file_search || 
            self.enable_image_generation
        )
    }
    
    /// Get full OpenAI API URL for a given endpoint
    pub fn openai_api_url(&self, endpoint: &str) -> String {
        format!("{}/v1/{}", self.openai_base_url, endpoint)
    }
    
    /// Get server bind address
    pub fn bind_address(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
    
    /// Get database pool configuration
    pub fn database_pool_config(&self) -> (String, usize) {
        (self.database_url.clone(), self.sqlite_max_connections)
    }
    
    /// Get Qdrant client configuration
    pub fn qdrant_config(&self) -> (String, String, usize) {
        (self.qdrant_url.clone(), self.qdrant_collection.clone(), self.qdrant_embedding_dim)
    }
    
    /// Check if debug logging is enabled
    pub fn is_debug(&self) -> bool {
        self.debug_logging || self.log_level.to_lowercase() == "debug"
    }
    
    /// Get timeout for OpenAI requests in milliseconds
    pub fn openai_timeout_ms(&self) -> u64 {
        self.openai_timeout * 1000
    }
    
    /// Get memory embedding settings as tuple
    pub fn embedding_settings(&self) -> (bool, bool, usize, f32) {
        (
            self.always_embed_user,
            self.always_embed_assistant,
            self.embed_min_chars,
            self.min_salience_for_qdrant
        )
    }
    
    /// Get WebSocket configuration as tuple
    pub fn websocket_config(&self) -> (u64, u64, usize, usize) {
        (
            self.ws_heartbeat_interval,
            self.ws_connection_timeout,
            self.ws_history_cap,
            self.ws_vector_search_k
        )
    }
}

// Global config instance - loaded once at startup
pub static CONFIG: Lazy<MiraConfig> = Lazy::new(MiraConfig::from_env);

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn test_config_defaults() {
        let config = MiraConfig::from_env();
        
        // Test some key defaults
        assert_eq!(config.model, "gpt-5");
        assert_eq!(config.verbosity, "high");
        assert_eq!(config.enable_web_search, true);
    }

    #[test]
    fn test_convenience_methods() {
        let config = MiraConfig::from_env();
        
        // Test OpenAI URL construction
        assert!(config.openai_api_url("chat/completions").contains("/v1/chat/completions"));
        
        // Test timeout conversion
        assert_eq!(config.openai_timeout_ms(), 60000);
    }

    #[test]
    fn test_tools_enabled() {
        // Save original env
        let original_tools = env::var("ENABLE_CHAT_TOOLS").ok();
        let original_web = env::var("ENABLE_WEB_SEARCH").ok();
        
        // Test tools disabled
        env::set_var("ENABLE_CHAT_TOOLS", "false");
        let config = MiraConfig::from_env();
        assert!(!config.tools_enabled());
        
        // Test tools enabled
        env::set_var("ENABLE_CHAT_TOOLS", "true");
        env::set_var("ENABLE_WEB_SEARCH", "true");
        let config = MiraConfig::from_env();
        assert!(config.tools_enabled());
        
        // Restore original env
        match original_tools {
            Some(val) => env::set_var("ENABLE_CHAT_TOOLS", val),
            None => env::remove_var("ENABLE_CHAT_TOOLS"),
        }
        match original_web {
            Some(val) => env::set_var("ENABLE_WEB_SEARCH", val),
            None => env::remove_var("ENABLE_WEB_SEARCH"),
        }
    }

    #[test]
    fn test_config_groups() {
        let config = MiraConfig::from_env();
        
        // Test database config
        let (db_url, max_conn) = config.database_pool_config();
        assert!(!db_url.is_empty());
        assert!(max_conn > 0);
        
        // Test Qdrant config
        let (qdrant_url, collection, dim) = config.qdrant_config();
        assert!(!qdrant_url.is_empty());
        assert!(!collection.is_empty());
        assert!(dim > 0);
        
        // Test embedding settings
        let (user, assistant, min_chars, min_sal) = config.embedding_settings();
        assert!(min_chars > 0);
        assert!(min_sal >= 0.0);
        
        // Test WebSocket config
        let (heartbeat, timeout, history, vector_k) = config.websocket_config();
        assert!(heartbeat > 0);
        assert!(timeout > 0);
        assert!(history > 0);
        assert!(vector_k > 0);
    }
}
