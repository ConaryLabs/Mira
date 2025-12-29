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

/// Excerpt head size (chars) - kept small to minimize context burn
pub const EXCERPT_HEAD_CHARS: usize = 500;

/// Excerpt tail size (chars) - kept small to minimize context burn
pub const EXCERPT_TAIL_CHARS: usize = 500;

/// TTL for tool output artifacts (7 days)
pub const TTL_TOOL_OUTPUT_SECS: i64 = 7 * 24 * 60 * 60;

/// TTL for diff artifacts (30 days)
pub const TTL_DIFF_SECS: i64 = 30 * 24 * 60 * 60;

/// TTL for artifacts containing secrets (24 hours)
pub const TTL_SECRET_SECS: i64 = 24 * 60 * 60;

// ============================================================================
// Semantic Search Scaling Controls
// ============================================================================

/// Gemini embedding dimensions
pub const EMBEDDING_DIM: u64 = 3072;

/// HTTP timeout for external API calls
pub const HTTP_TIMEOUT_SECS: u64 = 30;

/// Retry attempts for embedding API
pub const EMBED_RETRY_ATTEMPTS: u32 = 2;

/// Delay between retries (ms)
pub const RETRY_DELAY_MS: u64 = 500;

/// Max items per embedding batch (Gemini limit is 100, we use 50 for safety margin)
pub const EMBED_BATCH_MAX: usize = 50;

/// Max text length for single embedding (Gemini limit is ~10k tokens, we use 8k chars)
pub const EMBED_TEXT_MAX_CHARS: usize = 8000;

/// Default search result limit
pub const SEMANTIC_SEARCH_DEFAULT_LIMIT: usize = 10;

/// Max search result limit (prevents excessive memory/latency)
pub const SEMANTIC_SEARCH_MAX_LIMIT: usize = 100;

/// Minimum similarity score for search results (0.0-1.0)
pub const SEMANTIC_SEARCH_MIN_SCORE: f32 = 0.3;

/// Max size for sync endpoint messages
pub const SYNC_MAX_MESSAGE_BYTES: usize = 32 * 1024;

/// Project size cap for artifacts (100MB)
pub const PROJECT_ARTIFACT_CAP_BYTES: i64 = 100 * 1024 * 1024;

// ============================================================================
// Chain Reset Thresholds
// ============================================================================

/// Soft reset threshold (12k) - triggers if cache% is also low
/// Keeps context tight to reduce token burn
pub const CHAIN_RESET_TOKEN_THRESHOLD: u32 = 12_000;

/// Hard reset ceiling (15k) - always reset regardless of cache%
/// Aggressive ceiling to prevent runaway context accumulation
pub const CHAIN_RESET_HARD_CEILING: u32 = 15_000;

/// Minimum cache percentage to avoid soft reset (30%)
/// If cache% is above this, we don't soft-reset even if tokens are high
pub const CHAIN_RESET_MIN_CACHE_PCT: u32 = 30;

/// Consecutive low-cache turns required before soft reset (hysteresis)
/// Prevents flappy resets from one bad turn
pub const CHAIN_RESET_HYSTERESIS_TURNS: u32 = 2;

/// Minimum turns between resets (cooldown)
pub const CHAIN_RESET_COOLDOWN_TURNS: u32 = 3;
