// crates/mira-server/src/context/analytics.rs
// Analytics and learning for context injection

use crate::db::pool::DatabasePool;
use crate::db::set_server_state_sync;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::Mutex;

use super::InjectionSource;

/// Event recorded when context is injected
#[derive(Debug, Clone)]
pub struct InjectionEvent {
    pub session_id: String,
    pub project_id: Option<i64>,
    pub sources: Vec<InjectionSource>,
    pub context_len: usize,
    pub message_preview: String,
    /// Key terms extracted from the injected context for feedback tracking
    pub key_terms: Vec<String>,
}

/// In-memory analytics for injection events
/// Persists summary stats to database periodically
pub struct InjectionAnalytics {
    pool: Arc<DatabasePool>,
    events: Mutex<Vec<InjectionEvent>>,
    total_injections: Mutex<u64>,
    total_chars: Mutex<u64>,
}

impl InjectionAnalytics {
    pub fn new(pool: Arc<DatabasePool>) -> Self {
        Self {
            pool,
            events: Mutex::new(Vec::new()),
            total_injections: Mutex::new(0),
            total_chars: Mutex::new(0),
        }
    }

    /// Record an injection event and persist feedback tracking data
    pub async fn record(&self, event: InjectionEvent) {
        let context_len = event.context_len;

        // Persist feedback tracking row to database
        if !event.key_terms.is_empty() {
            let session_id = event.session_id.clone();
            let project_id = event.project_id;
            let sources: Vec<String> = event.sources.iter().map(|s| s.name().to_string()).collect();
            let key_terms = event.key_terms.clone();
            let ctx_len = event.context_len;

            let pool = self.pool.clone();
            if let Err(e) = pool
                .interact(move |conn| {
                    insert_injection_feedback_sync(
                        conn,
                        &session_id,
                        project_id,
                        &sources,
                        &key_terms,
                        ctx_len,
                    )
                })
                .await
            {
                tracing::debug!("Failed to persist injection feedback row: {}", e);
            }
        }

        // Update counters
        {
            let mut total = self.total_injections.lock().await;
            *total += 1;
        }
        {
            let mut chars = self.total_chars.lock().await;
            *chars += context_len as u64;
        }

        // Store event (keep last 100)
        {
            let mut events = self.events.lock().await;
            events.push(event);
            if events.len() > 100 {
                events.remove(0);
            }
        }

        // Persist stats every 10 injections
        let count = *self.total_injections.lock().await;
        if count % 10 == 0 {
            self.persist_stats().await;
        }
    }

    /// Check a response text against recent injections and record feedback.
    ///
    /// Uses a simple keyword overlap heuristic: if any key terms from the
    /// injected context appear in the response, the injection is marked as
    /// referenced.
    pub async fn record_response_feedback(&self, session_id: &str, response_text: &str) {
        let session_id_owned = session_id.to_string();
        let response_lower = response_text.to_lowercase();

        let pool = self.pool.clone();
        if let Err(e) = pool
            .interact(move |conn| {
                update_injection_feedback_sync(conn, &session_id_owned, &response_lower)
            })
            .await
        {
            tracing::debug!("Failed to update injection feedback: {}", e);
        }
    }

    /// Persist stats to database
    async fn persist_stats(&self) {
        let total = *self.total_injections.lock().await;
        let chars = *self.total_chars.lock().await;

        let total_str = total.to_string();
        let chars_str = chars.to_string();

        if let Err(e) = self
            .pool
            .interact(move |conn| {
                set_server_state_sync(conn, "injection_total_count", &total_str)
                    .map_err(|e| anyhow::anyhow!("{}", e))?;
                set_server_state_sync(conn, "injection_total_chars", &chars_str)
                    .map_err(|e| anyhow::anyhow!("{}", e))?;
                Ok::<_, anyhow::Error>(())
            })
            .await
        {
            tracing::debug!("Failed to persist injection stats: {}", e);
        }
    }

    /// Get analytics summary
    pub async fn summary(&self, _project_id: Option<i64>) -> String {
        let total = *self.total_injections.lock().await;
        let chars = *self.total_chars.lock().await;
        let events = self.events.lock().await;

        if total == 0 {
            return "No context injections recorded yet.".to_string();
        }

        let avg_chars = if total > 0 { chars / total } else { 0 };

        // Count source usage
        let mut semantic_count = 0u64;
        let mut file_count = 0u64;
        let mut task_count = 0u64;
        let mut convention_count = 0u64;

        for event in events.iter() {
            for source in &event.sources {
                match source {
                    InjectionSource::Semantic => semantic_count += 1,
                    InjectionSource::FileAware => file_count += 1,
                    InjectionSource::TaskAware => task_count += 1,
                    InjectionSource::Convention => convention_count += 1,
                }
            }
        }

        format!(
            "Injection analytics:\n  Total: {} injections, {} chars ({} avg)\n  Sources: semantic={}, files={}, tasks={}, convention={}",
            total, chars, avg_chars, semantic_count, file_count, task_count, convention_count
        )
    }

    /// Mark that injected context was useful (e.g., user acted on it)
    /// This can be used for learning which contexts are valuable
    pub async fn mark_useful(&self, session_id: &str) {
        // For now, just log - future: update weights for injection strategies
        tracing::debug!("Context injection marked useful for session {}", session_id);
    }

    /// Get recent events for debugging
    pub async fn recent_events(&self, limit: usize) -> Vec<InjectionEvent> {
        let events = self.events.lock().await;
        events.iter().rev().take(limit).cloned().collect()
    }
}

/// Extract key terms from injected context for feedback tracking.
///
/// Pulls out identifiers (function names, struct names, file paths) and
/// significant words that can be matched against Claude's response to
/// determine if the injected context was actually used.
pub fn extract_key_terms(context: &str) -> Vec<String> {
    let mut terms = HashSet::new();

    for line in context.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("---") || trimmed.starts_with("===") {
            continue;
        }

        for word in trimmed.split_whitespace() {
            let clean = word.trim_matches(|c: char| {
                c.is_ascii_punctuation() && c != '_' && c != '/' && c != '.'
            });

            if clean.is_empty() || clean.len() < 4 {
                continue;
            }

            // File paths (contains / and a dot-extension)
            if clean.contains('/') && clean.contains('.') {
                terms.insert(clean.to_lowercase());
                continue;
            }

            // Identifiers: snake_case or CamelCase names (at least 4 chars)
            if clean.contains('_')
                || (clean.len() >= 4
                    && clean
                        .chars()
                        .next()
                        .is_some_and(|c| c.is_ascii_alphabetic())
                    && clean.chars().any(|c| c.is_ascii_uppercase())
                    && clean.chars().any(|c| c.is_ascii_lowercase()))
            {
                terms.insert(clean.to_lowercase());
                continue;
            }

            // Significant domain words (6+ chars, not common filler)
            if clean.len() >= 6 && clean.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                let lower = clean.to_lowercase();
                if !is_common_filler(&lower) {
                    terms.insert(lower);
                }
            }
        }
    }

    let mut result: Vec<String> = terms.into_iter().collect();
    result.sort();
    // Cap at 30 terms to keep the storage reasonable
    result.truncate(30);
    result
}

/// Common words that are too generic to be useful as feedback terms
fn is_common_filler(word: &str) -> bool {
    matches!(
        word,
        "should"
            | "could"
            | "would"
            | "before"
            | "after"
            | "because"
            | "between"
            | "through"
            | "during"
            | "without"
            | "within"
            | "return"
            | "returns"
            | "string"
            | "number"
            | "option"
            | "result"
            | "default"
            | "current"
            | "following"
            | "example"
            | "include"
            | "includes"
            | "already"
            | "however"
            | "provide"
            | "provides"
            | "require"
            | "requires"
            | "available"
            | "created"
            | "updated"
    )
}

// -- Database operations for injection feedback --

/// Insert a new injection feedback row (pending feedback).
fn insert_injection_feedback_sync(
    conn: &rusqlite::Connection,
    session_id: &str,
    project_id: Option<i64>,
    sources: &[String],
    key_terms: &[String],
    context_len: usize,
) -> anyhow::Result<()> {
    let sources_json = serde_json::to_string(sources)?;
    let terms_json = serde_json::to_string(key_terms)?;

    conn.execute(
        "INSERT INTO injection_feedback
            (session_id, project_id, sources, key_terms, context_len)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![
            session_id,
            project_id,
            sources_json,
            terms_json,
            context_len as i64
        ],
    )?;
    Ok(())
}

/// Check pending injection feedback rows for a session against a response.
///
/// For each pending row, compute keyword overlap and mark as referenced or not.
fn update_injection_feedback_sync(
    conn: &rusqlite::Connection,
    session_id: &str,
    response_lower: &str,
) -> anyhow::Result<()> {
    // Fetch pending feedback rows for this session
    let mut stmt = conn.prepare(
        "SELECT id, key_terms FROM injection_feedback
         WHERE session_id = ?1 AND was_referenced IS NULL
         ORDER BY created_at DESC
         LIMIT 10",
    )?;

    let rows: Vec<(i64, String)> = stmt
        .query_map(rusqlite::params![session_id], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?
        .filter_map(|r| r.ok())
        .collect();

    for (id, terms_json) in rows {
        let terms: Vec<String> = serde_json::from_str(&terms_json).unwrap_or_default();
        if terms.is_empty() {
            continue;
        }

        let matched: Vec<&String> = terms
            .iter()
            .filter(|term| response_lower.contains(term.as_str()))
            .collect();

        let was_referenced = !matched.is_empty();
        let matched_count = matched.len() as i64;
        let total_count = terms.len() as i64;
        let overlap_ratio = if total_count > 0 {
            matched_count as f64 / total_count as f64
        } else {
            0.0
        };

        conn.execute(
            "UPDATE injection_feedback
             SET was_referenced = ?1, matched_terms = ?2, overlap_ratio = ?3,
                 checked_at = datetime('now')
             WHERE id = ?4",
            rusqlite::params![was_referenced, matched_count, overlap_ratio, id],
        )?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_analytics_record() {
        let pool = Arc::new(DatabasePool::open_in_memory().await.unwrap());
        let analytics = InjectionAnalytics::new(pool);

        analytics
            .record(InjectionEvent {
                session_id: "test-session".to_string(),
                project_id: Some(1),
                sources: vec![InjectionSource::Semantic],
                context_len: 100,
                message_preview: "test message".to_string(),
                key_terms: vec![],
            })
            .await;

        let summary = analytics.summary(None).await;
        assert!(summary.contains("1 injections"));
        assert!(summary.contains("100 chars"));
    }

    #[tokio::test]
    async fn test_analytics_summary_empty() {
        let pool = Arc::new(DatabasePool::open_in_memory().await.unwrap());
        let analytics = InjectionAnalytics::new(pool);

        let summary = analytics.summary(None).await;
        assert!(summary.contains("No context injections"));
    }

    #[tokio::test]
    async fn test_recent_events() {
        let pool = Arc::new(DatabasePool::open_in_memory().await.unwrap());
        let analytics = InjectionAnalytics::new(pool);

        for i in 0..5 {
            analytics
                .record(InjectionEvent {
                    session_id: format!("session-{}", i),
                    project_id: None,
                    sources: vec![],
                    context_len: i * 10,
                    message_preview: format!("message {}", i),
                    key_terms: vec![],
                })
                .await;
        }

        let recent = analytics.recent_events(3).await;
        assert_eq!(recent.len(), 3);
        assert_eq!(recent[0].session_id, "session-4"); // Most recent first
    }

    #[test]
    fn test_extract_key_terms_identifiers() {
        let context =
            "The DatabasePool struct handles connections.\nSee inject_context for details.";
        let terms = extract_key_terms(context);
        assert!(terms.contains(&"databasepool".to_string()));
        assert!(terms.contains(&"inject_context".to_string()));
    }

    #[test]
    fn test_extract_key_terms_file_paths() {
        let context = "Modified crates/mira-server/src/context/analytics.rs recently.";
        let terms = extract_key_terms(context);
        assert!(terms.contains(&"crates/mira-server/src/context/analytics.rs".to_string()));
    }

    #[test]
    fn test_extract_key_terms_skips_short_words() {
        let context = "The fn is ok and we use it.";
        let terms = extract_key_terms(context);
        // All words are too short (< 4 chars) to be key terms
        assert!(terms.is_empty());
    }

    #[test]
    fn test_extract_key_terms_skips_filler() {
        let context = "should provide returns default current already";
        let terms = extract_key_terms(context);
        assert!(terms.is_empty());
    }

    #[test]
    fn test_extract_key_terms_caps_at_30() {
        // Generate context with many unique identifiers
        let lines: Vec<String> = (0..50).map(|i| format!("function_name_{}", i)).collect();
        let context = lines.join(" ");
        let terms = extract_key_terms(&context);
        assert!(terms.len() <= 30);
    }

    #[tokio::test]
    async fn test_record_and_feedback() {
        let pool = Arc::new(DatabasePool::open_in_memory().await.unwrap());
        let analytics = InjectionAnalytics::new(pool.clone());

        // Record an injection with key terms (project_id: None to avoid FK constraint)
        analytics
            .record(InjectionEvent {
                session_id: "feedback-sess".to_string(),
                project_id: None,
                sources: vec![InjectionSource::Semantic],
                context_len: 200,
                message_preview: "test feedback".to_string(),
                key_terms: vec!["databasepool".to_string(), "inject_context".to_string()],
            })
            .await;

        // Check that the feedback row was inserted
        let count: i64 = pool
            .interact(|conn| {
                conn.query_row(
                    "SELECT COUNT(*) FROM injection_feedback WHERE session_id = 'feedback-sess'",
                    [],
                    |row| row.get(0),
                )
                .map_err(Into::into)
            })
            .await
            .unwrap();
        assert_eq!(count, 1);

        // Simulate response that references one of the key terms
        analytics
            .record_response_feedback(
                "feedback-sess",
                "We should update the databasepool configuration.",
            )
            .await;

        // Check that feedback was updated
        let (was_ref, matched, ratio): (bool, i64, f64) = pool
            .interact(|conn| {
                conn.query_row(
                    "SELECT was_referenced, matched_terms, overlap_ratio
                     FROM injection_feedback WHERE session_id = 'feedback-sess'",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .map_err(Into::into)
            })
            .await
            .unwrap();

        assert!(was_ref);
        assert_eq!(matched, 1);
        assert!((ratio - 0.5).abs() < 0.01);
    }

    #[tokio::test]
    async fn test_feedback_not_referenced() {
        let pool = Arc::new(DatabasePool::open_in_memory().await.unwrap());
        let analytics = InjectionAnalytics::new(pool.clone());

        analytics
            .record(InjectionEvent {
                session_id: "no-ref-sess".to_string(),
                project_id: None,
                sources: vec![InjectionSource::Convention],
                context_len: 50,
                message_preview: "test".to_string(),
                key_terms: vec!["unique_identifier_xyz".to_string()],
            })
            .await;

        // Response that does NOT reference the key term
        analytics
            .record_response_feedback(
                "no-ref-sess",
                "Just a generic response about something else.",
            )
            .await;

        let was_ref: bool = pool
            .interact(|conn| {
                conn.query_row(
                    "SELECT was_referenced FROM injection_feedback WHERE session_id = 'no-ref-sess'",
                    [],
                    |row| row.get(0),
                )
                .map_err(Into::into)
            })
            .await
            .unwrap();

        assert!(!was_ref);
    }
}
