// src/utils.rs
// Minimal utility functions - only what's actually needed

use anyhow::Result;
use futures::Future;
use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tracing::warn;

// Rate limiting support
use governor::clock::DefaultClock;
use governor::state::{InMemoryState, NotKeyed};
use governor::{Jitter, Quota, RateLimiter as GovRateLimiter};

// ============================================================================
// Timestamp utilities
// ============================================================================

/// Get current timestamp in seconds
pub fn get_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

/// Get current timestamp in milliseconds
pub fn get_timestamp_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis()
}

// ============================================================================
// Rate limiting utilities
// ============================================================================

pub struct RateLimiter {
    limiter: Arc<GovRateLimiter<NotKeyed, InMemoryState, DefaultClock>>,
    jitter: Jitter,
}

impl RateLimiter {
    /// Create a new rate limiter with requests per minute
    pub fn new(requests_per_minute: u32) -> Result<Self> {
        let quota = Quota::per_minute(
            NonZeroU32::new(requests_per_minute)
                .ok_or_else(|| anyhow::anyhow!("Invalid rate limit"))?,
        );

        Ok(Self {
            limiter: Arc::new(GovRateLimiter::direct(quota)),
            jitter: Jitter::new(Duration::from_millis(10), Duration::from_millis(100)),
        })
    }

    /// Wait until we can make a request
    pub async fn acquire(&self) -> Result<()> {
        self.limiter.until_ready_with_jitter(self.jitter).await;
        Ok(())
    }

    /// Check if we can make a request without waiting
    pub fn try_acquire(&self) -> bool {
        self.limiter.check().is_ok()
    }
}

// ============================================================================
// Retry utilities
// ============================================================================

pub struct RetryConfig {
    pub max_retries: u32,
    pub initial_interval: Duration,
    pub multiplier: f64,
    pub max_interval: Duration,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_interval: Duration::from_millis(500),
            multiplier: 2.0,
            max_interval: Duration::from_secs(10),
        }
    }
}

/// Retry with exponential backoff for transient errors
pub async fn retry_with_backoff<F, Fut, T>(mut operation: F, config: RetryConfig) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T>>,
{
    let mut retry_count = 0;
    let mut delay = config.initial_interval;

    loop {
        match operation().await {
            Ok(value) => return Ok(value),
            Err(e) => {
                let error_str = format!("{:?}", e);

                // Check if error is retryable
                let is_retryable = error_str.contains("429")
                    || error_str.contains("500")
                    || error_str.contains("502")
                    || error_str.contains("503");

                if is_retryable && retry_count < config.max_retries {
                    retry_count += 1;
                    warn!(
                        "Operation failed (attempt {}/{}), retrying in {:?}",
                        retry_count, config.max_retries, delay
                    );

                    tokio::time::sleep(delay).await;

                    // Exponential backoff
                    delay = Duration::from_millis(
                        (delay.as_millis() as f64 * config.multiplier) as u64,
                    )
                    .min(config.max_interval);
                } else {
                    return Err(e);
                }
            }
        }
    }
}

// ============================================================================
// Timeout utilities
// ============================================================================

/// Execute an operation with a timeout
pub async fn with_timeout<F, T>(duration: Duration, operation: F, operation_name: &str) -> Result<T>
where
    F: Future<Output = Result<T>>,
{
    match tokio::time::timeout(duration, operation).await {
        Ok(result) => result,
        Err(_) => Err(anyhow::anyhow!(
            "{} timed out after {:?}",
            operation_name,
            duration
        )),
    }
}
