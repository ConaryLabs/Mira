// tests/common/mod.rs
// Shared test utilities and configuration

use std::env;

/// Get Google API key for tests - REQUIRED, panics if not set
pub fn google_api_key() -> String {
    env::var("GOOGLE_API_KEY").expect(
        "GOOGLE_API_KEY environment variable is required for tests. \
         Set it to run integration tests against the real LLM."
    )
}

/// Check if running with real API keys
pub fn has_real_api_keys() -> bool {
    env::var("GOOGLE_API_KEY").is_ok()
}
