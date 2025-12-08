// src/utils/rate_limiter.rs
// Rate limiting utilities

use anyhow::Result;
use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::Duration;

use governor::clock::DefaultClock;
use governor::state::{InMemoryState, NotKeyed};
use governor::{Jitter, Quota, RateLimiter as GovRateLimiter};

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
