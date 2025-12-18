//! Shared limits and thresholds
//!
//! Centralized constants to prevent drift between mira and mira-chat.

/// Max bytes to return inline (below this, don't artifact)
pub const INLINE_MAX_BYTES: usize = 2048;

/// Threshold above which outputs should be artifacted
pub const ARTIFACT_THRESHOLD_BYTES: usize = 4096;

/// Maximum artifact size (10MB) - prevents unbounded storage/allocation
pub const MAX_ARTIFACT_SIZE: usize = 10 * 1024 * 1024;

/// Default fetch limit for artifact retrieval
pub const DEFAULT_FETCH_LIMIT: usize = 16 * 1024; // 16KB

/// Max grep matches to include in excerpts
pub const MAX_GREP_MATCHES: usize = 20;

/// Max diff files to include in excerpts
pub const MAX_DIFF_FILES: usize = 10;

/// Excerpt head size (chars)
pub const EXCERPT_HEAD_CHARS: usize = 1200;

/// Excerpt tail size (chars)
pub const EXCERPT_TAIL_CHARS: usize = 800;

/// TTL for tool output artifacts (7 days)
pub const TTL_TOOL_OUTPUT_SECS: i64 = 7 * 24 * 60 * 60;

/// TTL for diff artifacts (30 days)
pub const TTL_DIFF_SECS: i64 = 30 * 24 * 60 * 60;

/// TTL for artifacts containing secrets (24 hours)
pub const TTL_SECRET_SECS: i64 = 24 * 60 * 60;

/// Gemini embedding dimensions
pub const EMBEDDING_DIM: u64 = 3072;

/// HTTP timeout for external API calls
pub const HTTP_TIMEOUT_SECS: u64 = 30;

/// Retry attempts for embedding API
pub const EMBED_RETRY_ATTEMPTS: u32 = 2;

/// Delay between retries (ms)
pub const RETRY_DELAY_MS: u64 = 500;

/// Max size for sync endpoint messages
pub const SYNC_MAX_MESSAGE_BYTES: usize = 32 * 1024;

/// Project size cap for artifacts (100MB)
pub const PROJECT_ARTIFACT_CAP_BYTES: i64 = 100 * 1024 * 1024;
