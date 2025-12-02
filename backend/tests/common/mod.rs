// tests/common/mod.rs
// Shared test utilities and configuration

use std::env;

/// Get test API key from environment or use placeholder
pub fn get_test_api_key(env_var: &str) -> String {
    env::var(env_var).unwrap_or_else(|_| "test-key-placeholder".to_string())
}

/// Get Google API key for tests (from environment or placeholder)
pub fn google_api_key() -> String {
    get_test_api_key("GOOGLE_API_KEY")
}

/// Check if running with real API keys
pub fn has_real_api_keys() -> bool {
    env::var("GOOGLE_API_KEY").is_ok()
}

/// Skip test if API keys are not available (for integration tests only)
#[macro_export]
macro_rules! skip_without_api_keys {
    () => {
        if !$crate::common::has_real_api_keys() {
            eprintln!("Skipping test: GOOGLE_API_KEY not set");
            return;
        }
    };
}
