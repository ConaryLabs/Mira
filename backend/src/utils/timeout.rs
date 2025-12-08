// src/utils/timeout.rs
// Timeout utilities

use anyhow::Result;
use futures::Future;
use std::time::Duration;

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
