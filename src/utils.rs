// src/utils.rs
// Utility functions module

use std::time::{SystemTime, UNIX_EPOCH};

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
