//! Gemini Orchestrator - Intelligent context management for Mira
//!
//! Uses Gemini Flash/Pro to:
//! - Route queries to relevant context categories
//! - Summarize context within token budgets
//! - Extract decisions from transcripts
//! - Manage debouncing and caching
//!
//! Hybrid approach: inline for fast routing (<500ms), background for heavy lifting.

mod types;
mod worker;
pub mod task_type;

pub use types::*;
pub use worker::OrchestratorWorker;
pub use task_type::TaskType;

use anyhow::Result;
use chrono::Utc;
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, info, warn};

use crate::context::ContextCategory;

/// Gemini Orchestrator - central hub for intelligent context management
pub struct GeminiOrchestrator {
    /// Database pool for persistence
    db: SqlitePool,
    /// Gemini API key
    gemini_key: String,
    /// Configuration
    config: OrchestratorConfig,
    /// In-memory routing cache (LRU)
    routing_cache: Arc<RwLock<HashMap<String, (RoutingDecision, Instant)>>>,
    /// Pre-computed category summaries
    category_summaries: Arc<RwLock<HashMap<ContextCategory, CategorySummary>>>,
    /// Job submission channel for background worker
    job_tx: Option<mpsc::Sender<OrchestratorJob>>,
    /// HTTP client for Gemini API
    client: reqwest::Client,
}

impl GeminiOrchestrator {
    /// Create a new orchestrator
    pub async fn new(db: SqlitePool, gemini_key: String) -> Result<Self> {
        let config = OrchestratorConfig::from_env();

        info!(
            "GeminiOrchestrator initialized (routing={}, extraction={}, summarization={})",
            config.routing_enabled, config.extraction_enabled, config.summarization_enabled
        );

        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(config.routing_timeout_ms * 2))
            .build()?;

        Ok(Self {
            db,
            gemini_key,
            config,
            routing_cache: Arc::new(RwLock::new(HashMap::new())),
            category_summaries: Arc::new(RwLock::new(HashMap::new())),
            job_tx: None,
            client,
        })
    }

    /// Check if the orchestrator is available (has API key)
    pub fn is_available(&self) -> bool {
        !self.gemini_key.is_empty()
    }

    /// Get the current configuration
    pub fn config(&self) -> &OrchestratorConfig {
        &self.config
    }

    // ========================================================================
    // Routing
    // ========================================================================

    /// Route a query to relevant context categories
    ///
    /// Uses Gemini Flash for classification with aggressive caching.
    /// Falls back to keyword matching if Gemini is unavailable or times out.
    pub async fn route(&self, query: &str) -> RoutingDecision {
        let start = Instant::now();

        // Check if routing is enabled
        if !self.config.routing_enabled || !self.is_available() {
            return self.route_fallback(query, start);
        }

        // Check memory cache first
        let query_hash = hash_query(query);
        if let Some(cached) = self.check_routing_cache(&query_hash).await {
            return RoutingDecision {
                cached: true,
                latency_ms: start.elapsed().as_millis() as u64,
                source: RoutingSource::MemoryCache,
                ..cached
            };
        }

        // Call Gemini with timeout
        let timeout = Duration::from_millis(self.config.routing_timeout_ms);
        match tokio::time::timeout(timeout, self.route_with_gemini(query)).await {
            Ok(Ok(mut decision)) => {
                decision.latency_ms = start.elapsed().as_millis() as u64;
                decision.cached = false;
                decision.source = RoutingSource::Gemini;

                // Cache the decision
                self.cache_routing_decision(&query_hash, decision.clone())
                    .await;

                decision
            }
            Ok(Err(e)) => {
                warn!("Gemini routing error: {}", e);
                self.route_fallback(query, start)
            }
            Err(_) => {
                warn!("Gemini routing timeout ({}ms)", self.config.routing_timeout_ms);
                self.route_fallback(query, start)
            }
        }
    }

    /// Check the memory cache for a routing decision
    async fn check_routing_cache(&self, query_hash: &str) -> Option<RoutingDecision> {
        let cache = self.routing_cache.read().await;
        if let Some((decision, cached_at)) = cache.get(query_hash) {
            let age = cached_at.elapsed();
            if age < Duration::from_secs(self.config.routing_cache_ttl_secs) {
                return Some(decision.clone());
            }
        }
        None
    }

    /// Store a routing decision in the cache
    async fn cache_routing_decision(&self, query_hash: &str, decision: RoutingDecision) {
        let mut cache = self.routing_cache.write().await;

        // Evict old entries if cache is too large
        if cache.len() > 1000 {
            let cutoff = Instant::now() - Duration::from_secs(self.config.routing_cache_ttl_secs);
            cache.retain(|_, (_, cached_at)| *cached_at > cutoff);
        }

        cache.insert(query_hash.to_string(), (decision, Instant::now()));
    }

    /// Route using Gemini Flash API
    async fn route_with_gemini(&self, query: &str) -> Result<RoutingDecision> {
        let prompt = format!(
            r#"Classify this user query into the most relevant context category.

Categories:
- goals: Active goals, milestones, progress tracking
- decisions: Past decisions, architectural choices, rationale
- memories: User preferences, recurring patterns, learned context
- git_activity: Recent commits, branches, code changes
- code_context: File structure, symbols, related code
- system_status: Index status, system state, diagnostics
- recent_errors: Build errors, runtime issues, failed tests
- user_patterns: User habits, frequently used commands

Query: "{}"

Respond with JSON only:
{{"primary": "category_name", "secondary": null, "confidence": 0.0-1.0, "reasoning": "brief explanation"}}"#,
            query.replace('"', "\\\"")
        );

        let response = self.call_gemini_flash(&prompt).await?;

        // Parse JSON response
        self.parse_routing_response(&response)
    }

    /// Parse Gemini's routing response
    fn parse_routing_response(&self, response: &str) -> Result<RoutingDecision> {
        // Extract JSON from response (may have markdown code blocks)
        let json_str = response
            .trim()
            .trim_start_matches("```json")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim();

        let parsed: serde_json::Value = serde_json::from_str(json_str)?;

        let primary_str = parsed["primary"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing primary category"))?;

        let primary = self.parse_category(primary_str)?;

        let secondary = parsed["secondary"]
            .as_str()
            .and_then(|s| self.parse_category(s).ok());

        let confidence = parsed["confidence"].as_f64().unwrap_or(0.5) as f32;

        let reasoning = parsed["reasoning"]
            .as_str()
            .unwrap_or("No reasoning provided")
            .to_string();

        Ok(RoutingDecision {
            primary,
            secondary,
            confidence,
            reasoning,
            cached: false,
            latency_ms: 0,
            source: RoutingSource::Gemini,
        })
    }

    /// Parse category string to enum
    fn parse_category(&self, s: &str) -> Result<ContextCategory> {
        match s.to_lowercase().replace('_', "").as_str() {
            "goals" => Ok(ContextCategory::Goals),
            "decisions" => Ok(ContextCategory::Decisions),
            "memories" => Ok(ContextCategory::Memories),
            "gitactivity" | "git" => Ok(ContextCategory::GitActivity),
            "codecontext" | "code" => Ok(ContextCategory::CodeContext),
            "systemstatus" | "system" => Ok(ContextCategory::SystemStatus),
            "recenterrors" | "errors" => Ok(ContextCategory::RecentErrors),
            "userpatterns" | "patterns" => Ok(ContextCategory::UserPatterns),
            _ => Err(anyhow::anyhow!("Unknown category: {}", s)),
        }
    }

    /// Fallback to keyword-based routing
    fn route_fallback(&self, query: &str, start: Instant) -> RoutingDecision {
        let query_lower = query.to_lowercase();

        // Simple keyword matching (mirrors carousel logic)
        let category = if query_lower.contains("error")
            || query_lower.contains("fail")
            || query_lower.contains("bug")
            || query_lower.contains("fix")
        {
            ContextCategory::RecentErrors
        } else if query_lower.contains("goal")
            || query_lower.contains("milestone")
            || query_lower.contains("progress")
        {
            ContextCategory::Goals
        } else if query_lower.contains("decide")
            || query_lower.contains("decision")
            || query_lower.contains("chose")
            || query_lower.contains("approach")
        {
            ContextCategory::Decisions
        } else if query_lower.contains("commit")
            || query_lower.contains("branch")
            || query_lower.contains("git")
        {
            ContextCategory::GitActivity
        } else if query_lower.contains("file")
            || query_lower.contains("function")
            || query_lower.contains("code")
            || query_lower.contains("implement")
        {
            ContextCategory::CodeContext
        } else if query_lower.contains("remember")
            || query_lower.contains("preference")
            || query_lower.contains("always")
        {
            ContextCategory::Memories
        } else {
            // Default to code context for most queries
            ContextCategory::CodeContext
        };

        RoutingDecision {
            primary: category,
            secondary: None,
            confidence: 0.5,
            reasoning: "Keyword-based fallback".to_string(),
            cached: false,
            latency_ms: start.elapsed().as_millis() as u64,
            source: RoutingSource::KeywordFallback,
        }
    }

    // ========================================================================
    // Debounce
    // ========================================================================

    /// Check if an action should proceed based on debounce rules
    ///
    /// Returns true if NOT debounced (action should proceed).
    /// Atomically updates the last-triggered timestamp.
    pub async fn check_debounce(&self, key: &str, ttl_secs: u64) -> bool {
        let now = Utc::now().timestamp();

        // Check current state
        let result = sqlx::query_as::<_, (i64,)>(
            "SELECT last_triggered FROM debounce_state WHERE key = $1",
        )
        .bind(key)
        .fetch_optional(&self.db)
        .await;

        match result {
            Ok(Some((last_triggered,))) => {
                if now - last_triggered < ttl_secs as i64 {
                    debug!("Debounced: {} ({}s remaining)", key, ttl_secs as i64 - (now - last_triggered));
                    return false;
                }
            }
            Ok(None) => {
                // No previous entry, will proceed
            }
            Err(e) => {
                warn!("Debounce check failed: {} - proceeding anyway", e);
                return true; // Fail open
            }
        }

        // Update state
        let update_result = sqlx::query(
            "INSERT INTO debounce_state (key, last_triggered, trigger_count)
             VALUES ($1, $2, 1)
             ON CONFLICT(key) DO UPDATE SET
                 last_triggered = excluded.last_triggered,
                 trigger_count = trigger_count + 1",
        )
        .bind(key)
        .bind(now)
        .execute(&self.db)
        .await;

        if let Err(e) = update_result {
            warn!("Debounce update failed: {}", e);
        }

        true
    }

    /// Clean up old debounce entries
    pub async fn cleanup_debounce(&self, max_age_secs: i64) -> Result<usize> {
        let cutoff = Utc::now().timestamp() - max_age_secs;

        let result = sqlx::query("DELETE FROM debounce_state WHERE last_triggered < $1")
            .bind(cutoff)
            .execute(&self.db)
            .await?;

        Ok(result.rows_affected() as usize)
    }

    // ========================================================================
    // Summarization
    // ========================================================================

    /// Get a pre-computed category summary if available
    pub async fn get_category_summary(&self, category: ContextCategory) -> Option<CategorySummary> {
        let summaries = self.category_summaries.read().await;
        summaries.get(&category).cloned()
    }

    /// Summarize content to fit within a token budget
    pub async fn summarize(&self, content: &str, token_budget: usize) -> Result<SummarizedContext> {
        let original_tokens = estimate_tokens(content);

        // If already within budget, return as-is
        if original_tokens <= token_budget {
            return Ok(SummarizedContext {
                content: content.to_string(),
                original_tokens,
                compressed_tokens: original_tokens,
                preserved_keys: vec![],
                generated_at: Utc::now(),
            });
        }

        // Not enabled or no API key - truncate instead
        if !self.config.summarization_enabled || !self.is_available() {
            let truncated = truncate_to_tokens(content, token_budget);
            return Ok(SummarizedContext {
                content: truncated,
                original_tokens,
                compressed_tokens: token_budget,
                preserved_keys: vec!["[truncated]".to_string()],
                generated_at: Utc::now(),
            });
        }

        // Use Gemini to summarize
        let prompt = format!(
            r#"Summarize this context in {} tokens or less. Keep the most actionable items.
Preserve key decisions, active goals, and critical facts.

Content:
{}

Respond with only the summary, no explanation."#,
            token_budget, content
        );

        let summary = self.call_gemini_flash(&prompt).await?;
        let compressed_tokens = estimate_tokens(&summary);

        Ok(SummarizedContext {
            content: summary,
            original_tokens,
            compressed_tokens,
            preserved_keys: vec![],
            generated_at: Utc::now(),
        })
    }

    // ========================================================================
    // Extraction
    // ========================================================================

    /// Extract decisions and topics from a transcript
    pub async fn extract(&self, transcript: &str) -> Result<ExtractionResult> {
        if !self.config.extraction_enabled || !self.is_available() {
            return self.extract_fallback(transcript);
        }

        let prompt = format!(
            r#"Extract structured information from this conversation transcript.

Output JSON:
{{
  "decisions": [{{"content": "...", "confidence": 0.9, "type": "technical|architectural|approach|rejection", "context": "..."}}],
  "topics": ["topic1", "topic2"],
  "files_modified": ["path/to/file.rs"],
  "insights": ["key insight for future sessions"]
}}

Rules:
1. Only extract ACTUAL decisions (not plans or maybes)
2. Topics should be specific (not generic like "coding")
3. Files must be actual paths mentioned
4. Insights should be actionable for future sessions
5. Confidence 0.7+ only

Transcript (first 3000 chars):
{}

Respond with JSON only."#,
            truncate_to_chars(transcript, 3000)
        );

        let response = self.call_gemini_flash(&prompt).await?;
        self.parse_extraction_response(&response)
    }

    /// Parse extraction response from Gemini
    fn parse_extraction_response(&self, response: &str) -> Result<ExtractionResult> {
        let json_str = response
            .trim()
            .trim_start_matches("```json")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim();

        let parsed: serde_json::Value = serde_json::from_str(json_str)?;

        let decisions = parsed["decisions"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|d| {
                        Some(ExtractedDecision {
                            content: d["content"].as_str()?.to_string(),
                            confidence: d["confidence"].as_f64().unwrap_or(0.7) as f32,
                            decision_type: DecisionType::from_str(
                                d["type"].as_str().unwrap_or("approach"),
                            )
                            .unwrap_or(DecisionType::Approach),
                            context: d["context"].as_str().unwrap_or("").to_string(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        let topics = parsed["topics"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|t| t.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        let files_modified = parsed["files_modified"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|f| f.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        let insights = parsed["insights"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|i| i.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        Ok(ExtractionResult {
            decisions,
            topics,
            files_modified,
            insights,
            confidence: 0.8,
        })
    }

    /// Fallback extraction using string patterns
    fn extract_fallback(&self, transcript: &str) -> Result<ExtractionResult> {
        let mut decisions = Vec::new();
        let patterns = [
            "I'll ",
            "I will ",
            "Let's ",
            "We should ",
            "Going to ",
            "I'm going to ",
            "I decided to ",
            "The approach is to ",
            "Using ",
            "Switching to ",
            "Implementing ",
            "Creating ",
            "Adding ",
        ];

        for line in transcript.lines() {
            for pattern in &patterns {
                if let Some(idx) = line.to_lowercase().find(&pattern.to_lowercase()) {
                    let start = idx;
                    let rest: String = line[start..].chars().take(150).collect();
                    if rest.len() > 10 {
                        decisions.push(ExtractedDecision {
                            content: rest,
                            confidence: 0.5,
                            decision_type: DecisionType::Approach,
                            context: "Pattern-matched fallback".to_string(),
                        });
                    }
                    break;
                }
            }
        }

        Ok(ExtractionResult {
            decisions,
            topics: vec![],
            files_modified: vec![],
            insights: vec![],
            confidence: 0.5,
        })
    }

    // ========================================================================
    // Gemini API
    // ========================================================================

    /// Call Gemini Flash API
    async fn call_gemini_flash(&self, prompt: &str) -> Result<String> {
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.0-flash:generateContent?key={}",
            self.gemini_key
        );

        let body = serde_json::json!({
            "contents": [{
                "parts": [{"text": prompt}]
            }],
            "generationConfig": {
                "temperature": 0.1,
                "maxOutputTokens": 500
            }
        });

        let response = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await?
            .error_for_status()?;

        let json: serde_json::Value = response.json().await?;

        let text = json["candidates"][0]["content"]["parts"][0]["text"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("No text in Gemini response"))?;

        Ok(text.to_string())
    }
}

// ============================================================================
// Helpers
// ============================================================================

/// Truncate string to approximate token count
fn truncate_to_tokens(s: &str, tokens: usize) -> String {
    let char_limit = tokens * 4;
    if s.len() <= char_limit {
        s.to_string()
    } else {
        s.chars().take(char_limit).collect()
    }
}

/// Truncate string to character count
fn truncate_to_chars(s: &str, chars: usize) -> String {
    if s.len() <= chars {
        s.to_string()
    } else {
        s.chars().take(chars).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_to_tokens() {
        let s = "hello world";
        assert_eq!(truncate_to_tokens(s, 100), s);
        assert_eq!(truncate_to_tokens(s, 2).len(), 8); // 2 tokens * 4 chars
    }

    #[tokio::test]
    async fn test_route_fallback() {
        // Create a mock orchestrator without API key
        let db = sqlx::SqlitePool::connect(":memory:").await.unwrap();
        let orch = GeminiOrchestrator::new(db, String::new()).await.unwrap();

        let decision = orch.route("fix the error in auth").await;
        assert_eq!(decision.primary, ContextCategory::RecentErrors);
        assert_eq!(decision.source, RoutingSource::KeywordFallback);

        let decision = orch.route("what's the goal status").await;
        assert_eq!(decision.primary, ContextCategory::Goals);
    }
}
