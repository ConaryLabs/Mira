use once_cell::sync::Lazy;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct MiraConfig {
    // WebSocket settings
    pub ws_heartbeat_interval: u64,
    pub ws_connection_timeout: u64,
    pub ws_history_cap: usize,
    pub ws_vector_search_k: usize,
    pub ws_receive_timeout: u64,
    
    // Memory settings
    pub min_salience_for_storage: f32,
    pub min_salience_for_qdrant: f32,
    pub always_embed_user: bool,
    pub always_embed_assistant: bool,
    pub embed_min_chars: usize,
    
    // Tool settings
    pub enable_chat_tools: bool,
    pub enable_web_search: bool,
    pub enable_code_interpreter: bool,
    pub enable_file_search: bool,
    pub enable_image_generation: bool,
    
    // Model settings
    pub model: String,
    pub verbosity: String,
    pub reasoning_effort: String,
    pub max_output_tokens: usize,
    pub session_id: String,
    pub default_persona: String,
}

impl MiraConfig {
    pub fn from_env() -> Self {
        Self {
            ws_heartbeat_interval: env_var_or("MIRA_WS_HEARTBEAT_INTERVAL", 25),
            ws_connection_timeout: env_var_or("MIRA_WS_CONNECTION_TIMEOUT", 180),
            ws_history_cap: env_var_or("MIRA_WS_HISTORY_CAP", 100),
            ws_vector_search_k: env_var_or("MIRA_WS_VECTOR_SEARCH_K", 15),
            ws_receive_timeout: env_var_or("MIRA_WS_RECEIVE_TIMEOUT", 60),
            
            min_salience_for_storage: env_var_or("MIRA_MIN_SALIENCE_FOR_STORAGE", 5.0),
            min_salience_for_qdrant: env_var_or("MIRA_MIN_SALIENCE_FOR_QDRANT", 3.0),
            always_embed_user: env_var_or("MEM_ALWAYS_EMBED_USER", true),
            always_embed_assistant: env_var_or("MEM_ALWAYS_EMBED_ASSISTANT", true),
            embed_min_chars: env_var_or("MEM_EMBED_MIN_CHARS", 6),
            
            enable_chat_tools: env_var_or("ENABLE_CHAT_TOOLS", false),
            enable_web_search: env_var_or("ENABLE_WEB_SEARCH", true),
            enable_code_interpreter: env_var_or("ENABLE_CODE_INTERPRETER", true),
            enable_file_search: env_var_or("ENABLE_FILE_SEARCH", true),
            enable_image_generation: env_var_or("ENABLE_IMAGE_GENERATION", true),
            
            model: env_var_or("MIRA_MODEL", "o1-pro".to_string()),
            verbosity: env_var_or("MIRA_VERBOSITY", "high".to_string()),
            reasoning_effort: env_var_or("MIRA_REASONING_EFFORT", "high".to_string()),
            max_output_tokens: env_var_or("MIRA_MAX_OUTPUT_TOKENS", 128000),
            session_id: env_var_or("MIRA_SESSION_ID", "peter-eternal".to_string()),
            default_persona: env_var_or("MIRA_DEFAULT_PERSONA", "default".to_string()),
        }
    }
}

fn env_var_or<T: std::str::FromStr>(key: &str, default: T) -> T {
    std::env::var(key)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(default)
}

// Global config instance - loaded once at startup
pub static CONFIG: Lazy<MiraConfig> = Lazy::new(MiraConfig::from_env);
