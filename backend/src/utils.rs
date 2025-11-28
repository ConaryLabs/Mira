// src/utils.rs
// Minimal utility functions - only what's actually needed

use anyhow::Result;
use futures::Future;
use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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
