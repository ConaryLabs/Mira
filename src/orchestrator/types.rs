//! Orchestrator types for Gemini-powered context management
//!
//! Defines the core data structures for routing, summarization, and extraction.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::context::ContextCategory;

// ============================================================================
// Routing Types
// ============================================================================

/// Result of context routing - which categories are relevant for a query
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingDecision {
    /// Primary category to inject
    pub primary: ContextCategory,
    /// Secondary category (optional, for multi-focus)
    pub secondary: Option<ContextCategory>,
    /// Confidence score (0.0-1.0)
    pub confidence: f32,
    /// Brief reasoning (for observability)
    pub reasoning: String,
    /// Whether this was a cache hit
    pub cached: bool,
    /// Latency in milliseconds
    pub latency_ms: u64,
    /// Source of the decision
    pub source: RoutingSource,
}

/// Where the routing decision came from
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum RoutingSource {
    /// Fresh Gemini API call
    Gemini,
    /// Hit in memory LRU cache
    MemoryCache,
    /// Hit via embedding similarity in Qdrant
    SemanticCache,
    /// Fallback to keyword matching
    KeywordFallback,
}

/// Cached routing entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingCacheEntry {
    pub query_hash: String,
    pub category: ContextCategory,
    pub confidence: f32,
    pub created_at: i64,
    pub hits: i32,
}

// ============================================================================
// Summarization Types
// ============================================================================

/// Summarized context blob with token accounting
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SummarizedContext {
    /// Compressed content
    pub content: String,
    /// Original token count (estimated)
    pub original_tokens: usize,
    /// Compressed token count (estimated)
    pub compressed_tokens: usize,
    /// Key items preserved in summary
    pub preserved_keys: Vec<String>,
    /// When this summary was generated
    pub generated_at: DateTime<Utc>,
}

/// Pre-computed category summary for carousel
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategorySummary {
    /// Which category this summarizes
    pub category: ContextCategory,
    /// The summarized content
    pub content: String,
    /// Token count of the summary
    pub token_count: usize,
    /// When this was generated
    pub generated_at: DateTime<Utc>,
    /// Project ID if project-scoped
    pub project_id: Option<i64>,
}

// ============================================================================
// Extraction Types
// ============================================================================

/// Result of extracting decisions/topics from a transcript
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionResult {
    /// Decisions made during the session
    pub decisions: Vec<ExtractedDecision>,
    /// Topics discussed
    pub topics: Vec<String>,
    /// Files that were modified
    pub files_modified: Vec<String>,
    /// Key insights for future context
    pub insights: Vec<String>,
    /// Overall confidence in extraction
    pub confidence: f32,
}

/// A single extracted decision
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedDecision {
    /// The decision content
    pub content: String,
    /// Confidence this is a real decision (0.0-1.0)
    pub confidence: f32,
    /// Type of decision
    pub decision_type: DecisionType,
    /// Surrounding context
    pub context: String,
}

/// Types of decisions that can be extracted
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DecisionType {
    /// Technical implementation choice ("Using async/await")
    Technical,
    /// Architectural decision ("Splitting into modules")
    Architectural,
    /// Approach/strategy ("Going with incremental migration")
    Approach,
    /// Rejection of an approach ("Not using X because...")
    Rejection,
}

impl DecisionType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Technical => "technical",
            Self::Architectural => "architectural",
            Self::Approach => "approach",
            Self::Rejection => "rejection",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "technical" => Some(Self::Technical),
            "architectural" => Some(Self::Architectural),
            "approach" => Some(Self::Approach),
            "rejection" => Some(Self::Rejection),
            _ => None,
        }
    }
}

// ============================================================================
// Worker Types
// ============================================================================

/// Jobs that can be submitted to the background worker
#[derive(Debug)]
pub enum OrchestratorJob {
    /// Extract decisions from a transcript
    ExtractDecisions {
        transcript: String,
        session_id: String,
        callback: tokio::sync::oneshot::Sender<ExtractionResult>,
    },

    /// Pre-summarize a category for carousel
    SummarizeCategory {
        category: ContextCategory,
        token_budget: usize,
        project_id: Option<i64>,
    },

    /// Cache a routing decision for future queries
    CacheRouting {
        query: String,
        decision: RoutingDecision,
    },

    /// Periodic housekeeping (cleanup old entries)
    Housekeeping,
}

// ============================================================================
// Debounce Types
// ============================================================================

/// Debounce entry stored in database
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebounceEntry {
    pub key: String,
    pub last_triggered: i64,
    pub trigger_count: i32,
    pub context: Option<String>,
}

// ============================================================================
// Configuration
// ============================================================================

/// Configuration for the orchestrator
#[derive(Debug, Clone)]
pub struct OrchestratorConfig {
    /// Enable Gemini-powered routing (vs keyword fallback)
    pub routing_enabled: bool,
    /// Enable Gemini-powered extraction (vs string matching)
    pub extraction_enabled: bool,
    /// Enable Gemini-powered summarization
    pub summarization_enabled: bool,
    /// Token budget for category summaries
    pub summary_token_budget: usize,
    /// Routing cache TTL in seconds
    pub routing_cache_ttl_secs: u64,
    /// Routing similarity threshold for cache hits
    pub routing_similarity_threshold: f32,
    /// Timeout for inline routing in milliseconds
    pub routing_timeout_ms: u64,
    /// Job queue size for background worker
    pub job_queue_size: usize,
    /// Pre-summarization interval in seconds
    pub summarize_interval_secs: u64,
}

impl Default for OrchestratorConfig {
    fn default() -> Self {
        Self {
            routing_enabled: true,
            extraction_enabled: true,
            summarization_enabled: true,
            summary_token_budget: 300,
            routing_cache_ttl_secs: 300, // 5 minutes
            routing_similarity_threshold: 0.85,
            routing_timeout_ms: 500,
            job_queue_size: 100,
            summarize_interval_secs: 30,
        }
    }
}

impl OrchestratorConfig {
    /// Load config from environment variables
    pub fn from_env() -> Self {
        let mut config = Self::default();

        if let Ok(val) = std::env::var("MIRA_GEMINI_ROUTING") {
            config.routing_enabled = val == "1" || val.to_lowercase() == "true";
        }
        if let Ok(val) = std::env::var("MIRA_GEMINI_EXTRACTION") {
            config.extraction_enabled = val == "1" || val.to_lowercase() == "true";
        }
        if let Ok(val) = std::env::var("MIRA_GEMINI_SUMMARIZATION") {
            config.summarization_enabled = val == "1" || val.to_lowercase() == "true";
        }
        if let Ok(val) = std::env::var("MIRA_ROUTING_TIMEOUT_MS") {
            if let Ok(ms) = val.parse() {
                config.routing_timeout_ms = ms;
            }
        }
        if let Ok(val) = std::env::var("MIRA_ROUTING_CACHE_TTL") {
            if let Ok(secs) = val.parse() {
                config.routing_cache_ttl_secs = secs;
            }
        }

        config
    }
}

// ============================================================================
// Utility Functions
// ============================================================================

/// Estimate token count from string (rough approximation: 4 chars per token)
pub fn estimate_tokens(s: &str) -> usize {
    s.len() / 4
}

/// Hash a query string for cache lookup
pub fn hash_query(query: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    // Normalize: lowercase, trim, collapse whitespace
    let normalized: String = query
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    let mut hasher = DefaultHasher::new();
    normalized.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_query_normalization() {
        // Same content, different whitespace should produce same hash
        let h1 = hash_query("  hello   world  ");
        let h2 = hash_query("hello world");
        assert_eq!(h1, h2);

        // Case insensitive
        let h3 = hash_query("Hello World");
        assert_eq!(h1, h3);
    }

    #[test]
    fn test_estimate_tokens() {
        assert_eq!(estimate_tokens("hello world"), 2); // 11 chars / 4 = 2
        assert_eq!(estimate_tokens(""), 0);
    }

    #[test]
    fn test_decision_type_roundtrip() {
        for dt in [
            DecisionType::Technical,
            DecisionType::Architectural,
            DecisionType::Approach,
            DecisionType::Rejection,
        ] {
            let s = dt.as_str();
            let parsed = DecisionType::from_str(s);
            assert_eq!(parsed, Some(dt));
        }
    }
}
