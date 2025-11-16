// src/relationship/storage.rs

use anyhow::{Result, anyhow};
use sqlx::SqlitePool;
use std::sync::Arc;
use tracing::{debug, info};
use uuid::Uuid;

use crate::relationship::{LearnedPattern, MemoryFact, UserProfile};

/// Storage layer for relationship data (profiles, patterns, facts)
#[derive(Clone)]
pub struct RelationshipStorage {
    pool: Arc<SqlitePool>,
}

impl RelationshipStorage {
    pub fn new(pool: Arc<SqlitePool>) -> Self {
        Self { pool }
    }

    // ========================================================================
    // USER PROFILE OPERATIONS
    // ========================================================================

    /// Get or create user profile
    pub async fn get_or_create_profile(&self, user_id: &str) -> Result<UserProfile> {
        // Try to get existing profile
        if let Some(profile) = self.get_profile(user_id).await? {
            return Ok(profile);
        }

        // Create new profile
        let now = chrono::Utc::now().timestamp();
        let profile = UserProfile {
            id: 0, // Will be set by DB
            user_id: user_id.to_string(),
            preferred_languages: None,
            coding_style: None,
            code_verbosity: None,
            testing_philosophy: None,
            architecture_preferences: None,
            explanation_depth: None,
            conversation_style: None,
            profanity_comfort: None,
            tech_stack: None,
            learning_goals: None,
            relationship_started: now,
            last_active: Some(now),
            total_sessions: 0,
            created_at: now,
            updated_at: now,
        };

        self.create_profile(&profile).await
    }

    /// Get user profile
    pub async fn get_profile(&self, user_id: &str) -> Result<Option<UserProfile>> {
        let result =
            sqlx::query_as::<_, UserProfile>("SELECT * FROM user_profile WHERE user_id = ?")
                .bind(user_id)
                .fetch_optional(&*self.pool)
                .await?;

        Ok(result)
    }

    /// Create user profile
    async fn create_profile(&self, profile: &UserProfile) -> Result<UserProfile> {
        let result = sqlx::query(
            r#"
            INSERT INTO user_profile (
                user_id, preferred_languages, coding_style, code_verbosity,
                testing_philosophy, architecture_preferences, explanation_depth,
                conversation_style, profanity_comfort, tech_stack, learning_goals,
                relationship_started, last_active, total_sessions, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&profile.user_id)
        .bind(&profile.preferred_languages)
        .bind(&profile.coding_style)
        .bind(&profile.code_verbosity)
        .bind(&profile.testing_philosophy)
        .bind(&profile.architecture_preferences)
        .bind(&profile.explanation_depth)
        .bind(&profile.conversation_style)
        .bind(&profile.profanity_comfort)
        .bind(&profile.tech_stack)
        .bind(&profile.learning_goals)
        .bind(profile.relationship_started)
        .bind(profile.last_active)
        .bind(profile.total_sessions)
        .bind(profile.created_at)
        .bind(profile.updated_at)
        .execute(&*self.pool)
        .await?;

        let _id = result.last_insert_rowid();
        info!("Created user profile for user_id: {}", profile.user_id);

        // Return the created profile with its ID
        self.get_profile(&profile.user_id)
            .await?
            .ok_or_else(|| anyhow!("Failed to retrieve created profile"))
    }

    /// Update user profile
    pub async fn update_profile(&self, profile: &UserProfile) -> Result<()> {
        let now = chrono::Utc::now().timestamp();

        sqlx::query(
            r#"
            UPDATE user_profile SET
                preferred_languages = ?,
                coding_style = ?,
                code_verbosity = ?,
                testing_philosophy = ?,
                architecture_preferences = ?,
                explanation_depth = ?,
                conversation_style = ?,
                profanity_comfort = ?,
                tech_stack = ?,
                learning_goals = ?,
                last_active = ?,
                total_sessions = ?,
                updated_at = ?
            WHERE user_id = ?
            "#,
        )
        .bind(&profile.preferred_languages)
        .bind(&profile.coding_style)
        .bind(&profile.code_verbosity)
        .bind(&profile.testing_philosophy)
        .bind(&profile.architecture_preferences)
        .bind(&profile.explanation_depth)
        .bind(&profile.conversation_style)
        .bind(&profile.profanity_comfort)
        .bind(&profile.tech_stack)
        .bind(&profile.learning_goals)
        .bind(now)
        .bind(profile.total_sessions)
        .bind(now)
        .bind(&profile.user_id)
        .execute(&*self.pool)
        .await?;

        debug!("Updated user profile for user_id: {}", profile.user_id);
        Ok(())
    }

    // ========================================================================
    // LEARNED PATTERN OPERATIONS
    // ========================================================================

    /// Get all patterns for a user
    pub async fn get_patterns(&self, user_id: &str) -> Result<Vec<LearnedPattern>> {
        let patterns = sqlx::query_as::<_, LearnedPattern>(
            "SELECT * FROM learned_patterns WHERE user_id = ? AND deprecated = 0 ORDER BY confidence DESC"
        )
        .bind(user_id)
        .fetch_all(&*self.pool)
        .await?;

        Ok(patterns)
    }

    /// Get patterns by type
    pub async fn get_patterns_by_type(
        &self,
        user_id: &str,
        pattern_type: &str,
    ) -> Result<Vec<LearnedPattern>> {
        let patterns = sqlx::query_as::<_, LearnedPattern>(
            "SELECT * FROM learned_patterns WHERE user_id = ? AND pattern_type = ? AND deprecated = 0 ORDER BY confidence DESC"
        )
        .bind(user_id)
        .bind(pattern_type)
        .fetch_all(&*self.pool)
        .await?;

        Ok(patterns)
    }

    /// Get specific pattern
    pub async fn get_pattern(&self, pattern_id: &str) -> Result<Option<LearnedPattern>> {
        let pattern =
            sqlx::query_as::<_, LearnedPattern>("SELECT * FROM learned_patterns WHERE id = ?")
                .bind(pattern_id)
                .fetch_optional(&*self.pool)
                .await?;

        Ok(pattern)
    }

    /// Create or update a pattern
    pub async fn upsert_pattern(&self, pattern: &LearnedPattern) -> Result<String> {
        // Check if pattern exists by matching user_id + pattern_type + pattern_name
        let existing = sqlx::query_scalar::<_, String>(
            "SELECT id FROM learned_patterns WHERE user_id = ? AND pattern_type = ? AND pattern_name = ?"
        )
        .bind(&pattern.user_id)
        .bind(&pattern.pattern_type)
        .bind(&pattern.pattern_name)
        .fetch_optional(&*self.pool)
        .await?;

        if let Some(existing_id) = existing {
            // Update existing pattern
            self.update_pattern_with_id(&existing_id, pattern).await?;
            Ok(existing_id)
        } else {
            // Create new pattern
            self.create_pattern(pattern).await
        }
    }

    /// Create new pattern
    async fn create_pattern(&self, pattern: &LearnedPattern) -> Result<String> {
        let id = Uuid::new_v4().to_string();

        sqlx::query(
            r#"
            INSERT INTO learned_patterns (
                id, user_id, pattern_type, pattern_name, pattern_description,
                examples, confidence, times_observed, times_applied,
                applies_when, deprecated, first_observed, last_observed, last_applied
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&id)
        .bind(&pattern.user_id)
        .bind(&pattern.pattern_type)
        .bind(&pattern.pattern_name)
        .bind(&pattern.pattern_description)
        .bind(&pattern.examples)
        .bind(pattern.confidence)
        .bind(&pattern.times_observed)
        .bind(&pattern.times_applied)
        .bind(&pattern.applies_when)
        .bind(pattern.deprecated)
        .bind(pattern.first_observed)
        .bind(pattern.last_observed)
        .bind(pattern.last_applied)
        .execute(&*self.pool)
        .await?;

        info!(
            "Created pattern '{}' for user_id: {} (confidence: {:.2})",
            pattern.pattern_name, pattern.user_id, pattern.confidence
        );

        Ok(id)
    }

    /// Update existing pattern
    async fn update_pattern_with_id(&self, id: &str, pattern: &LearnedPattern) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE learned_patterns SET
                pattern_description = ?,
                examples = ?,
                confidence = ?,
                times_observed = ?,
                times_applied = ?,
                applies_when = ?,
                deprecated = ?,
                last_observed = ?,
                last_applied = ?
            WHERE id = ?
            "#,
        )
        .bind(&pattern.pattern_description)
        .bind(&pattern.examples)
        .bind(pattern.confidence)
        .bind(pattern.times_observed)
        .bind(&pattern.times_applied)
        .bind(&pattern.applies_when)
        .bind(pattern.deprecated)
        .bind(chrono::Utc::now().timestamp())
        .bind(pattern.last_applied)
        .bind(id)
        .execute(&*self.pool)
        .await?;

        debug!(
            "Updated pattern: {} (confidence: {:.2})",
            id, pattern.confidence
        );
        Ok(())
    }

    /// Increment times_observed for a pattern
    pub async fn increment_pattern_observed(&self, pattern_id: &str) -> Result<()> {
        sqlx::query(
            "UPDATE learned_patterns SET times_observed = times_observed + 1, last_observed = ? WHERE id = ?"
        )
        .bind(chrono::Utc::now().timestamp())
        .bind(pattern_id)
        .execute(&*self.pool)
        .await?;

        Ok(())
    }

    /// Increment times_applied for a pattern
    pub async fn increment_pattern_applied(&self, pattern_id: &str) -> Result<()> {
        sqlx::query(
            "UPDATE learned_patterns SET times_applied = times_applied + 1, last_applied = ? WHERE id = ?"
        )
        .bind(chrono::Utc::now().timestamp())
        .bind(pattern_id)
        .execute(&*self.pool)
        .await?;

        Ok(())
    }

    /// Deprecate a pattern
    pub async fn deprecate_pattern(&self, pattern_id: &str) -> Result<()> {
        sqlx::query("UPDATE learned_patterns SET deprecated = 1 WHERE id = ?")
            .bind(pattern_id)
            .execute(&*self.pool)
            .await?;

        info!("Deprecated pattern: {}", pattern_id);
        Ok(())
    }

    // ========================================================================
    // MEMORY FACT OPERATIONS
    // ========================================================================

    /// Get all facts for a user
    pub async fn get_facts(&self, user_id: &str) -> Result<Vec<MemoryFact>> {
        let facts = sqlx::query_as::<_, MemoryFact>(
            "SELECT * FROM memory_facts WHERE user_id = ? ORDER BY confidence DESC",
        )
        .bind(user_id)
        .fetch_all(&*self.pool)
        .await?;

        Ok(facts)
    }

    /// Get facts by category
    pub async fn get_facts_by_category(
        &self,
        user_id: &str,
        category: &str,
    ) -> Result<Vec<MemoryFact>> {
        let facts = sqlx::query_as::<_, MemoryFact>(
            "SELECT * FROM memory_facts WHERE user_id = ? AND fact_category = ? ORDER BY confidence DESC"
        )
        .bind(user_id)
        .bind(category)
        .fetch_all(&*self.pool)
        .await?;

        Ok(facts)
    }

    /// Get a specific fact
    pub async fn get_fact(&self, user_id: &str, fact_key: &str) -> Result<Option<MemoryFact>> {
        let fact = sqlx::query_as::<_, MemoryFact>(
            "SELECT * FROM memory_facts WHERE user_id = ? AND fact_key = ?",
        )
        .bind(user_id)
        .bind(fact_key)
        .fetch_optional(&*self.pool)
        .await?;

        Ok(fact)
    }

    /// Create or update a fact
    pub async fn upsert_fact(&self, fact: &MemoryFact) -> Result<String> {
        // Check if fact exists
        let existing = sqlx::query_scalar::<_, String>(
            "SELECT id FROM memory_facts WHERE user_id = ? AND fact_key = ?",
        )
        .bind(&fact.user_id)
        .bind(&fact.fact_key)
        .fetch_optional(&*self.pool)
        .await?;

        if let Some(existing_id) = existing {
            // Update existing fact
            self.update_fact_with_id(&existing_id, fact).await?;
            Ok(existing_id)
        } else {
            // Create new fact
            self.create_fact(fact).await
        }
    }

    /// Create new fact
    async fn create_fact(&self, fact: &MemoryFact) -> Result<String> {
        let id = Uuid::new_v4().to_string();

        sqlx::query(
            r#"
            INSERT INTO memory_facts (
                id, user_id, fact_key, fact_value, fact_category,
                confidence, source, learned_at, last_confirmed, times_referenced
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&id)
        .bind(&fact.user_id)
        .bind(&fact.fact_key)
        .bind(&fact.fact_value)
        .bind(&fact.fact_category)
        .bind(fact.confidence)
        .bind(&fact.source)
        .bind(fact.learned_at)
        .bind(fact.last_confirmed)
        .bind(fact.times_referenced)
        .execute(&*self.pool)
        .await?;

        info!(
            "Created fact '{}' = '{}' for user_id: {}",
            fact.fact_key, fact.fact_value, fact.user_id
        );

        Ok(id)
    }

    /// Update existing fact
    async fn update_fact_with_id(&self, id: &str, fact: &MemoryFact) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE memory_facts SET
                fact_value = ?,
                fact_category = ?,
                confidence = ?,
                source = ?,
                last_confirmed = ?
            WHERE id = ?
            "#,
        )
        .bind(&fact.fact_value)
        .bind(&fact.fact_category)
        .bind(fact.confidence)
        .bind(&fact.source)
        .bind(chrono::Utc::now().timestamp())
        .bind(id)
        .execute(&*self.pool)
        .await?;

        debug!("Updated fact: {} = {}", fact.fact_key, fact.fact_value);
        Ok(())
    }

    /// Increment times_referenced for a fact
    pub async fn increment_fact_referenced(&self, fact_id: &str) -> Result<()> {
        sqlx::query("UPDATE memory_facts SET times_referenced = times_referenced + 1 WHERE id = ?")
            .bind(fact_id)
            .execute(&*self.pool)
            .await?;

        Ok(())
    }
}
