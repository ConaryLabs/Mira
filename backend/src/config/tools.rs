// src/config/tools.rs
// Tool and feature configuration

use serde::{Deserialize, Serialize};

/// Tools configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolsConfig {
    pub enable_chat_tools: bool,
    pub enable_web_search: bool,
    pub web_search_max_results: usize,
    pub timeout_seconds: u64,
    pub max_iterations: usize,
}

impl ToolsConfig {
    pub fn from_env() -> Self {
        Self {
            enable_chat_tools: super::helpers::require_env_parsed("MIRA_ENABLE_CHAT_TOOLS"),
            enable_web_search: super::helpers::require_env_parsed("MIRA_ENABLE_WEB_SEARCH"),
            web_search_max_results: super::helpers::require_env_parsed(
                "MIRA_WEB_SEARCH_MAX_RESULTS",
            ),
            timeout_seconds: super::helpers::require_env_parsed("MIRA_TOOL_TIMEOUT_SECONDS"),
            max_iterations: super::helpers::env_usize("TOOL_MAX_ITERATIONS", 25),
        }
    }
}

/// JSON/structured output configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonConfig {
    pub enable_validation: bool,
    pub max_repair_attempts: usize,
    pub max_output_tokens: usize,
}

impl JsonConfig {
    pub fn from_env() -> Self {
        Self {
            enable_validation: super::helpers::require_env_parsed("ENABLE_JSON_VALIDATION"),
            max_repair_attempts: super::helpers::require_env_parsed("MAX_JSON_REPAIR_ATTEMPTS"),
            max_output_tokens: super::helpers::require_env_parsed("MAX_JSON_OUTPUT_TOKENS"),
        }
    }

    pub fn get_max_tokens(&self) -> usize {
        self.max_output_tokens
    }
}

/// Response and output configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseConfig {
    pub max_output_tokens: usize,
    pub max_response_tokens: usize,
}

impl ResponseConfig {
    pub fn from_env() -> Self {
        Self {
            max_output_tokens: super::helpers::require_env_parsed("MAX_OUTPUT_TOKENS"),
            max_response_tokens: super::helpers::require_env_parsed("MIRA_MAX_RESPONSE_TOKENS"),
        }
    }
}
