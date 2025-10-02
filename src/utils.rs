// src/utils.rs
// Utility functions module

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH, Duration};
use std::sync::Arc;
use std::num::NonZeroU32;
use anyhow::Result;
use futures::Future;
use tracing::warn;

// Rate limiting support
use governor::{Quota, RateLimiter as GovRateLimiter, Jitter};
use governor::clock::DefaultClock;
use governor::state::{InMemoryState, NotKeyed};

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
// Path security utilities
// ============================================================================

/// Security check for file system operations
/// Validates that a path is within allowed directories and doesn't contain traversal attempts
pub fn is_path_allowed(path: &Path) -> bool {
    let allowed_prefixes = vec![
        "/home",
        "/tmp",
        "/var/www",
        "./repos",
        "./uploads",
    ];
    
    let path_str = path.to_string_lossy();
    
    // Check for directory traversal attempts
    if path_str.contains("..") {
        warn!("Blocked directory traversal attempt: {}", path_str);
        return false;
    }
    
    // Check if path starts with any allowed prefix
    for prefix in &allowed_prefixes {
        if path_str.starts_with(prefix) {
            return true;
        }
    }
    
    // Also allow relative paths in the current working directory
    if !path.is_absolute() {
        return true;
    }
    
    warn!("Path outside allowed directories: {}", path_str);
    false
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
                .ok_or_else(|| anyhow::anyhow!("Invalid rate limit"))?
        );
        
        Ok(Self {
            limiter: Arc::new(GovRateLimiter::direct(quota)),
            jitter: Jitter::new(
                Duration::from_millis(10),
                Duration::from_millis(100),
            ),
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

/// Simple retry with exponential backoff (without backoff crate complexity)
pub async fn retry_with_backoff<F, Fut, T>(
    mut operation: F,
    config: RetryConfig,
) -> Result<T>
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
                let is_retryable = error_str.contains("429") || 
                                  error_str.contains("500") || 
                                  error_str.contains("502") || 
                                  error_str.contains("503");
                
                if is_retryable && retry_count < config.max_retries {
                    retry_count += 1;
                    warn!("Operation failed (attempt {}/{}), retrying in {:?}", 
                          retry_count, config.max_retries, delay);
                    
                    tokio::time::sleep(delay).await;
                    
                    // Exponential backoff
                    delay = Duration::from_millis(
                        (delay.as_millis() as f64 * config.multiplier) as u64
                    ).min(config.max_interval);
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
pub async fn with_timeout<F, T>(
    duration: Duration,
    operation: F,
    operation_name: &str,
) -> Result<T>
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

// ============================================================================
// Token counting utilities (LLM-agnostic)
// ============================================================================

/// Estimate token count for LLMs (rough approximation)
/// Claude uses similar tokenization, roughly 1 token per 4 characters
pub fn count_tokens(text: &str) -> Result<usize> {
    // Simple approximation based on char count
    // Rough approximation - ~4 chars per token
    Ok((text.len() + 3) / 4)
}

/// Count tokens for multiple texts
pub fn count_tokens_batch(texts: &[String]) -> Result<Vec<usize>> {
    texts.iter()
        .map(|text| count_tokens(text))
        .collect()
}

// ============================================================================
// WebSocket connection guard for cleanup
// ============================================================================

pub struct ConnectionGuard {
    handle: Option<tokio::task::JoinHandle<()>>,
}

impl ConnectionGuard {
    pub fn new(handle: tokio::task::JoinHandle<()>) -> Self {
        Self { handle: Some(handle) }
    }
}

impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            handle.abort();
        }
    }
}

// ============================================================================
// JSON repair utilities
// ============================================================================

/// Try to repair common JSON issues
pub fn repair_json_simple(json_str: &str) -> String {
    json_str
        .replace("'", "\"")           // Single to double quotes
        .replace(",]", "]")            // Trailing commas in arrays
        .replace(",}", "}")            // Trailing commas in objects
        .replace("undefined", "null")  // JavaScript undefined
        .replace("NaN", "null")        // Not a number
}

/// Parse JSON with lenient fallbacks
pub fn parse_json_lenient(json_str: &str) -> Result<serde_json::Value> {
    // Try normal parsing first
    if let Ok(value) = serde_json::from_str(json_str) {
        return Ok(value);
    }
    
    // Try with basic repairs
    let repaired = repair_json_simple(json_str);
    serde_json::from_str(&repaired)
        .map_err(|e| anyhow::anyhow!("Failed to parse JSON even with repairs: {}", e))
}
