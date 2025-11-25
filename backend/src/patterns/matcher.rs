// src/patterns/matcher.rs
// Pattern matching - find applicable patterns for a given context

use anyhow::{Context, Result};
use serde::Deserialize;
use std::sync::Arc;
use tracing::{debug, info};

use crate::llm::provider::{Gpt5Provider, LlmProvider, Message, ReasoningEffort};

use super::storage::PatternStorage;
use super::types::*;

/// Configuration for pattern matching
#[derive(Debug, Clone)]
pub struct MatcherConfig {
    /// Minimum score to consider a match
    pub min_match_score: f64,
    /// Maximum patterns to return
    pub max_matches: usize,
    /// Use LLM for semantic matching
    pub use_llm_matching: bool,
}

impl Default for MatcherConfig {
    fn default() -> Self {
        Self {
            min_match_score: 0.5,
            max_matches: 3,
            use_llm_matching: true,
        }
    }
}

/// Pattern matcher finds applicable patterns for a context
pub struct PatternMatcher {
    storage: Arc<PatternStorage>,
    llm: Option<Gpt5Provider>,
    config: MatcherConfig,
}

impl PatternMatcher {
    pub fn new(storage: Arc<PatternStorage>) -> Self {
        Self {
            storage,
            llm: None,
            config: MatcherConfig::default(),
        }
    }

    pub fn with_llm(mut self, llm: Gpt5Provider) -> Self {
        self.llm = Some(llm);
        self
    }

    pub fn with_config(mut self, config: MatcherConfig) -> Self {
        self.config = config;
        self
    }

    /// Find patterns that match the given context
    pub async fn find_matches(&self, context: &MatchContext) -> Result<Vec<PatternMatch>> {
        // Get candidate patterns
        let patterns = self.storage.get_recommended_patterns(20).await?;

        if patterns.is_empty() {
            debug!("No patterns available for matching");
            return Ok(Vec::new());
        }

        let mut matches = Vec::new();

        for pattern in patterns {
            let (score, reasons) = self.score_pattern(&pattern, context).await?;

            if score >= self.config.min_match_score {
                matches.push(PatternMatch {
                    pattern,
                    match_score: score,
                    match_reasons: reasons,
                });
            }
        }

        // Sort by score descending
        matches.sort_by(|a, b| b.match_score.partial_cmp(&a.match_score).unwrap());

        // Limit results
        matches.truncate(self.config.max_matches);

        info!("Found {} matching patterns", matches.len());
        Ok(matches)
    }

    /// Score how well a pattern matches the context
    async fn score_pattern(
        &self,
        pattern: &ReasoningPattern,
        context: &MatchContext,
    ) -> Result<(f64, Vec<String>)> {
        let mut score = 0.0;
        let mut reasons = Vec::new();
        let mut checks = 0;

        // Keyword matching
        if !context.keywords.is_empty() {
            checks += 1;
            let keyword_score = self.keyword_match_score(pattern, context);
            if keyword_score > 0.0 {
                score += keyword_score;
                reasons.push(format!("Keywords match ({:.0}%)", keyword_score * 100.0));
            }
        }

        // File type matching
        if let Some(ext) = context.file_extension() {
            checks += 1;
            if pattern.applicable_contexts.file_types.contains(&ext) {
                score += 1.0;
                reasons.push(format!("File type matches: .{}", ext));
            }
        }

        // Error code matching
        if let Some(ref code) = context.error_code {
            checks += 1;
            if pattern.applicable_contexts.error_codes.contains(code) {
                score += 1.0;
                reasons.push(format!("Error code matches: {}", code));
            }
        }

        // Intent matching
        if let Some(ref intent) = context.intent {
            checks += 1;
            if pattern.applicable_contexts.intents.iter().any(|i| {
                i.to_lowercase().contains(&intent.to_lowercase())
                    || intent.to_lowercase().contains(&i.to_lowercase())
            }) {
                score += 1.0;
                reasons.push(format!("Intent matches: {}", intent));
            }
        }

        // LLM semantic matching (if enabled and we have an LLM)
        if self.config.use_llm_matching && self.llm.is_some() && context.message.is_some() {
            checks += 1;
            let llm_score = self.llm_match_score(pattern, context).await?;
            if llm_score > 0.5 {
                score += llm_score;
                reasons.push(format!("Semantic match ({:.0}%)", llm_score * 100.0));
            }
        }

        // Normalize score
        let final_score = if checks > 0 {
            score / checks as f64
        } else {
            // No checks performed, use pattern's success rate as baseline
            pattern.success_rate * 0.5
        };

        // Boost by pattern success rate
        let boosted_score = final_score * (0.5 + pattern.success_rate * 0.5);

        Ok((boosted_score.min(1.0), reasons))
    }

    /// Score keyword matching
    fn keyword_match_score(&self, pattern: &ReasoningPattern, context: &MatchContext) -> f64 {
        if context.keywords.is_empty() || pattern.applicable_contexts.keywords.is_empty() {
            return 0.0;
        }

        let context_keywords: Vec<String> = context
            .keywords
            .iter()
            .map(|k| k.to_lowercase())
            .collect();

        let pattern_keywords: Vec<String> = pattern
            .applicable_contexts
            .keywords
            .iter()
            .map(|k| k.to_lowercase())
            .collect();

        let matches: usize = context_keywords
            .iter()
            .filter(|k| pattern_keywords.iter().any(|pk| pk.contains(k.as_str()) || k.contains(pk.as_str())))
            .count();

        matches as f64 / context_keywords.len() as f64
    }

    /// Score using LLM semantic matching
    async fn llm_match_score(
        &self,
        pattern: &ReasoningPattern,
        context: &MatchContext,
    ) -> Result<f64> {
        let llm = match &self.llm {
            Some(l) => l,
            None => return Ok(0.0),
        };

        let message = match &context.message {
            Some(m) => m,
            None => return Ok(0.0),
        };

        let system_prompt = r#"You are a pattern matching assistant. Given a user's request and a coding pattern, determine how well the pattern applies.

Return a JSON object with:
- score: A number from 0.0 to 1.0 indicating match quality
- reason: Brief explanation of why (one sentence)

Example response:
{"score": 0.85, "reason": "User is adding a database migration which matches this pattern"}
"#;

        let user_prompt = format!(
            r#"User request: "{}"

Pattern name: {}
Pattern description: {}
Pattern trigger: {}

How well does this pattern match the user's request?
"#,
            message,
            pattern.name,
            pattern.description,
            pattern.trigger_type.as_str()
        );

        let messages = vec![Message::user(user_prompt)];

        let response = llm
            .chat(messages, system_prompt.to_string())
            .await
            .context("LLM pattern matching failed")?;

        // Parse response
        let content = &response.content;

        // Extract JSON from response
        let json_str = if content.contains("```json") {
            content
                .split("```json")
                .nth(1)
                .and_then(|s| s.split("```").next())
                .unwrap_or(content)
        } else if content.contains("```") {
            content
                .split("```")
                .nth(1)
                .and_then(|s| s.split("```").next())
                .unwrap_or(content)
        } else {
            content
        };

        #[derive(Deserialize)]
        struct MatchResponse {
            score: f64,
            #[allow(dead_code)]
            reason: String,
        }

        match serde_json::from_str::<MatchResponse>(json_str.trim()) {
            Ok(resp) => Ok(resp.score.clamp(0.0, 1.0)),
            Err(_) => {
                debug!("Failed to parse LLM match response: {}", content);
                Ok(0.0)
            }
        }
    }

    /// Find best single match
    pub async fn find_best_match(&self, context: &MatchContext) -> Result<Option<PatternMatch>> {
        let matches = self.find_matches(context).await?;
        Ok(matches.into_iter().next())
    }

    /// Check if any pattern matches
    pub async fn has_match(&self, context: &MatchContext) -> Result<bool> {
        let matches = self.find_matches(context).await?;
        Ok(!matches.is_empty())
    }

    /// Extract keywords from text
    pub fn extract_keywords(text: &str) -> Vec<String> {
        // Common programming keywords to look for
        let important_words = [
            "add", "create", "remove", "delete", "update", "fix", "refactor",
            "migrate", "migration", "test", "debug", "api", "endpoint", "route",
            "function", "class", "struct", "interface", "type", "error", "bug",
            "database", "query", "schema", "model", "component", "service",
            "auth", "authentication", "authorization", "config", "configuration",
            "deploy", "build", "compile", "lint", "format", "optimize",
        ];

        let text_lower = text.to_lowercase();
        let words: Vec<&str> = text_lower
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| w.len() > 2)
            .collect();

        words
            .into_iter()
            .filter(|w| important_words.contains(w))
            .map(|w| w.to_string())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn create_test_storage() -> Arc<PatternStorage> {
        let pool = SqlitePoolOptions::new()
            .connect(":memory:")
            .await
            .unwrap();

        sqlx::query(
            r#"
            CREATE TABLE reasoning_patterns (
                id TEXT PRIMARY KEY,
                project_id TEXT,
                name TEXT NOT NULL,
                description TEXT NOT NULL,
                trigger_type TEXT NOT NULL,
                reasoning_chain TEXT NOT NULL,
                solution_template TEXT,
                applicable_contexts TEXT,
                success_rate REAL DEFAULT 1.0,
                use_count INTEGER DEFAULT 1,
                success_count INTEGER DEFAULT 0,
                cost_savings_usd REAL DEFAULT 0.0,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                last_used INTEGER
            )
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            r#"
            CREATE TABLE reasoning_steps (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                pattern_id TEXT NOT NULL,
                step_number INTEGER NOT NULL,
                step_type TEXT NOT NULL,
                description TEXT NOT NULL,
                rationale TEXT,
                created_at INTEGER NOT NULL
            )
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            r#"
            CREATE TABLE pattern_usage (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                pattern_id TEXT NOT NULL,
                operation_id TEXT,
                user_id TEXT,
                context_match_score REAL,
                applied_successfully BOOLEAN NOT NULL,
                outcome_notes TEXT,
                time_saved_ms INTEGER,
                cost_saved_usd REAL,
                used_at INTEGER NOT NULL
            )
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        Arc::new(PatternStorage::new(Arc::new(pool)))
    }

    #[test]
    fn test_keyword_extraction() {
        let text = "I need to create a new migration for the database schema";
        let keywords = PatternMatcher::extract_keywords(text);

        assert!(keywords.contains(&"create".to_string()));
        assert!(keywords.contains(&"migration".to_string()));
        assert!(keywords.contains(&"database".to_string()));
        assert!(keywords.contains(&"schema".to_string()));
    }

    #[tokio::test]
    async fn test_keyword_matching() {
        let storage = create_test_storage().await;

        // Create a pattern with keywords
        let mut pattern = ReasoningPattern::new(
            "db_migration".to_string(),
            "Database migration pattern".to_string(),
            TriggerType::Keyword,
            "Check -> Create -> Test".to_string(),
        );
        pattern.applicable_contexts.keywords = vec![
            "migration".to_string(),
            "database".to_string(),
            "schema".to_string(),
        ];
        pattern.success_rate = 0.9;
        pattern.use_count = 10;
        pattern.success_count = 9;

        storage.store_pattern(&pattern).await.unwrap();

        let matcher = PatternMatcher::new(storage)
            .with_config(MatcherConfig {
                min_match_score: 0.3,
                max_matches: 5,
                use_llm_matching: false,
            });

        let context = MatchContext::new()
            .with_message("Add a new database migration")
            .with_keywords(vec!["migration".to_string(), "database".to_string()]);

        let matches = matcher.find_matches(&context).await.unwrap();
        assert!(!matches.is_empty());
        assert!(matches[0].match_score > 0.3);
    }
}
