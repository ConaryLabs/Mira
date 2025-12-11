// tests/common/mod.rs
// Shared test utilities and configuration

use std::env;
use std::sync::Once;

static INIT: Once = Once::new();

/// Initialize test environment (loads .env file)
pub fn init() {
    INIT.call_once(|| {
        dotenv::dotenv().ok();
    });
}

/// Get OpenAI API key for tests - REQUIRED, panics if not set
pub fn openai_api_key() -> String {
    init();
    env::var("OPENAI_API_KEY").expect(
        "OPENAI_API_KEY environment variable is required for tests. \
         Set it to run integration tests against the real LLM."
    )
}

/// Check if running with real API keys
pub fn has_real_api_keys() -> bool {
    init();
    env::var("OPENAI_API_KEY").is_ok()
}
