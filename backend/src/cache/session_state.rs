// backend/src/cache/session_state.rs
// Session-level cache state for LLM-side prompt caching optimization
//
// Tracks what was sent to OpenAI per session to enable:
// 1. Incremental context updates (only send what changed)
// 2. Cache hit rate monitoring
// 3. TTL-aware context decisions

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;

/// Default TTL window for assuming OpenAI cache is still warm (5 minutes)
pub const DEFAULT_CACHE_WARM_WINDOW_SECS: i64 = 300;

/// Tracks what was sent to OpenAI for a session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionCacheState {
    /// Session ID this state belongs to
    pub session_id: String,

    /// Hash of the static prefix (persona + env + tools + guidelines)
    /// If this changes, the entire OpenAI cache is invalidated
    pub static_prefix_hash: String,

    /// When the last LLM call was made (for TTL estimation)
    pub last_call_at: DateTime<Utc>,

    /// Hashes of dynamic context sections sent in last call
    pub context_hashes: ContextHashes,

    /// Estimated tokens in the static prefix (for monitoring)
    pub static_prefix_tokens: i64,

    /// Actual cached_tokens reported by OpenAI in last response
    pub last_reported_cached_tokens: i64,

    /// Total requests made in this session
    pub total_requests: i64,

    /// Total tokens that were cache hits (from OpenAI)
    pub total_cached_tokens: i64,
}

/// Hashes of each dynamic context section
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ContextHashes {
    /// Hash of project context section
    pub project_context: Option<String>,

    /// Hash of memory context section
    pub memory_context: Option<String>,

    /// Hash of code intelligence section
    pub code_intelligence: Option<String>,

    /// Hash of file context section
    pub file_context: Option<String>,

    /// Hashes of individual file contents (path -> hash)
    pub file_contents: HashMap<String, FileContentHash>,
}

/// Track file content with hash for incremental updates
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileContentHash {
    /// File path
    pub path: String,

    /// SHA-256 hash of file content
    pub content_hash: String,

    /// Estimated token count for this file
    pub token_estimate: i64,

    /// When this content was last sent to OpenAI
    pub sent_at: DateTime<Utc>,
}

impl SessionCacheState {
    /// Create a new session cache state
    pub fn new(session_id: String, static_prefix_hash: String, static_prefix_tokens: i64) -> Self {
        Self {
            session_id,
            static_prefix_hash,
            last_call_at: Utc::now(),
            context_hashes: ContextHashes::default(),
            static_prefix_tokens,
            last_reported_cached_tokens: 0,
            total_requests: 0,
            total_cached_tokens: 0,
        }
    }

    /// Check if the OpenAI cache is likely still warm
    ///
    /// OpenAI caches prompts for ~5-10 minutes. We use 5 minutes as a conservative estimate.
    pub fn is_cache_likely_warm(&self) -> bool {
        self.is_cache_warm_with_ttl(DEFAULT_CACHE_WARM_WINDOW_SECS)
    }

    /// Check if cache is warm with custom TTL window
    pub fn is_cache_warm_with_ttl(&self, ttl_seconds: i64) -> bool {
        let elapsed = Utc::now().signed_duration_since(self.last_call_at);
        elapsed.num_seconds() < ttl_seconds
    }

    /// Check if static prefix has changed (cache invalidation)
    pub fn static_prefix_changed(&self, new_hash: &str) -> bool {
        self.static_prefix_hash != new_hash
    }

    /// Update state after an LLM call
    pub fn update_after_call(
        &mut self,
        context_hashes: ContextHashes,
        cached_tokens: i64,
    ) {
        self.last_call_at = Utc::now();
        self.context_hashes = context_hashes;
        self.last_reported_cached_tokens = cached_tokens;
        self.total_requests += 1;
        self.total_cached_tokens += cached_tokens;
    }

    /// Get the cache hit rate for this session
    pub fn cache_hit_rate(&self) -> f64 {
        if self.total_requests == 0 {
            return 0.0;
        }
        // This is an approximation based on cached tokens vs estimated static prefix
        // A more accurate calculation would need total input tokens
        if self.static_prefix_tokens > 0 {
            (self.total_cached_tokens as f64)
                / (self.total_requests as f64 * self.static_prefix_tokens as f64)
        } else {
            0.0
        }
    }

    /// Generate SHA-256 hash for content
    pub fn hash_content(content: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(content.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    /// Estimate token count for content (rough approximation: 4 chars per token)
    pub fn estimate_tokens(content: &str) -> i64 {
        (content.len() as f64 / 4.0).ceil() as i64
    }
}

impl ContextHashes {
    /// Check if a specific section hash matches
    pub fn section_matches(&self, section: &str, hash: &str) -> bool {
        match section {
            "project" => self.project_context.as_deref() == Some(hash),
            "memory" => self.memory_context.as_deref() == Some(hash),
            "code_intelligence" => self.code_intelligence.as_deref() == Some(hash),
            "file" => self.file_context.as_deref() == Some(hash),
            _ => false,
        }
    }

    /// Check if a file content hash matches
    pub fn file_matches(&self, path: &str, hash: &str) -> bool {
        self.file_contents
            .get(path)
            .map(|h| h.content_hash == hash)
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_content() {
        let hash1 = SessionCacheState::hash_content("hello world");
        let hash2 = SessionCacheState::hash_content("hello world");
        let hash3 = SessionCacheState::hash_content("hello world!");

        assert_eq!(hash1, hash2);
        assert_ne!(hash1, hash3);
        assert_eq!(hash1.len(), 64); // SHA-256 produces 64 hex chars
    }

    #[test]
    fn test_cache_warm_check() {
        let state = SessionCacheState::new(
            "test-session".to_string(),
            "abc123".to_string(),
            1200,
        );

        assert!(state.is_cache_likely_warm());
        assert!(state.is_cache_warm_with_ttl(600));
    }

    #[test]
    fn test_static_prefix_change_detection() {
        let state = SessionCacheState::new(
            "test-session".to_string(),
            "original-hash".to_string(),
            1200,
        );

        assert!(!state.static_prefix_changed("original-hash"));
        assert!(state.static_prefix_changed("different-hash"));
    }

    #[test]
    fn test_estimate_tokens() {
        // ~4 chars per token
        assert_eq!(SessionCacheState::estimate_tokens("hello world"), 3); // 11 chars
        assert_eq!(SessionCacheState::estimate_tokens("a"), 1);
        assert_eq!(SessionCacheState::estimate_tokens(""), 0);
    }

    #[test]
    fn test_context_hashes_matching() {
        let mut hashes = ContextHashes::default();
        hashes.project_context = Some("proj-hash".to_string());
        hashes.file_contents.insert(
            "src/main.rs".to_string(),
            FileContentHash {
                path: "src/main.rs".to_string(),
                content_hash: "file-hash".to_string(),
                token_estimate: 100,
                sent_at: Utc::now(),
            },
        );

        assert!(hashes.section_matches("project", "proj-hash"));
        assert!(!hashes.section_matches("project", "wrong-hash"));
        assert!(!hashes.section_matches("memory", "anything"));

        assert!(hashes.file_matches("src/main.rs", "file-hash"));
        assert!(!hashes.file_matches("src/main.rs", "wrong-hash"));
        assert!(!hashes.file_matches("src/other.rs", "file-hash"));
    }
}
