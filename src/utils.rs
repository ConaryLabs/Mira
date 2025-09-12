// src/utils.rs
// Utility functions module

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::warn;

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
