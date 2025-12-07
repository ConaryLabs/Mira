// src/config/testing.rs
// Configuration for testing mode

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Testing configuration for mock mode
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestingConfig {
    /// Whether mock mode is enabled (MIRA_MOCK_MODE)
    pub mock_mode: bool,

    /// Path to recording file for mock responses (MIRA_MOCK_RECORDING)
    pub recording_path: Option<PathBuf>,

    /// Match strategy for mock mode (MIRA_MOCK_STRATEGY): exact, last_user, fuzzy, sequential
    pub match_strategy: String,
}

impl TestingConfig {
    pub fn from_env() -> Self {
        Self {
            mock_mode: std::env::var("MIRA_MOCK_MODE")
                .map(|v| v == "true" || v == "1")
                .unwrap_or(false),

            recording_path: std::env::var("MIRA_MOCK_RECORDING")
                .ok()
                .map(PathBuf::from),

            match_strategy: std::env::var("MIRA_MOCK_STRATEGY")
                .unwrap_or_else(|_| "sequential".to_string()),
        }
    }
}

impl Default for TestingConfig {
    fn default() -> Self {
        Self {
            mock_mode: false,
            recording_path: None,
            match_strategy: "sequential".to_string(),
        }
    }
}
