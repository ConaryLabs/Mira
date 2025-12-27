// src/context/carousel.rs
//! Carousel-based context rotation v2
//!
//! Rotates through context categories across tool calls with intelligent
//! mode switching based on triggers and semantic analysis.
//!
//! Features:
//! - State machine: Cruising / Focus / Panic modes
//! - Semantic interrupts: Query matching forces relevant categories
//! - Trigger overrides: File edits, errors, planning language
//! - Anchor slot: Critical items carry across rotations
//! - Starvation prevention: Force-flash categories after N turns unseen
//! - Observability: Every decision is logged with rationale

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::collections::HashMap;
use tracing::{debug, info, warn};

/// How many tool calls before rotating to next category (in Cruising mode)
pub const ROTATION_INTERVAL: u64 = 4;

/// Maximum turns a category can go unseen before forced injection
pub const MAX_STARVATION_TURNS: u64 = 12;

/// Maximum tokens for anchor slot (critical carryover)
pub const ANCHOR_MAX_TOKENS: usize = 200;

/// Maximum items in anchor slot
pub const ANCHOR_MAX_ITEMS: usize = 2;

// ============================================================================
// Core Types
// ============================================================================

/// Categories of context that rotate through the carousel
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextCategory {
    Goals,
    Decisions,
    Memories,
    GitActivity,
    CodeContext,
    SystemStatus,
    RecentErrors,
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

    /// Short string identifier for database storage
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Goals => "goals",
            Self::Decisions => "decisions",
            Self::Memories => "memories",
            Self::GitActivity => "git_activity",
            Self::CodeContext => "code_context",
            Self::SystemStatus => "system_status",
            Self::RecentErrors => "recent_errors",
            Self::UserPatterns => "user_patterns",
        }
    }

    /// Keywords that trigger this category via semantic interrupt
    pub fn trigger_keywords(&self) -> &'static [&'static str] {
        match self {
            Self::Goals => &["goal", "milestone", "progress", "objective", "target", "plan"],
            Self::Decisions => &["decision", "decided", "chose", "rationale", "why did we"],
            Self::Memories => &["remember", "preference", "prefer", "usually", "always"],
            Self::GitActivity => &["commit", "git", "push", "branch", "merge", "diff", "change"],
            Self::CodeContext => &["file", "function", "class", "symbol", "code", "implement"],
            Self::SystemStatus => &["index", "status", "system", "carousel", "mira"],
            Self::RecentErrors => &["error", "fail", "crash", "bug", "fix", "broken", "issue", "wrong"],
            Self::UserPatterns => &["pattern", "habit", "recurring", "often", "frequent"],
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

/// Carousel operating mode (state machine)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CarouselMode {
    /// Normal rotation based on ticks/turns
    Cruising,
    /// Focused on a specific category (user pinned or trigger-activated)
    Focus(ContextCategory),
    /// Emergency mode - errors/code locked until resolved
    Panic,
}

impl Default for CarouselMode {
    fn default() -> Self {
        Self::Cruising
    }
}

/// Triggers that can force mode/category changes
#[derive(Debug, Clone, PartialEq)]
pub enum CarouselTrigger {
    /// File was edited/opened
    FileEdit(String),
    /// Build/test failure detected
    BuildFailure(String),
    /// Panic/crash detected
    CrashDetected(String),
    /// Planning language in query ("let's implement", "TODO", etc.)
    PlanningLanguage,
    /// User query matched category keywords
    SemanticMatch(ContextCategory, f32),
    /// User explicitly requested focus
    UserFocus(ContextCategory),
    /// Error was resolved, exit panic mode
    ErrorResolved,
}

/// An item in the anchor slot (carries across rotations)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnchorItem {
    /// The content to carry over
    pub content: String,
    /// Why this was anchored
    pub reason: String,
    /// Category it came from
    pub source_category: ContextCategory,
    /// When it was anchored
    pub anchored_at: i64,
    /// Turns remaining before it expires
    pub ttl_turns: u32,
}

/// Decision log entry for observability
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CarouselDecision {
    /// Timestamp of decision
    pub timestamp: i64,
    /// The mode we're in
    pub mode: CarouselMode,
    /// Category chosen
    pub category: ContextCategory,
    /// Why this category was chosen
    pub reason: String,
    /// Triggers that fired (if any)
    pub triggers: Vec<String>,
    /// Runner-up category (what would have been shown otherwise)
    pub runner_up: Option<ContextCategory>,
    /// Was this a starvation prevention injection?
    pub starvation_rescue: bool,
}

// ============================================================================
// Carousel State
// ============================================================================

/// Persistent carousel state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CarouselState {
    /// Current position in the rotation (0-based index)
    pub index: usize,
    /// Current operating mode
    #[serde(default)]
    pub mode: CarouselMode,
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
    /// Last shown turn for each category (for starvation prevention)
    #[serde(default)]
    pub category_last_shown: HashMap<String, u64>,
    /// Anchor slot - critical items that carry across rotations
    #[serde(default)]
    pub anchor_items: Vec<AnchorItem>,
    /// Last N decision logs for observability
    #[serde(default)]
    pub decision_log: Vec<CarouselDecision>,
    /// Whether panic mode is active (separate from mode for persistence)
    #[serde(default)]
    pub panic_active: bool,
    /// What triggered panic mode (for exit detection)
    #[serde(default)]
    pub panic_trigger: Option<String>,
}

impl Default for CarouselState {
    fn default() -> Self {
        Self {
            index: 0,
            mode: CarouselMode::Cruising,
            call_count: 0,
            calls_since_advance: 0,
            last_advanced: 0,
            pinned_category: None,
            pin_expires_at: None,
            category_last_shown: HashMap::new(),
            anchor_items: Vec::new(),
            decision_log: Vec::new(),
            panic_active: false,
            panic_trigger: None,
        }
    }
}

// ============================================================================
// Context Carousel
// ============================================================================

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

        // Restore mode from panic_active flag
        if state.panic_active {
            state.mode = CarouselMode::Panic;
        }

        // Check if pin has expired
        if let Some(expires) = state.pin_expires_at {
            if Utc::now().timestamp() > expires {
                state.pinned_category = None;
                state.pin_expires_at = None;
                if matches!(state.mode, CarouselMode::Focus(_)) {
                    state.mode = CarouselMode::Cruising;
                }
            }
        }

        // Expire old anchor items
        state.anchor_items.retain(|item| item.ttl_turns > 0);

        // Trim decision log to last 20 entries
        if state.decision_log.len() > 20 {
            state.decision_log = state.decision_log.split_off(state.decision_log.len() - 20);
        }

        Ok(Self { state, db, project_id })
    }

    // ========================================================================
    // Trigger Processing
    // ========================================================================

    /// Process triggers and update mode/category accordingly
    pub fn process_triggers(&mut self, triggers: &[CarouselTrigger]) -> Option<CarouselDecision> {
        if triggers.is_empty() {
            return None;
        }

        let mut decision_triggers = Vec::new();

        for trigger in triggers {
            match trigger {
                CarouselTrigger::CrashDetected(msg) | CarouselTrigger::BuildFailure(msg) => {
                    info!("[CAROUSEL] Panic mode activated: {}", msg);
                    self.state.mode = CarouselMode::Panic;
                    self.state.panic_active = true;
                    self.state.panic_trigger = Some(msg.clone());
                    decision_triggers.push(format!("panic:{}", truncate(msg, 30)));
                }
                CarouselTrigger::FileEdit(path) => {
                    if !self.state.panic_active {
                        info!("[CAROUSEL] File edit trigger: {}", path);
                        self.state.mode = CarouselMode::Focus(ContextCategory::CodeContext);
                        decision_triggers.push(format!("file_edit:{}", truncate(path, 30)));
                    }
                }
                CarouselTrigger::PlanningLanguage => {
                    if !self.state.panic_active {
                        info!("[CAROUSEL] Planning language detected");
                        self.state.mode = CarouselMode::Focus(ContextCategory::Goals);
                        decision_triggers.push("planning_language".to_string());
                    }
                }
                CarouselTrigger::SemanticMatch(cat, match_count) => {
                    // Already filtered to 2+ matches in detect_semantic_interrupt
                    if !self.state.panic_active {
                        info!("[CAROUSEL] Semantic match: {:?} ({} keywords)", cat, *match_count as usize);
                        self.state.mode = CarouselMode::Focus(*cat);
                        decision_triggers.push(format!("semantic:{:?}:{}", cat, *match_count as usize));
                    }
                }
                CarouselTrigger::UserFocus(cat) => {
                    info!("[CAROUSEL] User focus requested: {:?}", cat);
                    self.state.mode = CarouselMode::Focus(*cat);
                    self.state.panic_active = false;
                    decision_triggers.push(format!("user_focus:{:?}", cat));
                }
                CarouselTrigger::ErrorResolved => {
                    if self.state.panic_active {
                        info!("[CAROUSEL] Exiting panic mode - error resolved");
                        self.state.mode = CarouselMode::Cruising;
                        self.state.panic_active = false;
                        self.state.panic_trigger = None;
                        decision_triggers.push("error_resolved".to_string());
                    }
                }
            }
        }

        if !decision_triggers.is_empty() {
            Some(CarouselDecision {
                timestamp: Utc::now().timestamp(),
                mode: self.state.mode,
                category: self.current(),
                reason: format!("Triggers fired: {}", decision_triggers.join(", ")),
                triggers: decision_triggers,
                runner_up: None,
                starvation_rescue: false,
            })
        } else {
            None
        }
    }

    /// Detect semantic interrupts from user query
    ///
    /// Triggers when 2+ keywords match (absolute count, not proportional).
    /// This prevents single-word false positives while still being responsive
    /// to relevant queries like "fix the error" or "check my goals".
    pub fn detect_semantic_interrupt(&self, query: &str) -> Option<CarouselTrigger> {
        let query_lower = query.to_lowercase();
        let mut best_match: Option<(ContextCategory, usize)> = None;

        for cat in ContextCategory::rotation() {
            let keywords = cat.trigger_keywords();
            let match_count = keywords.iter()
                .filter(|kw| query_lower.contains(*kw))
                .count();

            // Require 2+ keyword matches for semantic interrupt
            if match_count >= 2 {
                if best_match.map(|(_, c)| match_count > c).unwrap_or(true) {
                    best_match = Some((*cat, match_count));
                }
            }
        }

        // Also check for planning language
        let planning_keywords = ["let's implement", "let's add", "let's build", "todo", "we need to", "we should"];
        if planning_keywords.iter().any(|kw| query_lower.contains(kw)) {
            // Planning language overrides other matches unless error-related
            if !best_match.map(|(c, _)| c == ContextCategory::RecentErrors).unwrap_or(false) {
                return Some(CarouselTrigger::PlanningLanguage);
            }
        }

        best_match.map(|(cat, count)| CarouselTrigger::SemanticMatch(cat, count as f32))
    }

    // ========================================================================
    // Starvation Prevention
    // ========================================================================

    /// Check if any category is starving and needs forced injection
    fn check_starvation(&self) -> Option<ContextCategory> {
        let current_turn = self.state.call_count;

        for cat in ContextCategory::rotation() {
            let cat_key = format!("{:?}", cat).to_lowercase();
            let last_shown = self.state.category_last_shown.get(&cat_key).copied().unwrap_or(0);
            let turns_unseen = current_turn.saturating_sub(last_shown);

            if turns_unseen >= MAX_STARVATION_TURNS {
                debug!("[CAROUSEL] Starvation detected: {:?} unseen for {} turns", cat, turns_unseen);
                return Some(*cat);
            }
        }
        None
    }

    // ========================================================================
    // Anchor Slot
    // ========================================================================

    /// Add an item to the anchor slot
    pub fn anchor_item(&mut self, content: String, reason: String, source: ContextCategory, ttl: u32) {
        // Check token budget (rough estimate: 4 chars per token)
        let current_tokens: usize = self.state.anchor_items.iter()
            .map(|i| i.content.len() / 4)
            .sum();
        let new_tokens = content.len() / 4;

        if current_tokens + new_tokens > ANCHOR_MAX_TOKENS {
            // Remove oldest items to make room
            while self.state.anchor_items.len() > 0
                && current_tokens + new_tokens > ANCHOR_MAX_TOKENS
            {
                self.state.anchor_items.remove(0);
            }
        }

        // Enforce max items
        while self.state.anchor_items.len() >= ANCHOR_MAX_ITEMS {
            self.state.anchor_items.remove(0);
        }

        info!("[CAROUSEL] Anchoring item from {:?}: {}", source, truncate(&reason, 40));
        self.state.anchor_items.push(AnchorItem {
            content,
            reason,
            source_category: source,
            anchored_at: Utc::now().timestamp(),
            ttl_turns: ttl,
        });
    }

    /// Decrement anchor TTLs and remove expired items
    fn tick_anchors(&mut self) {
        for item in &mut self.state.anchor_items {
            item.ttl_turns = item.ttl_turns.saturating_sub(1);
        }
        self.state.anchor_items.retain(|item| item.ttl_turns > 0);
    }

    /// Render anchor slot content
    pub fn render_anchor(&self) -> Option<String> {
        if self.state.anchor_items.is_empty() {
            return None;
        }

        let mut lines = vec!["📌 ANCHORED:".to_string()];
        for item in &self.state.anchor_items {
            lines.push(format!("  • {}", truncate(&item.content, 70)));
        }
        Some(lines.join("\n"))
    }

    // ========================================================================
    // Core Rotation Logic
    // ========================================================================

    /// Get the current category based on mode and state
    pub fn current(&self) -> ContextCategory {
        match self.state.mode {
            CarouselMode::Panic => ContextCategory::RecentErrors,
            CarouselMode::Focus(cat) => cat,
            CarouselMode::Cruising => {
                // Check pinned first
                if let Some(pinned) = self.state.pinned_category {
                    if let Some(expires) = self.state.pin_expires_at {
                        if Utc::now().timestamp() <= expires {
                            return pinned;
                        }
                    }
                }
                let rotation = ContextCategory::rotation();
                rotation[self.state.index % rotation.len()]
            }
        }
    }

    /// Get categories to render for this turn (handles panic mode showing multiple)
    pub fn categories_to_render(&self) -> Vec<ContextCategory> {
        match self.state.mode {
            CarouselMode::Panic => {
                // In panic mode, show both errors and code
                vec![ContextCategory::RecentErrors, ContextCategory::CodeContext]
            }
            _ => vec![self.current()],
        }
    }

    /// Tick the carousel with optional triggers and query
    /// Returns the decision made and categories to render
    pub async fn tick_with_context(
        &mut self,
        triggers: &[CarouselTrigger],
        query: Option<&str>,
    ) -> anyhow::Result<(CarouselDecision, Vec<ContextCategory>)> {
        self.state.call_count += 1;
        self.state.calls_since_advance += 1;

        // Process explicit triggers first
        let trigger_decision = self.process_triggers(triggers);

        // Check semantic interrupt from query
        let semantic_trigger = query.and_then(|q| self.detect_semantic_interrupt(q));
        if let Some(trigger) = semantic_trigger {
            self.process_triggers(&[trigger]);
        }

        // Check starvation
        let starving_cat = self.check_starvation();
        let starvation_rescue = starving_cat.is_some();

        // Determine final category
        let (category, reason, runner_up) = if starvation_rescue {
            let starving = starving_cat.unwrap();
            (starving, format!("Starvation rescue: {:?} unseen too long", starving), Some(self.current()))
        } else {
            match self.state.mode {
                CarouselMode::Panic => {
                    (ContextCategory::RecentErrors, "Panic mode active".to_string(), None)
                }
                CarouselMode::Focus(cat) => {
                    (cat, format!("Focus mode on {:?}", cat), None)
                }
                CarouselMode::Cruising => {
                    // Check if we should advance
                    if self.state.pinned_category.is_none()
                        && self.state.calls_since_advance >= ROTATION_INTERVAL
                    {
                        let rotation = ContextCategory::rotation();
                        let old_idx = self.state.index;
                        self.state.index = (self.state.index + 1) % rotation.len();
                        self.state.calls_since_advance = 0;
                        self.state.last_advanced = Utc::now().timestamp();

                        let runner = rotation[old_idx % rotation.len()];
                        (self.current(), "Rotation advanced".to_string(), Some(runner))
                    } else if let Some(pinned) = self.state.pinned_category {
                        (pinned, format!("Pinned: {:?}", pinned), None)
                    } else {
                        (self.current(), "Cruising".to_string(), None)
                    }
                }
            }
        };

        // Record when this category was shown
        let cat_key = format!("{:?}", category).to_lowercase();
        self.state.category_last_shown.insert(cat_key, self.state.call_count);

        // Tick anchors
        self.tick_anchors();

        // Build decision log
        let decision = trigger_decision.unwrap_or_else(|| CarouselDecision {
            timestamp: Utc::now().timestamp(),
            mode: self.state.mode,
            category,
            reason: reason.clone(),
            triggers: Vec::new(),
            runner_up,
            starvation_rescue,
        });

        // Log the decision
        info!(
            "[CAROUSEL] {} → {:?} | Mode: {:?} | Reason: {}{}",
            self.state.call_count,
            category,
            self.state.mode,
            &decision.reason,
            if starvation_rescue { " [STARVATION RESCUE]" } else { "" }
        );

        // Store in decision log
        self.state.decision_log.push(decision.clone());
        if self.state.decision_log.len() > 20 {
            self.state.decision_log.remove(0);
        }

        // Save state
        self.save().await?;

        Ok((decision, self.categories_to_render()))
    }

    /// Simple tick without context (backwards compatible)
    pub async fn tick_and_maybe_advance(&mut self) -> anyhow::Result<ContextCategory> {
        let (decision, _) = self.tick_with_context(&[], None).await?;
        Ok(decision.category)
    }

    /// Force advance to next category (ignores throttle)
    pub async fn force_advance(&mut self) -> anyhow::Result<ContextCategory> {
        // Exit focus/panic modes
        self.state.mode = CarouselMode::Cruising;
        self.state.panic_active = false;

        let rotation = ContextCategory::rotation();
        self.state.index = (self.state.index + 1) % rotation.len();
        self.state.calls_since_advance = 0;
        self.state.last_advanced = Utc::now().timestamp();
        self.state.call_count += 1;

        info!("[CAROUSEL] Force advanced to {:?}", self.current());
        self.save().await?;
        Ok(self.current())
    }

    /// Pin a category for the specified duration (in minutes)
    pub async fn pin(&mut self, category: ContextCategory, duration_minutes: i64) -> anyhow::Result<()> {
        let expires = Utc::now().timestamp() + (duration_minutes * 60);
        self.state.pinned_category = Some(category);
        self.state.pin_expires_at = Some(expires);
        self.state.mode = CarouselMode::Focus(category);
        info!("[CAROUSEL] Pinned {:?} for {} minutes", category, duration_minutes);
        self.save().await
    }

    /// Unpin the current category
    pub async fn unpin(&mut self) -> anyhow::Result<()> {
        self.state.pinned_category = None;
        self.state.pin_expires_at = None;
        self.state.mode = CarouselMode::Cruising;
        info!("[CAROUSEL] Unpinned, returning to Cruising mode");
        self.save().await
    }

    /// Check if a category is currently pinned
    pub fn is_pinned(&self) -> Option<(ContextCategory, i64)> {
        if let (Some(cat), Some(expires)) = (self.state.pinned_category, self.state.pin_expires_at) {
            let now = Utc::now().timestamp();
            if now <= expires {
                return Some((cat, expires - now));
            }
        }
        None
    }

    /// Enter panic mode manually
    pub async fn enter_panic(&mut self, reason: &str) -> anyhow::Result<()> {
        self.state.mode = CarouselMode::Panic;
        self.state.panic_active = true;
        self.state.panic_trigger = Some(reason.to_string());
        warn!("[CAROUSEL] Entering panic mode: {}", reason);
        self.save().await
    }

    /// Exit panic mode
    pub async fn exit_panic(&mut self) -> anyhow::Result<()> {
        self.state.mode = CarouselMode::Cruising;
        self.state.panic_active = false;
        self.state.panic_trigger = None;
        info!("[CAROUSEL] Exiting panic mode");
        self.save().await
    }

    /// Get current mode
    pub fn mode(&self) -> CarouselMode {
        self.state.mode
    }

    /// Get decision log for observability
    pub fn decision_log(&self) -> &[CarouselDecision] {
        &self.state.decision_log
    }

    /// Increment call count without advancing (for skipped injections)
    pub async fn tick(&mut self) -> anyhow::Result<()> {
        self.state.call_count += 1;
        self.save().await
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

    // ========================================================================
    // Rendering
    // ========================================================================

    /// Render context for the current category
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
            let filename = path.rsplit('/').next().unwrap_or(&path);
            lines.push(format!("  • {} ({} symbols)", filename, count));
        }

        Ok(Some(lines.join("\n")))
    }

    async fn render_system_status(&self) -> anyhow::Result<Option<String>> {
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

        let mode_str = match self.state.mode {
            CarouselMode::Cruising => "Cruising",
            CarouselMode::Focus(cat) => return Ok(Some(format!(
                "⚙️ SYSTEM:\n  • Mode: Focus({:?})\n  • {} symbols | {} commits | {} memories",
                cat, symbol_count.0, commit_count.0, memory_count.0
            ))),
            CarouselMode::Panic => "🚨 PANIC",
        };

        let mut lines = vec![
            "⚙️ SYSTEM:".to_string(),
            format!("  • Mode: {}", mode_str),
            format!("  • {} symbols | {} commits | {} memories", symbol_count.0, commit_count.0, memory_count.0),
            format!("  • Carousel: {}/{} (every {} calls)",
                self.state.index + 1,
                ContextCategory::rotation().len(),
                ROTATION_INTERVAL
            ),
        ];

        if let Some((cat, remaining)) = self.is_pinned() {
            lines.push(format!("  • 📌 Pinned: {:?} ({}m left)", cat, remaining / 60));
        }

        if !self.state.anchor_items.is_empty() {
            lines.push(format!("  • ⚓ {} anchored items", self.state.anchor_items.len()));
        }

        Ok(Some(lines.join("\n")))
    }

    async fn render_recent_errors(&self) -> anyhow::Result<Option<String>> {
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

    /// Render full context: anchor + critical + current categories
    pub async fn render_full_context(&self) -> anyhow::Result<String> {
        let mut parts = Vec::new();

        // Anchor slot first (most critical carryover)
        if let Some(anchor) = self.render_anchor() {
            parts.push(anchor);
        }

        // Critical context (corrections, blocked goals)
        if let Some(critical) = self.render_critical().await? {
            parts.push(critical);
        }

        // Current category/categories
        for cat in self.categories_to_render() {
            if let Some(content) = self.render_category(cat).await? {
                parts.push(content);
            }
        }

        Ok(parts.join("\n\n"))
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
        assert_eq!(state.mode, CarouselMode::Cruising);
    }

    #[test]
    fn test_semantic_keywords() {
        assert!(ContextCategory::Goals.trigger_keywords().contains(&"goal"));
        assert!(ContextCategory::RecentErrors.trigger_keywords().contains(&"error"));
    }

    #[test]
    fn test_mode_default() {
        assert_eq!(CarouselMode::default(), CarouselMode::Cruising);
    }
}
