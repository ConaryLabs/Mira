// crates/mira-server/src/http.rs
// Shared HTTP client for all network operations

use std::time::Duration;

/// Default request timeout (5 minutes for LLM operations)
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(300);

/// Default connect timeout
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// Fast operation timeout (for embeddings, quick API calls)
pub const FAST_TIMEOUT: Duration = Duration::from_secs(30);

/// Create the shared HTTP client with appropriate defaults.
///
/// This client should be created once at startup and passed to all
/// modules that need HTTP access. Uses connection pooling internally.
pub fn create_shared_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(DEFAULT_TIMEOUT)
        .connect_timeout(CONNECT_TIMEOUT)
        .pool_max_idle_per_host(10)
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_shared_client() {
        let client = create_shared_client();
        // Just verify it creates successfully
        drop(client);
    }

    #[test]
    fn test_timeout_values() {
        assert_eq!(DEFAULT_TIMEOUT, Duration::from_secs(300));
        assert_eq!(CONNECT_TIMEOUT, Duration::from_secs(30));
        assert_eq!(FAST_TIMEOUT, Duration::from_secs(30));
    }
}
