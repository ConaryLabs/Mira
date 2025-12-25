// src/context/carousel.rs
//! Carousel-based context rotation
//!
//! Rotates through context categories across tool calls, with critical items
//! always breaking through. State is persisted globally for session continuity.
//!
//! Features:
//! - Throttled rotation (advances every N calls, not every call)
//! - Cold start uses LRU category selection
//! - Pin override to lock a category during focused work
//! - 8 categories including RecentErrors and UserPatterns

use rand::Rng;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::collections::HashMap;

/// How many tool calls before rotating to next category
pub const ROTATION_INTERVAL: u64 = 4;

/// Categories of context that rotate through the carousel
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextCategory {
    /// Active goals and their progress
    Goals,
    /// Recent decisions and their rationale
    Decisions,
    /// Relevant memories (preferences, context)
    Memories,
    /// Recent commits and git activity
    GitActivity,
    /// Code context (related files, symbols)
    CodeContext,
    /// System status (index freshness, etc.)
    SystemStatus,
    /// Recent errors, mispredictions, wrong assumptions
    RecentErrors,
    /// User patterns - recurring topics, preferences
    UserPatterns,
}

impl ContextCategory {
    /// All categories in rotation order
    pub fn rotation() -> &'static [ContextCategory] {
        use ContextCategory::*;
        &[Goals, Decisions, Memories, GitActivity, CodeContext, SystemStatus, RecentErrors, UserPatterns]
    }

    /// Display name for the category
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Goals => "Active Goals",
            Self::Decisions => "Decisions",
            Self::Memories => "Memories",
            Self::GitActivity => "Git Activity",
            Self::CodeContext => "Code Context",
            Self::SystemStatus => "System Status",
            Self::RecentErrors => "Recent Errors",
            Self::UserPatterns => "User Patterns",
        }
    }

    /// Parse from string (for pin command)
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "goals" => Some(Self::Goals),
            "decisions" => Some(Self::Decisions),
            "memories" => Some(Self::Memories),
            "git" | "gitactivity" | "git_activity" => Some(Self::GitActivity),
            "code" | "codecontext" | "code_context" => Some(Self::CodeContext),
            "system" | "systemstatus" | "system_status" => Some(Self::SystemStatus),
            "errors" | "recenterrors" | "recent_errors" => Some(Self::RecentErrors),
            "patterns" | "userpatterns" | "user_patterns" => Some(Self::UserPatterns),
            _ => None,
        }
    }
}

/// Persistent carousel state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CarouselState {
    /// Current position in the rotation (0-based index)
    pub index: usize,
    /// Total tool calls tracked
    pub call_count: u64,
    /// Calls since last rotation (for throttling)
    pub calls_since_advance: u64,
    /// Last time the carousel advanced
    pub last_advanced: i64,
    /// Pinned category (bypasses rotation)
    pub pinned_category: Option<ContextCategory>,
    /// When the pin expires (unix timestamp)
    pub pin_expires_at: Option<i64>,
    /// Last shown timestamp for each category (for LRU)
    #[serde(default)]
    pub category_last_shown: HashMap<String, i64>,
    /// Whether this is a fresh/cold start
    #[serde(default)]
    pub is_cold_start: bool,
}

impl Default for CarouselState {
    fn default() -> Self {
        Self {
            index: 0,
            call_count: 0,
            calls_since_advance: 0,
            last_advanced: 0,
            pinned_category: None,
            pin_expires_at: None,
            category_last_shown: HashMap::new(),
            is_cold_start: true,
        }
    }
}

/// The context carousel - manages rotation through context categories
pub struct ContextCarousel {
    state: CarouselState,
    db: SqlitePool,
    project_id: Option<i64>,
}

impl ContextCarousel {
    /// Load carousel state from database (or create default)
    pub async fn load(db: SqlitePool, project_id: Option<i64>) -> anyhow::Result<Self> {
        // Ensure table exists
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS carousel_state (
                id INTEGER PRIMARY KEY DEFAULT 1,
                state_json TEXT NOT NULL,
                updated_at INTEGER NOT NULL DEFAULT (unixepoch())
            )
            "#,
        )
        .execute(&db)
        .await?;

        // Load existing state or use default
        let mut state: CarouselState = sqlx::query_scalar::<_, String>(
            "SELECT state_json FROM carousel_state WHERE id = 1",
        )
        .fetch_optional(&db)
        .await?
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default();

        // Handle cold start - pick LRU or random category
        if state.is_cold_start || state.category_last_shown.is_empty() {
            state.index = Self::pick_lru_or_random(&state);
            state.is_cold_start = false;
            state.calls_since_advance = 0;
        }

        // Check if pin has expired
        if let Some(expires) = state.pin_expires_at {
            if chrono::Utc::now().timestamp() > expires {
                state.pinned_category = None;
                state.pin_expires_at = None;
            }
        }

        Ok(Self { state, db, project_id })
    }

    /// Pick least-recently-used category, or random if no history
    fn pick_lru_or_random(state: &CarouselState) -> usize {
        let rotation = ContextCategory::rotation();

        if state.category_last_shown.is_empty() {
            // No history - pick random
            let mut rng = rand::rng();
            return rng.random_range(0..rotation.len());
        }

        // Find least recently shown category
        let mut oldest_time = i64::MAX;
        let mut oldest_idx = 0;

        for (idx, cat) in rotation.iter().enumerate() {
            let cat_key = format!("{:?}", cat).to_lowercase();
            let last_shown = state.category_last_shown.get(&cat_key).copied().unwrap_or(0);
            if last_shown < oldest_time {
                oldest_time = last_shown;
                oldest_idx = idx;
            }
        }

        oldest_idx
    }

    /// Get the current category (respects pinning)
    pub fn current(&self) -> ContextCategory {
        // If pinned and not expired, return pinned category
        if let Some(pinned) = self.state.pinned_category {
            if let Some(expires) = self.state.pin_expires_at {
                if chrono::Utc::now().timestamp() <= expires {
                    return pinned;
                }
            }
        }

        let rotation = ContextCategory::rotation();
        rotation[self.state.index % rotation.len()]
    }

    /// Tick the carousel - may advance if throttle interval reached
    /// Returns the current category after potential advancement
    pub async fn tick_and_maybe_advance(&mut self) -> anyhow::Result<ContextCategory> {
        self.state.call_count += 1;
        self.state.calls_since_advance += 1;

        // Record when this category was shown
        let current = self.current();
        let cat_key = format!("{:?}", current).to_lowercase();
        self.state.category_last_shown.insert(cat_key, chrono::Utc::now().timestamp());

        // Check if we should advance (only if not pinned)
        if self.state.pinned_category.is_none() && self.state.calls_since_advance >= ROTATION_INTERVAL {
            let rotation = ContextCategory::rotation();
            self.state.index = (self.state.index + 1) % rotation.len();
            self.state.calls_since_advance = 0;
            self.state.last_advanced = chrono::Utc::now().timestamp();
        }

        self.save().await?;
        Ok(self.current())
    }

    /// Force advance to next category (ignores throttle)
    pub async fn force_advance(&mut self) -> anyhow::Result<ContextCategory> {
        let rotation = ContextCategory::rotation();
        self.state.index = (self.state.index + 1) % rotation.len();
        self.state.calls_since_advance = 0;
        self.state.last_advanced = chrono::Utc::now().timestamp();
        self.state.call_count += 1;

        self.save().await?;
        Ok(self.current())
    }

    /// Increment call count without advancing (for skipped injections)
    pub async fn tick(&mut self) -> anyhow::Result<()> {
        self.state.call_count += 1;
        self.save().await
    }

    /// Pin a category for the specified duration (in minutes)
    pub async fn pin(&mut self, category: ContextCategory, duration_minutes: i64) -> anyhow::Result<()> {
        let expires = chrono::Utc::now().timestamp() + (duration_minutes * 60);
        self.state.pinned_category = Some(category);
        self.state.pin_expires_at = Some(expires);
        self.save().await
    }

    /// Unpin the current category
    pub async fn unpin(&mut self) -> anyhow::Result<()> {
        self.state.pinned_category = None;
        self.state.pin_expires_at = None;
        self.save().await
    }

    /// Check if a category is currently pinned
    pub fn is_pinned(&self) -> Option<(ContextCategory, i64)> {
        if let (Some(cat), Some(expires)) = (self.state.pinned_category, self.state.pin_expires_at) {
            let now = chrono::Utc::now().timestamp();
            if now <= expires {
                return Some((cat, expires - now));
            }
        }
        None
    }

    /// Save state to database
    async fn save(&self) -> anyhow::Result<()> {
        let json = serde_json::to_string(&self.state)?;
        sqlx::query(
            r#"
            INSERT INTO carousel_state (id, state_json, updated_at)
            VALUES (1, $1, unixepoch())
            ON CONFLICT(id) DO UPDATE SET
                state_json = excluded.state_json,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(&json)
        .execute(&self.db)
        .await?;
        Ok(())
    }

    /// Get statistics about carousel usage
    pub fn stats(&self) -> &CarouselState {
        &self.state
    }

    /// Render context for the current category
    /// Returns None if no relevant context exists for this category
    pub async fn render_current(&self) -> anyhow::Result<Option<String>> {
        self.render_category(self.current()).await
    }

    /// Render context for a specific category
    pub async fn render_category(&self, category: ContextCategory) -> anyhow::Result<Option<String>> {
        match category {
            ContextCategory::Goals => self.render_goals().await,
            ContextCategory::Decisions => self.render_decisions().await,
            ContextCategory::Memories => self.render_memories().await,
            ContextCategory::GitActivity => self.render_git_activity().await,
            ContextCategory::CodeContext => self.render_code_context().await,
            ContextCategory::SystemStatus => self.render_system_status().await,
            ContextCategory::RecentErrors => self.render_recent_errors().await,
            ContextCategory::UserPatterns => self.render_user_patterns().await,
        }
    }

    /// Render critical context that always appears (corrections, blocked goals)
    pub async fn render_critical(&self) -> anyhow::Result<Option<String>> {
        let mut parts = Vec::new();

        // Get high-confidence corrections
        let corrections: Vec<(String, String, f64)> = sqlx::query_as(
            r#"
            SELECT what_was_wrong, what_is_right, confidence
            FROM corrections
            WHERE (project_id IS NULL OR project_id = $1)
              AND confidence > 0.8
            ORDER BY confidence DESC, created_at DESC
            LIMIT 3
            "#,
        )
        .bind(self.project_id)
        .fetch_all(&self.db)
        .await
        .unwrap_or_default();

        if !corrections.is_empty() {
            let mut correction_lines = vec!["⚠️ CORRECTIONS:".to_string()];
            for (wrong, right, _conf) in corrections {
                correction_lines.push(format!("  • {} → {}", truncate(&wrong, 40), truncate(&right, 50)));
            }
            parts.push(correction_lines.join("\n"));
        }

        // Get blocked goals
        let blocked: Vec<(String, String)> = sqlx::query_as(
            r#"
            SELECT title, COALESCE(description, '') as desc
            FROM goals
            WHERE (project_id IS NULL OR project_id = $1)
              AND status = 'blocked'
            ORDER BY created_at DESC
            LIMIT 2
            "#,
        )
        .bind(self.project_id)
        .fetch_all(&self.db)
        .await
        .unwrap_or_default();

        if !blocked.is_empty() {
            let mut blocked_lines = vec!["🚫 BLOCKED:".to_string()];
            for (title, _desc) in blocked {
                blocked_lines.push(format!("  • {}", truncate(&title, 60)));
            }
            parts.push(blocked_lines.join("\n"));
        }

        if parts.is_empty() {
            Ok(None)
        } else {
            Ok(Some(parts.join("\n\n")))
        }
    }

    // =========================================================================
    // Category renderers
    // =========================================================================

    async fn render_goals(&self) -> anyhow::Result<Option<String>> {
        let goals: Vec<(String, String, i32)> = sqlx::query_as(
            r#"
            SELECT title, status, progress_percent
            FROM goals
            WHERE (project_id IS NULL OR project_id = $1)
              AND status IN ('planning', 'in_progress')
            ORDER BY
                CASE status WHEN 'in_progress' THEN 0 ELSE 1 END,
                progress_percent DESC
            LIMIT 3
            "#,
        )
        .bind(self.project_id)
        .fetch_all(&self.db)
        .await?;

        if goals.is_empty() {
            return Ok(None);
        }

        let mut lines = vec!["🎯 ACTIVE GOALS:".to_string()];
        for (title, status, progress) in goals {
            let status_icon = if status == "in_progress" { "▶" } else { "○" };
            lines.push(format!("  {} {} ({}%)", status_icon, truncate(&title, 50), progress));
        }

        Ok(Some(lines.join("\n")))
    }

    async fn render_decisions(&self) -> anyhow::Result<Option<String>> {
        let decisions: Vec<(String, String)> = sqlx::query_as(
            r#"
            SELECT key, value
            FROM memory_facts
            WHERE fact_type = 'decision'
              AND (project_id IS NULL OR project_id = $1)
              AND key NOT LIKE 'compaction-%'
            ORDER BY updated_at DESC
            LIMIT 3
            "#,
        )
        .bind(self.project_id)
        .fetch_all(&self.db)
        .await?;

        if decisions.is_empty() {
            return Ok(None);
        }

        let mut lines = vec!["📋 DECISIONS:".to_string()];
        for (key, value) in decisions {
            lines.push(format!("  • {}: {}", key, truncate(&value, 60)));
        }

        Ok(Some(lines.join("\n")))
    }

    async fn render_memories(&self) -> anyhow::Result<Option<String>> {
        let memories: Vec<(String, String, String)> = sqlx::query_as(
            r#"
            SELECT key, value, fact_type
            FROM memory_facts
            WHERE fact_type IN ('preference', 'context')
              AND (project_id IS NULL OR project_id = $1)
              AND key NOT LIKE 'compaction-%'
            ORDER BY times_used DESC, updated_at DESC
            LIMIT 3
            "#,
        )
        .bind(self.project_id)
        .fetch_all(&self.db)
        .await?;

        if memories.is_empty() {
            return Ok(None);
        }

        let mut lines = vec!["💭 CONTEXT:".to_string()];
        for (key, value, _fact_type) in memories {
            lines.push(format!("  • {}: {}", key, truncate(&value, 60)));
        }

        Ok(Some(lines.join("\n")))
    }

    async fn render_git_activity(&self) -> anyhow::Result<Option<String>> {
        let commits: Vec<(String, String, String)> = sqlx::query_as(
            r#"
            SELECT commit_hash, message, author
            FROM commits
            WHERE project_id = $1
            ORDER BY committed_at DESC
            LIMIT 3
            "#,
        )
        .bind(self.project_id)
        .fetch_all(&self.db)
        .await?;

        if commits.is_empty() {
            return Ok(None);
        }

        let mut lines = vec!["📝 RECENT COMMITS:".to_string()];
        for (hash, message, _author) in commits {
            let short_hash = &hash[..7.min(hash.len())];
            let first_line = message.lines().next().unwrap_or(&message);
            lines.push(format!("  • {} {}", short_hash, truncate(first_line, 55)));
        }

        Ok(Some(lines.join("\n")))
    }

    async fn render_code_context(&self) -> anyhow::Result<Option<String>> {
        // Get recently touched files from symbols table
        let recent_files: Vec<(String, i64)> = sqlx::query_as(
            r#"
            SELECT file_path, COUNT(*) as symbol_count
            FROM code_symbols
            WHERE project_id = $1
            GROUP BY file_path
            ORDER BY MAX(analyzed_at) DESC
            LIMIT 5
            "#,
        )
        .bind(self.project_id)
        .fetch_all(&self.db)
        .await?;

        if recent_files.is_empty() {
            return Ok(None);
        }

        let mut lines = vec!["📂 CODE CONTEXT:".to_string()];
        for (path, count) in recent_files {
            // Extract just the filename for brevity
            let filename = path.rsplit('/').next().unwrap_or(&path);
            lines.push(format!("  • {} ({} symbols)", filename, count));
        }

        Ok(Some(lines.join("\n")))
    }

    async fn render_system_status(&self) -> anyhow::Result<Option<String>> {
        // Get index stats
        let symbol_count: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM code_symbols WHERE project_id = $1",
        )
        .bind(self.project_id)
        .fetch_one(&self.db)
        .await
        .unwrap_or((0,));

        let commit_count: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM commits WHERE project_id = $1",
        )
        .bind(self.project_id)
        .fetch_one(&self.db)
        .await
        .unwrap_or((0,));

        let memory_count: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM memory_facts WHERE project_id IS NULL OR project_id = $1",
        )
        .bind(self.project_id)
        .fetch_one(&self.db)
        .await
        .unwrap_or((0,));

        let mut lines = vec![
            "⚙️ SYSTEM:".to_string(),
            format!("  • {} symbols indexed", symbol_count.0),
            format!("  • {} commits tracked", commit_count.0),
            format!("  • {} memories stored", memory_count.0),
            format!("  • Carousel: {}/{} (every {} calls)",
                self.state.index + 1,
                ContextCategory::rotation().len(),
                ROTATION_INTERVAL
            ),
        ];

        // Show pin status if active
        if let Some((cat, remaining)) = self.is_pinned() {
            lines.push(format!("  • 📌 Pinned: {:?} ({}m left)", cat, remaining / 60));
        }

        Ok(Some(lines.join("\n")))
    }

    async fn render_recent_errors(&self) -> anyhow::Result<Option<String>> {
        // Get recent build errors
        let build_errors: Vec<(String, String, String)> = sqlx::query_as(
            r#"
            SELECT file_path, message, severity
            FROM build_errors
            WHERE project_id = $1
              AND resolved_at IS NULL
            ORDER BY created_at DESC
            LIMIT 3
            "#,
        )
        .bind(self.project_id)
        .fetch_all(&self.db)
        .await
        .unwrap_or_default();

        // Get rejected approaches (wrong assumptions)
        let rejected: Vec<(String, String)> = sqlx::query_as(
            r#"
            SELECT problem_context, rejection_reason
            FROM rejected_approaches
            WHERE project_id IS NULL OR project_id = $1
            ORDER BY created_at DESC
            LIMIT 2
            "#,
        )
        .bind(self.project_id)
        .fetch_all(&self.db)
        .await
        .unwrap_or_default();

        if build_errors.is_empty() && rejected.is_empty() {
            return Ok(None);
        }

        let mut lines = vec!["⚠️ RECENT ISSUES:".to_string()];

        for (file, msg, _sev) in build_errors {
            let filename = file.rsplit('/').next().unwrap_or(&file);
            lines.push(format!("  • {}: {}", filename, truncate(&msg, 45)));
        }

        for (context, reason) in rejected {
            lines.push(format!("  • ❌ {}: {}", truncate(&context, 20), truncate(&reason, 35)));
        }

        Ok(Some(lines.join("\n")))
    }

    async fn render_user_patterns(&self) -> anyhow::Result<Option<String>> {
        // Get most-used memories (frequent preferences)
        let frequent: Vec<(String, String, i64)> = sqlx::query_as(
            r#"
            SELECT key, value, times_used
            FROM memory_facts
            WHERE (project_id IS NULL OR project_id = $1)
              AND times_used > 2
              AND key NOT LIKE 'compaction-%'
            ORDER BY times_used DESC
            LIMIT 3
            "#,
        )
        .bind(self.project_id)
        .fetch_all(&self.db)
        .await
        .unwrap_or_default();

        // Get recent session topics (recurring themes)
        let topics: Vec<(String,)> = sqlx::query_as(
            r#"
            SELECT topics
            FROM sessions
            WHERE project_id = $1
              AND topics IS NOT NULL
              AND topics != ''
            ORDER BY ended_at DESC
            LIMIT 5
            "#,
        )
        .bind(self.project_id)
        .fetch_all(&self.db)
        .await
        .unwrap_or_default();

        if frequent.is_empty() && topics.is_empty() {
            return Ok(None);
        }

        let mut lines = vec!["🔄 PATTERNS:".to_string()];

        for (key, _value, uses) in frequent {
            lines.push(format!("  • {} (used {}x)", truncate(&key, 40), uses));
        }

        // Extract unique topic keywords from recent sessions
        let mut topic_counts: HashMap<String, usize> = HashMap::new();
        for (topic_str,) in topics {
            for topic in topic_str.split(',').map(|s| s.trim().to_lowercase()) {
                if !topic.is_empty() {
                    *topic_counts.entry(topic).or_insert(0) += 1;
                }
            }
        }

        let mut sorted_topics: Vec<_> = topic_counts.into_iter().filter(|(_, c)| *c > 1).collect();
        sorted_topics.sort_by(|a, b| b.1.cmp(&a.1));

        if !sorted_topics.is_empty() {
            let top_topics: Vec<String> = sorted_topics.iter().take(3).map(|(t, _)| t.clone()).collect();
            lines.push(format!("  • Recurring: {}", top_topics.join(", ")));
        }

        Ok(Some(lines.join("\n")))
    }
}

/// Truncate a string to max length, adding "..." if truncated
fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max.saturating_sub(3)])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_category_rotation() {
        let rotation = ContextCategory::rotation();
        assert_eq!(rotation.len(), 8);
        assert_eq!(rotation[0], ContextCategory::Goals);
        assert_eq!(rotation[6], ContextCategory::RecentErrors);
        assert_eq!(rotation[7], ContextCategory::UserPatterns);
    }

    #[test]
    fn test_truncate() {
        assert_eq!(truncate("hello", 10), "hello");
        assert_eq!(truncate("hello world", 8), "hello...");
    }

    #[test]
    fn test_carousel_state_default() {
        let state = CarouselState::default();
        assert_eq!(state.index, 0);
        assert_eq!(state.call_count, 0);
    }
}
