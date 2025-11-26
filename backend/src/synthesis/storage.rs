// src/synthesis/storage.rs
// Database storage for tool synthesis artifacts

use anyhow::{Context, Result};
use chrono::{TimeZone, Utc};
use sqlx::SqlitePool;
use std::sync::Arc;
use tracing::{debug, info};

use super::types::*;

/// Storage layer for tool synthesis data
pub struct SynthesisStorage {
    pool: Arc<SqlitePool>,
}

impl SynthesisStorage {
    pub fn new(pool: Arc<SqlitePool>) -> Self {
        Self { pool }
    }

    // ========================================================================
    // Pattern Operations
    // ========================================================================

    /// Store a new pattern
    pub async fn store_pattern(&self, pattern: &ToolPattern) -> Result<i64> {
        let pattern_type = pattern.pattern_type.as_str();
        let locations_json = serde_json::to_string(&pattern.example_locations)?;
        let now = Utc::now().timestamp();

        let result = sqlx::query(
            r#"
            INSERT INTO tool_patterns (
                project_id, pattern_name, pattern_type, description,
                detected_occurrences, example_locations, confidence_score,
                should_synthesize, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&pattern.project_id)
        .bind(&pattern.pattern_name)
        .bind(pattern_type)
        .bind(&pattern.description)
        .bind(pattern.detected_occurrences)
        .bind(&locations_json)
        .bind(pattern.confidence_score)
        .bind(pattern.should_synthesize)
        .bind(now)
        .bind(now)
        .execute(self.pool.as_ref())
        .await
        .context("Failed to store pattern")?;

        let id = result.last_insert_rowid();
        info!("Stored pattern {} with id {}", pattern.pattern_name, id);
        Ok(id)
    }

    /// Get a pattern by ID
    pub async fn get_pattern(&self, id: i64) -> Result<Option<ToolPattern>> {
        let row = sqlx::query_as::<_, PatternRow>(
            r#"
            SELECT id, project_id, pattern_name, pattern_type, description,
                   detected_occurrences, example_locations, confidence_score,
                   should_synthesize, created_at, updated_at
            FROM tool_patterns WHERE id = ?
            "#,
        )
        .bind(id)
        .fetch_optional(self.pool.as_ref())
        .await
        .context("Failed to get pattern")?;

        Ok(row.map(|r| r.into_pattern()))
    }

    /// List patterns for a project
    pub async fn list_patterns(
        &self,
        project_id: &str,
        without_tools_only: bool,
    ) -> Result<Vec<ToolPattern>> {
        let sql = if without_tools_only {
            r#"
            SELECT p.id, p.project_id, p.pattern_name, p.pattern_type, p.description,
                   p.detected_occurrences, p.example_locations, p.confidence_score,
                   p.should_synthesize, p.created_at, p.updated_at
            FROM tool_patterns p
            LEFT JOIN synthesized_tools t ON t.tool_pattern_id = p.id
            WHERE p.project_id = ? AND t.id IS NULL
            ORDER BY p.confidence_score DESC
            "#
        } else {
            r#"
            SELECT id, project_id, pattern_name, pattern_type, description,
                   detected_occurrences, example_locations, confidence_score,
                   should_synthesize, created_at, updated_at
            FROM tool_patterns WHERE project_id = ?
            ORDER BY confidence_score DESC
            "#
        };

        let rows = sqlx::query_as::<_, PatternRow>(sql)
            .bind(project_id)
            .fetch_all(self.pool.as_ref())
            .await
            .context("Failed to list patterns")?;

        Ok(rows.into_iter().map(|r| r.into_pattern()).collect())
    }

    /// Mark a pattern as having a generated tool
    pub async fn mark_pattern_tool_generated(&self, pattern_id: i64) -> Result<()> {
        sqlx::query("UPDATE tool_patterns SET should_synthesize = FALSE, updated_at = ? WHERE id = ?")
            .bind(Utc::now().timestamp())
            .bind(pattern_id)
            .execute(self.pool.as_ref())
            .await
            .context("Failed to update pattern")?;

        Ok(())
    }

    // ========================================================================
    // Tool Operations
    // ========================================================================

    /// Store a new synthesized tool
    pub async fn store_tool(&self, tool: &SynthesizedTool) -> Result<()> {
        let status = tool.compilation_status.as_str();
        let now = Utc::now().timestamp();

        sqlx::query(
            r#"
            INSERT INTO synthesized_tools (
                id, project_id, tool_pattern_id, name, description,
                version, source_code, language, compilation_status,
                compilation_error, binary_path, enabled, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&tool.id)
        .bind(&tool.project_id)
        .bind(tool.tool_pattern_id)
        .bind(&tool.name)
        .bind(&tool.description)
        .bind(tool.version)
        .bind(&tool.source_code)
        .bind(&tool.language)
        .bind(status)
        .bind(&tool.compilation_error)
        .bind(&tool.binary_path)
        .bind(tool.enabled)
        .bind(now)
        .bind(now)
        .execute(self.pool.as_ref())
        .await
        .context("Failed to store tool")?;

        info!("Stored tool {} (id: {})", tool.name, tool.id);
        Ok(())
    }

    /// Get a tool by name
    pub async fn get_tool(&self, name: &str) -> Result<Option<SynthesizedTool>> {
        let row = sqlx::query_as::<_, ToolRow>(
            r#"
            SELECT id, project_id, tool_pattern_id, name, description,
                   version, source_code, language, compilation_status,
                   compilation_error, binary_path, enabled, created_at, updated_at
            FROM synthesized_tools WHERE name = ?
            "#,
        )
        .bind(name)
        .fetch_optional(self.pool.as_ref())
        .await
        .context("Failed to get tool")?;

        Ok(row.map(|r| r.into_tool()))
    }

    /// Get a tool by ID
    pub async fn get_tool_by_id(&self, id: &str) -> Result<Option<SynthesizedTool>> {
        let row = sqlx::query_as::<_, ToolRow>(
            r#"
            SELECT id, project_id, tool_pattern_id, name, description,
                   version, source_code, language, compilation_status,
                   compilation_error, binary_path, enabled, created_at, updated_at
            FROM synthesized_tools WHERE id = ?
            "#,
        )
        .bind(id)
        .fetch_optional(self.pool.as_ref())
        .await
        .context("Failed to get tool")?;

        Ok(row.map(|r| r.into_tool()))
    }

    /// Update a tool
    pub async fn update_tool(&self, tool: &SynthesizedTool) -> Result<()> {
        let status = tool.compilation_status.as_str();
        let now = Utc::now().timestamp();

        sqlx::query(
            r#"
            UPDATE synthesized_tools SET
                description = ?, version = ?, source_code = ?,
                compilation_status = ?, compilation_error = ?,
                binary_path = ?, enabled = ?, updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind(&tool.description)
        .bind(tool.version)
        .bind(&tool.source_code)
        .bind(status)
        .bind(&tool.compilation_error)
        .bind(&tool.binary_path)
        .bind(tool.enabled)
        .bind(now)
        .bind(&tool.id)
        .execute(self.pool.as_ref())
        .await
        .context("Failed to update tool")?;

        debug!("Updated tool {}", tool.name);
        Ok(())
    }

    /// List tools for a project
    pub async fn list_tools(&self, project_id: &str, active_only: bool) -> Result<Vec<SynthesizedTool>> {
        let sql = if active_only {
            r#"
            SELECT id, project_id, tool_pattern_id, name, description,
                   version, source_code, language, compilation_status,
                   compilation_error, binary_path, enabled, created_at, updated_at
            FROM synthesized_tools
            WHERE project_id = ? AND enabled = TRUE AND compilation_status = 'success'
            ORDER BY name
            "#
        } else {
            r#"
            SELECT id, project_id, tool_pattern_id, name, description,
                   version, source_code, language, compilation_status,
                   compilation_error, binary_path, enabled, created_at, updated_at
            FROM synthesized_tools WHERE project_id = ?
            ORDER BY name
            "#
        };

        let rows = sqlx::query_as::<_, ToolRow>(sql)
            .bind(project_id)
            .fetch_all(self.pool.as_ref())
            .await
            .context("Failed to list tools")?;

        Ok(rows.into_iter().map(|r| r.into_tool()).collect())
    }

    /// Deactivate a tool
    pub async fn deactivate_tool(&self, name: &str) -> Result<()> {
        sqlx::query("UPDATE synthesized_tools SET enabled = FALSE, updated_at = ? WHERE name = ?")
            .bind(Utc::now().timestamp())
            .bind(name)
            .execute(self.pool.as_ref())
            .await
            .context("Failed to deactivate tool")?;

        Ok(())
    }

    // ========================================================================
    // Execution Tracking
    // ========================================================================

    /// Record a tool execution
    pub async fn record_execution(&self, execution: &ToolExecution) -> Result<i64> {
        let args_json = execution
            .arguments
            .as_ref()
            .map(|a| serde_json::to_string(a).unwrap_or_default());

        let result = sqlx::query(
            r#"
            INSERT INTO tool_executions (
                tool_id, operation_id, session_id, user_id,
                arguments, success, output, error_message,
                duration_ms, executed_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&execution.tool_id)
        .bind(&execution.operation_id)
        .bind(&execution.session_id)
        .bind(&execution.user_id)
        .bind(&args_json)
        .bind(execution.success)
        .bind(&execution.output)
        .bind(&execution.error_message)
        .bind(execution.duration_ms)
        .bind(execution.executed_at.timestamp())
        .execute(self.pool.as_ref())
        .await
        .context("Failed to record execution")?;

        let id = result.last_insert_rowid();

        // Update effectiveness metrics
        self.update_effectiveness(&execution.tool_id, execution).await?;

        Ok(id)
    }

    /// Update effectiveness metrics for a tool
    async fn update_effectiveness(&self, tool_id: &str, execution: &ToolExecution) -> Result<()> {
        let now = Utc::now().timestamp();

        // Check if effectiveness record exists
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM tool_effectiveness WHERE tool_id = ?)",
        )
        .bind(tool_id)
        .fetch_one(self.pool.as_ref())
        .await
        .context("Failed to check effectiveness")?;

        if exists {
            // Update existing record
            let (success_inc, fail_inc) = if execution.success { (1, 0) } else { (0, 1) };

            sqlx::query(
                r#"
                UPDATE tool_effectiveness SET
                    total_executions = total_executions + 1,
                    successful_executions = successful_executions + ?,
                    failed_executions = failed_executions + ?,
                    average_duration_ms = (
                        (average_duration_ms * total_executions + ?) / (total_executions + 1)
                    ),
                    last_executed = ?,
                    updated_at = ?
                WHERE tool_id = ?
                "#,
            )
            .bind(success_inc)
            .bind(fail_inc)
            .bind(execution.duration_ms as f64)
            .bind(now)
            .bind(now)
            .bind(tool_id)
            .execute(self.pool.as_ref())
            .await
            .context("Failed to update effectiveness")?;
        } else {
            // Create new record
            let (success_count, fail_count) = if execution.success { (1, 0) } else { (0, 1) };

            sqlx::query(
                r#"
                INSERT INTO tool_effectiveness (
                    tool_id, total_executions, successful_executions,
                    failed_executions, average_duration_ms, total_time_saved_ms,
                    last_executed, created_at, updated_at
                ) VALUES (?, 1, ?, ?, ?, 0, ?, ?, ?)
                "#,
            )
            .bind(tool_id)
            .bind(success_count)
            .bind(fail_count)
            .bind(execution.duration_ms as f64)
            .bind(now)
            .bind(now)
            .bind(now)
            .execute(self.pool.as_ref())
            .await
            .context("Failed to create effectiveness")?;
        }

        Ok(())
    }

    /// Get effectiveness metrics for a tool
    pub async fn get_effectiveness(&self, tool_id: &str) -> Result<Option<ToolEffectiveness>> {
        let row = sqlx::query_as::<_, EffectivenessRow>(
            r#"
            SELECT tool_id, total_executions, successful_executions,
                   failed_executions, average_duration_ms, total_time_saved_ms,
                   last_executed, created_at, updated_at
            FROM tool_effectiveness WHERE tool_id = ?
            "#,
        )
        .bind(tool_id)
        .fetch_optional(self.pool.as_ref())
        .await
        .context("Failed to get effectiveness")?;

        Ok(row.map(|r| r.into_effectiveness()))
    }

    /// Get tools below effectiveness threshold
    pub async fn get_tools_below_threshold(&self, threshold: f64) -> Result<Vec<String>> {
        let rows: Vec<(String,)> = sqlx::query_as(
            r#"
            SELECT tool_id FROM tool_effectiveness
            WHERE total_executions > 5
              AND CAST(successful_executions AS REAL) / total_executions < ?
            "#,
        )
        .bind(threshold)
        .fetch_all(self.pool.as_ref())
        .await
        .context("Failed to get tools below threshold")?;

        Ok(rows.into_iter().map(|(id,)| id).collect())
    }

    // ========================================================================
    // Feedback
    // ========================================================================

    /// Record user feedback
    pub async fn record_feedback(&self, feedback: &ToolFeedback) -> Result<i64> {
        let result = sqlx::query(
            r#"
            INSERT INTO tool_feedback (
                tool_id, execution_id, user_id, rating, comment, issue_type, created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&feedback.tool_id)
        .bind(feedback.execution_id)
        .bind(&feedback.user_id)
        .bind(feedback.rating)
        .bind(&feedback.comment)
        .bind(&feedback.issue_type)
        .bind(feedback.created_at.timestamp())
        .execute(self.pool.as_ref())
        .await
        .context("Failed to record feedback")?;

        Ok(result.last_insert_rowid())
    }

    // ========================================================================
    // Evolution
    // ========================================================================

    /// Record a tool evolution
    pub async fn record_evolution(&self, evolution: &ToolEvolution) -> Result<i64> {
        let result = sqlx::query(
            r#"
            INSERT INTO tool_evolution_history (
                tool_id, old_version, new_version, change_description,
                motivation, source_code_diff, evolved_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&evolution.tool_id)
        .bind(evolution.old_version)
        .bind(evolution.new_version)
        .bind(&evolution.change_description)
        .bind(&evolution.motivation)
        .bind(&evolution.source_code_diff)
        .bind(evolution.evolved_at.timestamp())
        .execute(self.pool.as_ref())
        .await
        .context("Failed to record evolution")?;

        Ok(result.last_insert_rowid())
    }

    // ========================================================================
    // Statistics
    // ========================================================================

    /// Get synthesis statistics
    pub async fn get_statistics(&self, effectiveness_threshold: f64) -> Result<SynthesisStats> {
        let total_patterns: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM tool_patterns")
                .fetch_one(self.pool.as_ref())
                .await?;

        let patterns_with_tools: i64 = sqlx::query_scalar(
            "SELECT COUNT(DISTINCT tool_pattern_id) FROM synthesized_tools WHERE tool_pattern_id IS NOT NULL",
        )
        .fetch_one(self.pool.as_ref())
        .await?;

        let total_tools: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM synthesized_tools")
                .fetch_one(self.pool.as_ref())
                .await?;

        let active_tools: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM synthesized_tools WHERE enabled = TRUE AND compilation_status = 'success'",
        )
        .fetch_one(self.pool.as_ref())
        .await?;

        let total_executions: i64 =
            sqlx::query_scalar("SELECT COALESCE(SUM(total_executions), 0) FROM tool_effectiveness")
                .fetch_one(self.pool.as_ref())
                .await?;

        let successful_executions: i64 = sqlx::query_scalar(
            "SELECT COALESCE(SUM(successful_executions), 0) FROM tool_effectiveness",
        )
        .fetch_one(self.pool.as_ref())
        .await?;

        let average_success_rate = if total_executions > 0 {
            successful_executions as f64 / total_executions as f64
        } else {
            0.0
        };

        let tools_below_threshold: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*) FROM tool_effectiveness
            WHERE total_executions > 5
              AND CAST(successful_executions AS REAL) / total_executions < ?
            "#,
        )
        .bind(effectiveness_threshold)
        .fetch_one(self.pool.as_ref())
        .await?;

        Ok(SynthesisStats {
            total_patterns,
            patterns_with_tools,
            total_tools,
            active_tools,
            total_executions,
            successful_executions,
            average_success_rate,
            tools_below_threshold,
        })
    }
}

// ============================================================================
// Row Types for SQLx
// ============================================================================

#[derive(sqlx::FromRow)]
struct PatternRow {
    id: i64,
    project_id: String,
    pattern_name: String,
    pattern_type: String,
    description: String,
    detected_occurrences: i64,
    example_locations: String,
    confidence_score: f64,
    should_synthesize: bool,
    created_at: i64,
    updated_at: i64,
}

impl PatternRow {
    fn into_pattern(self) -> ToolPattern {
        let locations: Vec<PatternLocation> =
            serde_json::from_str(&self.example_locations).unwrap_or_default();

        ToolPattern {
            id: Some(self.id),
            project_id: self.project_id,
            pattern_name: self.pattern_name,
            pattern_type: PatternType::from_str(&self.pattern_type),
            description: self.description,
            detected_occurrences: self.detected_occurrences,
            example_locations: locations,
            confidence_score: self.confidence_score,
            should_synthesize: self.should_synthesize,
            created_at: Utc.timestamp_opt(self.created_at, 0).unwrap(),
            updated_at: Utc.timestamp_opt(self.updated_at, 0).unwrap(),
        }
    }
}

#[derive(sqlx::FromRow)]
struct ToolRow {
    id: String,
    project_id: String,
    tool_pattern_id: Option<i64>,
    name: String,
    description: String,
    version: i64,
    source_code: String,
    language: String,
    compilation_status: String,
    compilation_error: Option<String>,
    binary_path: Option<String>,
    enabled: bool,
    created_at: i64,
    updated_at: i64,
}

impl ToolRow {
    fn into_tool(self) -> SynthesizedTool {
        SynthesizedTool {
            id: self.id,
            project_id: self.project_id,
            tool_pattern_id: self.tool_pattern_id,
            name: self.name,
            description: self.description,
            version: self.version,
            source_code: self.source_code,
            language: self.language,
            compilation_status: CompilationStatus::from_str(&self.compilation_status),
            compilation_error: self.compilation_error,
            binary_path: self.binary_path,
            enabled: self.enabled,
            created_at: Utc.timestamp_opt(self.created_at, 0).unwrap(),
            updated_at: Utc.timestamp_opt(self.updated_at, 0).unwrap(),
        }
    }
}

#[derive(sqlx::FromRow)]
struct EffectivenessRow {
    tool_id: String,
    total_executions: i64,
    successful_executions: i64,
    failed_executions: i64,
    average_duration_ms: Option<f64>,
    total_time_saved_ms: i64,
    last_executed: Option<i64>,
    created_at: i64,
    updated_at: i64,
}

impl EffectivenessRow {
    fn into_effectiveness(self) -> ToolEffectiveness {
        ToolEffectiveness {
            tool_id: self.tool_id,
            total_executions: self.total_executions,
            successful_executions: self.successful_executions,
            failed_executions: self.failed_executions,
            average_duration_ms: self.average_duration_ms,
            total_time_saved_ms: self.total_time_saved_ms,
            last_executed: self
                .last_executed
                .and_then(|t| Utc.timestamp_opt(t, 0).single()),
            created_at: Utc.timestamp_opt(self.created_at, 0).unwrap(),
            updated_at: Utc.timestamp_opt(self.updated_at, 0).unwrap(),
        }
    }
}
