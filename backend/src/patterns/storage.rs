// src/patterns/storage.rs
// Pattern storage and retrieval

use anyhow::{Context, Result};
use chrono::{DateTime, TimeZone, Utc};
use sqlx::{FromRow, SqlitePool};
use std::sync::Arc;
use tracing::{debug, warn};

use super::types::*;

/// Parse a Unix timestamp into a DateTime<Utc>, falling back to epoch on invalid values.
/// Logs a warning if the timestamp is invalid.
fn parse_timestamp(ts: i64) -> DateTime<Utc> {
    Utc.timestamp_opt(ts, 0)
        .single()
        .unwrap_or_else(|| {
            warn!("Invalid timestamp value: {}, using epoch", ts);
            DateTime::UNIX_EPOCH
        })
}

/// Parse an optional Unix timestamp into an Option<DateTime<Utc>>.
fn parse_timestamp_opt(ts: Option<i64>) -> Option<DateTime<Utc>> {
    ts.map(parse_timestamp)
}

/// Row struct for query_as
#[derive(FromRow)]
struct PatternRow {
    id: Option<String>,
    project_id: Option<String>,
    name: String,
    description: String,
    trigger_type: String,
    reasoning_chain: String,
    solution_template: Option<String>,
    applicable_contexts: Option<String>,
    success_rate: Option<f64>,
    use_count: Option<i64>,
    success_count: Option<i64>,
    cost_savings_usd: Option<f64>,
    created_at: i64,
    updated_at: i64,
    last_used: Option<i64>,
}

/// Pattern storage handles CRUD operations for reasoning patterns
pub struct PatternStorage {
    pool: Arc<SqlitePool>,
}

impl PatternStorage {
    pub fn new(pool: Arc<SqlitePool>) -> Self {
        Self { pool }
    }

    /// Store a new pattern
    pub async fn store_pattern(&self, pattern: &ReasoningPattern) -> Result<()> {
        let trigger_type = pattern.trigger_type.as_str();
        let contexts_json = serde_json::to_string(&pattern.applicable_contexts)?;
        let created_at = pattern.created_at.timestamp();
        let updated_at = pattern.updated_at.timestamp();
        let last_used = pattern.last_used.map(|t| t.timestamp());

        sqlx::query!(
            r#"
            INSERT INTO reasoning_patterns (
                id, project_id, name, description, trigger_type, reasoning_chain,
                solution_template, applicable_contexts, success_rate, use_count,
                success_count, cost_savings_usd, created_at, updated_at, last_used
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
            pattern.id,
            pattern.project_id,
            pattern.name,
            pattern.description,
            trigger_type,
            pattern.reasoning_chain,
            pattern.solution_template,
            contexts_json,
            pattern.success_rate,
            pattern.use_count,
            pattern.success_count,
            pattern.cost_savings_usd,
            created_at,
            updated_at,
            last_used,
        )
        .execute(self.pool.as_ref())
        .await
        .context("Failed to insert pattern")?;

        // Store steps
        for step in &pattern.steps {
            self.store_step(step).await?;
        }

        debug!("Stored pattern: {} ({})", pattern.name, pattern.id);
        Ok(())
    }

    /// Store a reasoning step
    pub async fn store_step(&self, step: &ReasoningStep) -> Result<i64> {
        let step_type = step.step_type.as_str();
        let created_at = step.created_at.timestamp();

        let id = sqlx::query!(
            r#"
            INSERT INTO reasoning_steps (
                pattern_id, step_number, step_type, description, rationale, created_at
            ) VALUES (?, ?, ?, ?, ?, ?)
            "#,
            step.pattern_id,
            step.step_number,
            step_type,
            step.description,
            step.rationale,
            created_at,
        )
        .execute(self.pool.as_ref())
        .await?
        .last_insert_rowid();

        Ok(id)
    }

    /// Get a pattern by ID
    pub async fn get_pattern(&self, id: &str) -> Result<Option<ReasoningPattern>> {
        let row = sqlx::query!(
            r#"
            SELECT id, project_id, name, description, trigger_type, reasoning_chain,
                   solution_template, applicable_contexts, success_rate, use_count,
                   success_count, cost_savings_usd, created_at, updated_at, last_used
            FROM reasoning_patterns WHERE id = ?
            "#,
            id
        )
        .fetch_optional(self.pool.as_ref())
        .await?;

        match row {
            Some(r) => {
                let contexts: ApplicableContext =
                    serde_json::from_str(&r.applicable_contexts.unwrap_or_default())
                        .unwrap_or_default();

                let pattern_id = r.id.unwrap_or_default();
                let mut pattern = ReasoningPattern {
                    id: pattern_id.clone(),
                    project_id: r.project_id,
                    name: r.name,
                    description: r.description,
                    trigger_type: TriggerType::from_str(&r.trigger_type),
                    reasoning_chain: r.reasoning_chain,
                    solution_template: r.solution_template,
                    applicable_contexts: contexts,
                    success_rate: r.success_rate.unwrap_or(1.0),
                    use_count: r.use_count.unwrap_or(0) as i32,
                    success_count: r.success_count.unwrap_or(0) as i32,
                    cost_savings_usd: r.cost_savings_usd.unwrap_or(0.0),
                    created_at: parse_timestamp(r.created_at),
                    updated_at: parse_timestamp(r.updated_at),
                    last_used: parse_timestamp_opt(r.last_used),
                    steps: Vec::new(),
                };

                // Load steps
                pattern.steps = self.get_steps(&pattern_id).await?;

                Ok(Some(pattern))
            }
            None => Ok(None),
        }
    }

    /// Get steps for a pattern
    pub async fn get_steps(&self, pattern_id: &str) -> Result<Vec<ReasoningStep>> {
        let rows = sqlx::query!(
            r#"
            SELECT id, pattern_id, step_number, step_type, description, rationale, created_at
            FROM reasoning_steps
            WHERE pattern_id = ?
            ORDER BY step_number
            "#,
            pattern_id
        )
        .fetch_all(self.pool.as_ref())
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| ReasoningStep {
                id: r.id,
                pattern_id: r.pattern_id,
                step_number: r.step_number as i32,
                step_type: StepType::from_str(&r.step_type),
                description: r.description,
                rationale: r.rationale,
                created_at: parse_timestamp(r.created_at),
            })
            .collect())
    }

    /// Update a pattern
    pub async fn update_pattern(&self, pattern: &ReasoningPattern) -> Result<()> {
        let trigger_type = pattern.trigger_type.as_str();
        let contexts_json = serde_json::to_string(&pattern.applicable_contexts)?;
        let updated_at = Utc::now().timestamp();
        let last_used = pattern.last_used.map(|t| t.timestamp());

        sqlx::query!(
            r#"
            UPDATE reasoning_patterns SET
                name = ?, description = ?, trigger_type = ?, reasoning_chain = ?,
                solution_template = ?, applicable_contexts = ?, success_rate = ?,
                use_count = ?, success_count = ?, cost_savings_usd = ?,
                updated_at = ?, last_used = ?
            WHERE id = ?
            "#,
            pattern.name,
            pattern.description,
            trigger_type,
            pattern.reasoning_chain,
            pattern.solution_template,
            contexts_json,
            pattern.success_rate,
            pattern.use_count,
            pattern.success_count,
            pattern.cost_savings_usd,
            updated_at,
            last_used,
            pattern.id,
        )
        .execute(self.pool.as_ref())
        .await?;

        debug!("Updated pattern: {}", pattern.id);
        Ok(())
    }

    /// Delete a pattern
    pub async fn delete_pattern(&self, id: &str) -> Result<bool> {
        let result = sqlx::query!("DELETE FROM reasoning_patterns WHERE id = ?", id)
            .execute(self.pool.as_ref())
            .await?;

        Ok(result.rows_affected() > 0)
    }

    /// List patterns for a project
    pub async fn list_patterns(
        &self,
        project_id: Option<&str>,
        limit: i32,
    ) -> Result<Vec<ReasoningPattern>> {
        // Use query_as with PatternRow to avoid type mismatch between branches
        let rows = if let Some(pid) = project_id {
            sqlx::query_as::<_, PatternRow>(
                r#"
                SELECT id, project_id, name, description, trigger_type, reasoning_chain,
                       solution_template, applicable_contexts, success_rate, use_count,
                       success_count, cost_savings_usd, created_at, updated_at, last_used
                FROM reasoning_patterns
                WHERE project_id = ? OR project_id IS NULL
                ORDER BY success_rate DESC, use_count DESC
                LIMIT ?
                "#,
            )
            .bind(pid)
            .bind(limit)
            .fetch_all(self.pool.as_ref())
            .await?
        } else {
            sqlx::query_as::<_, PatternRow>(
                r#"
                SELECT id, project_id, name, description, trigger_type, reasoning_chain,
                       solution_template, applicable_contexts, success_rate, use_count,
                       success_count, cost_savings_usd, created_at, updated_at, last_used
                FROM reasoning_patterns
                ORDER BY success_rate DESC, use_count DESC
                LIMIT ?
                "#,
            )
            .bind(limit)
            .fetch_all(self.pool.as_ref())
            .await?
        };

        let mut patterns = Vec::new();
        for r in rows {
            let contexts: ApplicableContext =
                serde_json::from_str(&r.applicable_contexts.as_deref().unwrap_or("{}"))
                    .unwrap_or_default();

            patterns.push(ReasoningPattern {
                id: r.id.unwrap_or_default(),
                project_id: r.project_id,
                name: r.name,
                description: r.description,
                trigger_type: TriggerType::from_str(&r.trigger_type),
                reasoning_chain: r.reasoning_chain,
                solution_template: r.solution_template,
                applicable_contexts: contexts,
                success_rate: r.success_rate.unwrap_or(1.0),
                use_count: r.use_count.unwrap_or(0) as i32,
                success_count: r.success_count.unwrap_or(0) as i32,
                cost_savings_usd: r.cost_savings_usd.unwrap_or(0.0),
                created_at: parse_timestamp(r.created_at),
                updated_at: parse_timestamp(r.updated_at),
                last_used: parse_timestamp_opt(r.last_used),
                steps: Vec::new(), // Load separately if needed
            });
        }

        Ok(patterns)
    }

    /// Find patterns by trigger type
    pub async fn find_by_trigger(&self, trigger_type: TriggerType) -> Result<Vec<ReasoningPattern>> {
        let trigger_str = trigger_type.as_str();

        let rows = sqlx::query!(
            r#"
            SELECT id, project_id, name, description, trigger_type, reasoning_chain,
                   solution_template, applicable_contexts, success_rate, use_count,
                   success_count, cost_savings_usd, created_at, updated_at, last_used
            FROM reasoning_patterns
            WHERE trigger_type = ? AND success_rate >= 0.5
            ORDER BY success_rate DESC, use_count DESC
            LIMIT 10
            "#,
            trigger_str
        )
        .fetch_all(self.pool.as_ref())
        .await?;

        let mut patterns = Vec::new();
        for r in rows {
            let contexts: ApplicableContext =
                serde_json::from_str(&r.applicable_contexts.unwrap_or_default())
                    .unwrap_or_default();

            patterns.push(ReasoningPattern {
                id: r.id.unwrap_or_default(),
                project_id: r.project_id,
                name: r.name,
                description: r.description,
                trigger_type: TriggerType::from_str(&r.trigger_type),
                reasoning_chain: r.reasoning_chain,
                solution_template: r.solution_template,
                applicable_contexts: contexts,
                success_rate: r.success_rate.unwrap_or(1.0),
                use_count: r.use_count.unwrap_or(0) as i32,
                success_count: r.success_count.unwrap_or(0) as i32,
                cost_savings_usd: r.cost_savings_usd.unwrap_or(0.0),
                created_at: parse_timestamp(r.created_at),
                updated_at: parse_timestamp(r.updated_at),
                last_used: parse_timestamp_opt(r.last_used),
                steps: Vec::new(),
            });
        }

        Ok(patterns)
    }

    /// Store pattern usage
    pub async fn store_usage(&self, usage: &PatternUsage) -> Result<i64> {
        let used_at = usage.used_at.timestamp();

        let id = sqlx::query!(
            r#"
            INSERT INTO pattern_usage (
                pattern_id, operation_id, user_id, context_match_score,
                applied_successfully, outcome_notes, time_saved_ms, cost_saved_usd, used_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
            usage.pattern_id,
            usage.operation_id,
            usage.user_id,
            usage.context_match_score,
            usage.applied_successfully,
            usage.outcome_notes,
            usage.time_saved_ms,
            usage.cost_saved_usd,
            used_at,
        )
        .execute(self.pool.as_ref())
        .await?
        .last_insert_rowid();

        // Update pattern stats
        if usage.applied_successfully {
            sqlx::query!(
                r#"
                UPDATE reasoning_patterns SET
                    use_count = use_count + 1,
                    success_count = success_count + 1,
                    success_rate = CAST(success_count + 1 AS REAL) / CAST(use_count + 1 AS REAL),
                    cost_savings_usd = cost_savings_usd + COALESCE(?, 0),
                    last_used = ?,
                    updated_at = ?
                WHERE id = ?
                "#,
                usage.cost_saved_usd,
                used_at,
                used_at,
                usage.pattern_id,
            )
            .execute(self.pool.as_ref())
            .await?;
        } else {
            sqlx::query!(
                r#"
                UPDATE reasoning_patterns SET
                    use_count = use_count + 1,
                    success_rate = CAST(success_count AS REAL) / CAST(use_count + 1 AS REAL),
                    last_used = ?,
                    updated_at = ?
                WHERE id = ?
                "#,
                used_at,
                used_at,
                usage.pattern_id,
            )
            .execute(self.pool.as_ref())
            .await?;
        }

        debug!(
            "Stored usage for pattern {} (success: {})",
            usage.pattern_id, usage.applied_successfully
        );
        Ok(id)
    }

    /// Get usage history for a pattern
    pub async fn get_usage_history(&self, pattern_id: &str, limit: i32) -> Result<Vec<PatternUsage>> {
        let rows = sqlx::query!(
            r#"
            SELECT id, pattern_id, operation_id, user_id, context_match_score,
                   applied_successfully, outcome_notes, time_saved_ms, cost_saved_usd, used_at
            FROM pattern_usage
            WHERE pattern_id = ?
            ORDER BY used_at DESC
            LIMIT ?
            "#,
            pattern_id,
            limit
        )
        .fetch_all(self.pool.as_ref())
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| PatternUsage {
                id: r.id,
                pattern_id: r.pattern_id,
                operation_id: r.operation_id,
                user_id: r.user_id,
                context_match_score: r.context_match_score,
                applied_successfully: r.applied_successfully,
                outcome_notes: r.outcome_notes,
                time_saved_ms: r.time_saved_ms,
                cost_saved_usd: r.cost_saved_usd,
                used_at: parse_timestamp(r.used_at),
            })
            .collect())
    }

    /// Get pattern statistics
    pub async fn get_stats(&self, project_id: Option<&str>) -> Result<PatternStats> {
        // Total patterns
        let total = if let Some(pid) = project_id {
            sqlx::query!(
                r#"SELECT COUNT(*) as count FROM reasoning_patterns WHERE project_id = ? OR project_id IS NULL"#,
                pid
            )
            .fetch_one(self.pool.as_ref())
            .await?
            .count as i64
        } else {
            sqlx::query!(r#"SELECT COUNT(*) as count FROM reasoning_patterns"#)
                .fetch_one(self.pool.as_ref())
                .await?
                .count as i64
        };

        // Active patterns (success_rate >= 0.5)
        let active = sqlx::query!(
            r#"SELECT COUNT(*) as count FROM reasoning_patterns WHERE success_rate >= 0.5"#
        )
        .fetch_one(self.pool.as_ref())
        .await?
        .count as i64;

        // Deprecated patterns (success_rate < 0.3 and use_count >= 10)
        let deprecated = sqlx::query!(
            r#"SELECT COUNT(*) as count FROM reasoning_patterns WHERE success_rate < 0.3 AND use_count >= 10"#
        )
        .fetch_one(self.pool.as_ref())
        .await?
        .count as i64;

        // Total uses
        let total_uses = sqlx::query!(r#"SELECT SUM(use_count) as total FROM reasoning_patterns"#)
            .fetch_one(self.pool.as_ref())
            .await?
            .total
            .unwrap_or(0);

        // Successful uses
        let successful_uses = sqlx::query!(r#"SELECT SUM(success_count) as total FROM reasoning_patterns"#)
            .fetch_one(self.pool.as_ref())
            .await?
            .total
            .unwrap_or(0);

        // Total cost savings
        let total_savings = sqlx::query!(r#"SELECT SUM(cost_savings_usd) as total FROM reasoning_patterns"#)
            .fetch_one(self.pool.as_ref())
            .await?
            .total
            .unwrap_or(0.0);

        // Top patterns
        let top = sqlx::query!(
            r#"
            SELECT name, use_count
            FROM reasoning_patterns
            ORDER BY use_count DESC
            LIMIT 5
            "#
        )
        .fetch_all(self.pool.as_ref())
        .await?
        .into_iter()
        .map(|r| (r.name, r.use_count.unwrap_or(0) as i64))
        .collect();

        let overall_success_rate = if total_uses > 0 {
            successful_uses as f64 / total_uses as f64
        } else {
            0.0
        };

        Ok(PatternStats {
            total_patterns: total,
            active_patterns: active,
            deprecated_patterns: deprecated,
            total_uses: total_uses as i64,
            successful_uses: successful_uses as i64,
            overall_success_rate,
            total_cost_savings: total_savings,
            top_patterns: top,
        })
    }

    /// Get high-performing patterns for recommendations
    pub async fn get_recommended_patterns(&self, limit: i32) -> Result<Vec<ReasoningPattern>> {
        let rows = sqlx::query!(
            r#"
            SELECT id, project_id, name, description, trigger_type, reasoning_chain,
                   solution_template, applicable_contexts, success_rate, use_count,
                   success_count, cost_savings_usd, created_at, updated_at, last_used
            FROM reasoning_patterns
            WHERE success_rate >= 0.8 AND use_count >= 3
            ORDER BY success_rate DESC, use_count DESC
            LIMIT ?
            "#,
            limit
        )
        .fetch_all(self.pool.as_ref())
        .await?;

        let mut patterns = Vec::new();
        for r in rows {
            let contexts: ApplicableContext =
                serde_json::from_str(&r.applicable_contexts.unwrap_or_default())
                    .unwrap_or_default();

            let pattern_id = r.id.unwrap_or_default();
            let mut pattern = ReasoningPattern {
                id: pattern_id.clone(),
                project_id: r.project_id,
                name: r.name,
                description: r.description,
                trigger_type: TriggerType::from_str(&r.trigger_type),
                reasoning_chain: r.reasoning_chain,
                solution_template: r.solution_template,
                applicable_contexts: contexts,
                success_rate: r.success_rate.unwrap_or(1.0),
                use_count: r.use_count.unwrap_or(0) as i32,
                success_count: r.success_count.unwrap_or(0) as i32,
                cost_savings_usd: r.cost_savings_usd.unwrap_or(0.0),
                created_at: parse_timestamp(r.created_at),
                updated_at: parse_timestamp(r.updated_at),
                last_used: parse_timestamp_opt(r.last_used),
                steps: Vec::new(),
            };

            pattern.steps = self.get_steps(&pattern_id).await?;
            patterns.push(pattern);
        }

        Ok(patterns)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn create_test_pool() -> SqlitePool {
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

        pool
    }

    #[tokio::test]
    async fn test_store_and_retrieve_pattern() {
        let pool = create_test_pool().await;
        let storage = PatternStorage::new(Arc::new(pool));

        let mut pattern = ReasoningPattern::new(
            "test_pattern".to_string(),
            "A test pattern".to_string(),
            TriggerType::Keyword,
            "Step 1 -> Step 2".to_string(),
        );
        pattern.add_step(StepType::Gather, "Gather context");
        pattern.add_step(StepType::Generate, "Generate code");

        storage.store_pattern(&pattern).await.unwrap();

        let retrieved = storage.get_pattern(&pattern.id).await.unwrap().unwrap();
        assert_eq!(retrieved.name, "test_pattern");
        assert_eq!(retrieved.steps.len(), 2);
    }

    #[tokio::test]
    async fn test_usage_tracking() {
        let pool = create_test_pool().await;
        let storage = PatternStorage::new(Arc::new(pool));

        let pattern = ReasoningPattern::new(
            "usage_test".to_string(),
            "Test".to_string(),
            TriggerType::Keyword,
            "chain".to_string(),
        );
        storage.store_pattern(&pattern).await.unwrap();

        let usage = PatternUsage::new(pattern.id.clone(), true)
            .with_savings(1000, 0.05);
        storage.store_usage(&usage).await.unwrap();

        let updated = storage.get_pattern(&pattern.id).await.unwrap().unwrap();
        assert_eq!(updated.use_count, 1);
        assert_eq!(updated.success_count, 1);
    }
}
