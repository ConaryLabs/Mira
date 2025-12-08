// src/utils/hash.rs
// Centralized SHA-256 hashing utility

use sha2::{Digest, Sha256};

/// Compute SHA-256 hash of a string and return hex-encoded result
///
/// Used for content deduplication, cache keys, and change detection throughout the codebase.
pub fn sha256_hash(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Compute SHA-256 hash of bytes and return hex-encoded result
pub fn sha256_hash_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

/// Estimate token count for content (rough approximation: ~4 chars per token)
///
/// This is a fast heuristic used for budget/context window estimation.
/// For precise counts, use the actual tokenizer.
pub fn estimate_tokens(content: &str) -> i64 {
    (content.len() as f64 / 4.0).ceil() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sha256_hash() {
        let hash = sha256_hash("hello world");
        assert_eq!(hash.len(), 64); // SHA-256 hex is 64 characters

        // Same content produces same hash
        let hash2 = sha256_hash("hello world");
        assert_eq!(hash, hash2);

        // Different content produces different hash
        let hash3 = sha256_hash("hello world!");
        assert_ne!(hash, hash3);
    }

    #[test]
    fn test_sha256_hash_bytes() {
        let hash = sha256_hash_bytes(b"hello world");
        assert_eq!(hash.len(), 64);

        // Should match string version
        let hash_str = sha256_hash("hello world");
        assert_eq!(hash, hash_str);
    }

    #[test]
    fn test_estimate_tokens() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("test"), 1); // 4 chars = 1 token
        assert_eq!(estimate_tokens("hello world"), 3); // 11 chars = ~3 tokens
    }
}
