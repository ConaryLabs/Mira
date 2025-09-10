// src/services/chat/config.rs
// Configuration management for chat services with GPT-5 robust memory support

use serde::{Deserialize, Serialize};
use crate::config::CONFIG;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatConfig {
    // Core LLM Configuration
    model: String,
    verbosity: String,
    reasoning_effort: String,
    max_output_tokens: usize,
    
    // History & Memory Configuration
    history_message_cap: usize,
    history_token_limit: usize,
    max_retrieval_tokens: usize,
    max_vector_search_results: usize,
    
    // Feature Flags
    enable_vector_search: bool,
    enable_web_search: bool,
    enable_code_interpreter: bool,

    // Robust Memory Configuration
    enable_robust_memory: bool,
    embedding_heads: Vec<String>,
    enable_rolling_summaries: bool,
}

impl Default for ChatConfig {
    fn default() -> Self {
        let embedding_heads = if CONFIG.is_robust_memory_enabled() {
            CONFIG.get_embedding_heads()
        } else {
            vec!["semantic".to_string()]
        };

        ChatConfig {
            model: CONFIG.gpt5_model.clone(),
            verbosity: CONFIG.verbosity.clone(),
            reasoning_effort: CONFIG.reasoning_effort.clone(),
            max_output_tokens: CONFIG.max_output_tokens,
            history_message_cap: CONFIG.history_message_cap,
            history_token_limit: CONFIG.history_token_limit,
            max_retrieval_tokens: CONFIG.max_retrieval_tokens,
            max_vector_search_results: CONFIG.max_vector_results,
            enable_vector_search: CONFIG.enable_vector_search,
            enable_web_search: CONFIG.enable_web_search,
            enable_code_interpreter: CONFIG.enable_code_interpreter,
            enable_robust_memory: CONFIG.is_robust_memory_enabled(),
            embedding_heads,
            enable_rolling_summaries: CONFIG.rolling_summaries_enabled(),
        }
    }
}

impl ChatConfig {
    // Getters
    pub fn model(&self) -> &str {
        &self.model
    }

    pub fn verbosity(&self) -> &str {
        &self.verbosity
    }

    pub fn reasoning_effort(&self) -> &str {
        &self.reasoning_effort
    }

    pub fn max_output_tokens(&self) -> usize {
        self.max_output_tokens
    }

    pub fn history_message_cap(&self) -> usize {
        self.history_message_cap
    }

    pub fn history_token_limit(&self) -> usize {
        self.history_token_limit
    }

    pub fn max_retrieval_tokens(&self) -> usize {
        self.max_retrieval_tokens
    }

    pub fn max_vector_search_results(&self) -> usize {
        self.max_vector_search_results
    }

    pub fn enable_vector_search(&self) -> bool {
        self.enable_vector_search
    }

    pub fn enable_robust_memory(&self) -> bool {
        self.enable_robust_memory
    }
    
    pub fn embedding_heads(&self) -> &[String] {
        &self.embedding_heads
    }
    
    pub fn enable_rolling_summaries(&self) -> bool {
        self.enable_rolling_summaries
    }
}
