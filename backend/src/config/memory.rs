// src/config/memory.rs
// Memory and embedding configuration

use serde::{Deserialize, Serialize};
use tracing::warn;

/// Memory service configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConfig {
    // Storage behavior
    pub always_embed_user: bool,
    pub always_embed_assistant: bool,
    pub embed_min_chars: usize,
    pub dedup_sim_threshold: f32,
    pub salience_min_for_embed: f32,
    pub min_salience_for_qdrant: f32,

    // Context and retrieval
    pub context_recent_messages: usize,
    pub context_semantic_matches: usize,
    pub llm_message_history_limit: usize, // Max messages to include in LLM message array

    // Recall configuration
    pub recall_recent: usize,
    pub recall_semantic: usize,
    pub recall_k_per_head: usize,

    // Embedding heads
    pub embed_heads: Vec<String>,
    pub embed_code_from_chat: bool,

    // Vector search
    pub max_vector_results: usize,
    pub enable_vector_search: bool,

    // Decay configuration
    pub decay_recent_half_life_days: f32,
    pub decay_gentle_factor: f32,
    pub decay_stronger_factor: f32,
    pub decay_floor: f32,
    pub decay_high_salience_threshold: f32,
}

impl MemoryConfig {
    pub fn from_env() -> Self {
        // Embedding heads: allow subsets; warn if missing common heads
        let embed_heads_str = super::helpers::require_env("MIRA_EMBED_HEADS");
        let embed_heads: Vec<String> = embed_heads_str
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        if !embed_heads.iter().any(|h| h == "semantic") {
            warn!("MIRA_EMBED_HEADS does not include 'semantic' — retrieval quality may suffer");
        }
        if !embed_heads.iter().any(|h| h == "code") {
            warn!(
                "MIRA_EMBED_HEADS does not include 'code' — code-head embeddings disabled globally"
            );
        }

        Self {
            always_embed_user: super::helpers::require_env_parsed("MEM_ALWAYS_EMBED_USER"),
            always_embed_assistant: super::helpers::require_env_parsed(
                "MEM_ALWAYS_EMBED_ASSISTANT",
            ),
            embed_min_chars: super::helpers::require_env_parsed("MEM_EMBED_MIN_CHARS"),
            dedup_sim_threshold: super::helpers::require_env_parsed("MEM_DEDUP_SIM_THRESHOLD"),
            salience_min_for_embed: super::helpers::require_env_parsed(
                "MEM_SALIENCE_MIN_FOR_EMBED",
            ),
            min_salience_for_qdrant: super::helpers::require_env_parsed("MIN_SALIENCE_FOR_QDRANT"),

            context_recent_messages: super::helpers::require_env_parsed(
                "MIRA_CONTEXT_RECENT_MESSAGES",
            ),
            context_semantic_matches: super::helpers::require_env_parsed(
                "MIRA_CONTEXT_SEMANTIC_MATCHES",
            ),
            llm_message_history_limit: super::helpers::require_env_parsed(
                "MIRA_LLM_MESSAGE_HISTORY_LIMIT",
            ),

            recall_recent: super::helpers::require_env_parsed("MIRA_RECALL_RECENT"),
            recall_semantic: super::helpers::require_env_parsed("MIRA_RECALL_SEMANTIC"),
            recall_k_per_head: super::helpers::require_env_parsed("MIRA_RECALL_K_PER_HEAD"),

            embed_heads,
            embed_code_from_chat: std::env::var("MIRA_EMBED_CODE_FROM_CHAT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(false),

            max_vector_results: super::helpers::require_env_parsed("MIRA_MAX_VECTOR_RESULTS"),
            enable_vector_search: super::helpers::require_env_parsed("MIRA_ENABLE_VECTOR_SEARCH"),

            decay_recent_half_life_days: super::helpers::require_env_parsed(
                "MIRA_DECAY_RECENT_HALF_LIFE_DAYS",
            ),
            decay_gentle_factor: super::helpers::require_env_parsed("MIRA_DECAY_GENTLE_FACTOR"),
            decay_stronger_factor: super::helpers::require_env_parsed("MIRA_DECAY_STRONGER_FACTOR"),
            decay_floor: super::helpers::require_env_parsed("MIRA_DECAY_FLOOR"),
            decay_high_salience_threshold: super::helpers::require_env_parsed(
                "MIRA_DECAY_HIGH_SALIENCE_THRESHOLD",
            ),
        }
    }

    pub fn get_embedding_heads(&self) -> Vec<String> {
        self.embed_heads.clone()
    }
}

/// Summarization configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SummarizationConfig {
    pub enabled: bool,
    pub token_limit: usize,
    pub output_tokens: usize,
    pub summarize_after_messages: usize,

    // Rolling summaries (100-message window)
    pub rolling_enabled: bool,
    pub use_rolling_in_context: bool,
    pub max_age_hours: u32,
    pub min_gap: usize,
}

impl SummarizationConfig {
    pub fn from_env() -> Self {
        Self {
            enabled: super::helpers::require_env_parsed("MIRA_ENABLE_SUMMARIZATION"),
            token_limit: super::helpers::require_env_parsed("MIRA_SUMMARY_TOKEN_LIMIT"),
            output_tokens: super::helpers::require_env_parsed("MIRA_SUMMARY_OUTPUT_TOKENS"),
            summarize_after_messages: super::helpers::require_env_parsed(
                "MIRA_SUMMARIZE_AFTER_MESSAGES",
            ),

            rolling_enabled: super::helpers::require_env_parsed("MIRA_SUMMARY_ROLLING_ENABLED"),
            use_rolling_in_context: super::helpers::require_env_parsed(
                "MIRA_USE_ROLLING_SUMMARIES_IN_CONTEXT",
            ),
            max_age_hours: super::helpers::require_env_parsed("MIRA_ROLLING_SUMMARY_MAX_AGE_HOURS"),
            min_gap: super::helpers::require_env_parsed("MIRA_ROLLING_SUMMARY_MIN_GAP"),
        }
    }

    pub fn is_rolling_enabled(&self) -> bool {
        self.rolling_enabled
    }
}

/// Qdrant vector database configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QdrantConfig {
    pub url: String,
    pub collection: String,
    pub embedding_dim: usize,
    pub timeout: u64,
}

impl QdrantConfig {
    pub fn from_env() -> Self {
        Self {
            url: super::helpers::require_env("QDRANT_URL"),
            collection: super::helpers::require_env("QDRANT_COLLECTION"),
            embedding_dim: super::helpers::require_env_parsed("QDRANT_EMBEDDING_DIM"),
            timeout: super::helpers::require_env_parsed("QDRANT_TIMEOUT"),
        }
    }
}

/// Embedding model configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingConfig {
    pub model: String,
    pub dimensions: usize,
    pub max_concurrent: usize,
}

impl EmbeddingConfig {
    pub fn from_env() -> Self {
        Self {
            model: super::helpers::require_env("MIRA_EMBED_MODEL"),
            dimensions: super::helpers::require_env_parsed("MIRA_EMBED_DIMENSIONS"),
            max_concurrent: super::helpers::require_env_parsed("MIRA_MAX_CONCURRENT_EMBEDDINGS"),
        }
    }
}
