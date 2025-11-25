// src/build/tracker.rs
// Build tracking and storage

use anyhow::{Context, Result};
use chrono::{TimeZone, Utc};
use sqlx::SqlitePool;
use std::sync::Arc;
use tracing::{debug, info};

use super::types::*;

/// Build tracker stores build runs and errors
pub struct BuildTracker {
    pool: Arc<SqlitePool>,
}

impl BuildTracker {
    pub fn new(pool: Arc<SqlitePool>) -> Self {
        Self { pool }
    }

    /// Store a build run and return its ID
    pub async fn store_build_run(&self, run: &BuildRun) -> Result<i64> {
        let build_type = run.build_type.as_str();
        let started_at = run.started_at.timestamp();
        let completed_at = run.completed_at.timestamp();

        let id = sqlx::query!(
            r#"
            INSERT INTO build_runs (
                project_id, operation_id, build_type, command, exit_code,
                duration_ms, started_at, completed_at, error_count, warning_count, triggered_by
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
            run.project_id,
            run.operation_id,
            build_type,
            run.command,
            run.exit_code,
            run.duration_ms,
            started_at,
            completed_at,
            run.error_count,
            run.warning_count,
            run.triggered_by,
        )
        .execute(self.pool.as_ref())
        .await
        .context("Failed to insert build run")?
        .last_insert_rowid();

        debug!("Stored build run {} for project {}", id, run.project_id);
        Ok(id)
    }

    /// Store a build error
    pub async fn store_error(&self, error: &BuildError) -> Result<i64> {
        let severity = error.severity.as_str();
        let category = error.category.as_str();
        let first_seen_at = error.first_seen_at.timestamp();
        let last_seen_at = error.last_seen_at.timestamp();
        let resolved_at = error.resolved_at.map(|t| t.timestamp());

        // Check if this error hash already exists
        let existing = sqlx::query!(
            r#"
            SELECT id, occurrence_count FROM build_errors
            WHERE error_hash = ? AND resolved_at IS NULL
            "#,
            error.error_hash
        )
        .fetch_optional(self.pool.as_ref())
        .await?;

        if let Some(row) = existing {
            // Update occurrence count
            let new_count = row.occurrence_count.unwrap_or(1) + 1;
            sqlx::query!(
                r#"
                UPDATE build_errors
                SET occurrence_count = ?, last_seen_at = ?, build_run_id = ?
                WHERE id = ?
                "#,
                new_count,
                last_seen_at,
                error.build_run_id,
                row.id
            )
            .execute(self.pool.as_ref())
            .await?;

            debug!("Updated error occurrence count to {} for hash {}", new_count, error.error_hash);
            return Ok(row.id.unwrap_or(0));
        }

        // Insert new error
        let id = sqlx::query!(
            r#"
            INSERT INTO build_errors (
                build_run_id, error_hash, severity, error_code, message,
                file_path, line_number, column_number, suggestion, code_snippet,
                category, first_seen_at, last_seen_at, occurrence_count, resolved_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
            error.build_run_id,
            error.error_hash,
            severity,
            error.error_code,
            error.message,
            error.file_path,
            error.line_number,
            error.column_number,
            error.suggestion,
            error.code_snippet,
            category,
            first_seen_at,
            last_seen_at,
            error.occurrence_count,
            resolved_at,
        )
        .execute(self.pool.as_ref())
        .await
        .context("Failed to insert build error")?
        .last_insert_rowid();

        debug!("Stored new build error {} with hash {}", id, error.error_hash);
        Ok(id)
    }

    /// Get a build run by ID
    pub async fn get_build_run(&self, id: i64) -> Result<Option<BuildRun>> {
        let row = sqlx::query!(
            r#"
            SELECT id, project_id, operation_id, build_type, command, exit_code,
                   duration_ms, started_at, completed_at, error_count, warning_count, triggered_by
            FROM build_runs WHERE id = ?
            "#,
            id
        )
        .fetch_optional(self.pool.as_ref())
        .await?;

        Ok(row.map(|r| BuildRun {
            id: Some(r.id),
            project_id: r.project_id,
            operation_id: r.operation_id,
            build_type: BuildType::from_str(&r.build_type),
            command: r.command,
            exit_code: r.exit_code as i32,
            duration_ms: r.duration_ms,
            started_at: Utc.timestamp_opt(r.started_at, 0).unwrap(),
            completed_at: Utc.timestamp_opt(r.completed_at, 0).unwrap(),
            error_count: r.error_count.unwrap_or(0) as i32,
            warning_count: r.warning_count.unwrap_or(0) as i32,
            triggered_by: r.triggered_by,
            stdout: None,
            stderr: None,
        }))
    }

    /// Get recent build runs for a project
    pub async fn get_recent_builds(
        &self,
        project_id: &str,
        limit: i32,
    ) -> Result<Vec<BuildRun>> {
        let rows = sqlx::query!(
            r#"
            SELECT id, project_id, operation_id, build_type, command, exit_code,
                   duration_ms, started_at, completed_at, error_count, warning_count, triggered_by
            FROM build_runs
            WHERE project_id = ?
            ORDER BY started_at DESC
            LIMIT ?
            "#,
            project_id,
            limit
        )
        .fetch_all(self.pool.as_ref())
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| BuildRun {
                id: r.id,
                project_id: r.project_id,
                operation_id: r.operation_id,
                build_type: BuildType::from_str(&r.build_type),
                command: r.command,
                exit_code: r.exit_code as i32,
                duration_ms: r.duration_ms,
                started_at: Utc.timestamp_opt(r.started_at, 0).unwrap(),
                completed_at: Utc.timestamp_opt(r.completed_at, 0).unwrap(),
                error_count: r.error_count.unwrap_or(0) as i32,
                warning_count: r.warning_count.unwrap_or(0) as i32,
                triggered_by: r.triggered_by,
                stdout: None,
                stderr: None,
            })
            .collect())
    }

    /// Get errors for a build run
    pub async fn get_errors_for_build(&self, build_run_id: i64) -> Result<Vec<BuildError>> {
        let rows = sqlx::query!(
            r#"
            SELECT id, build_run_id, error_hash, severity, error_code, message,
                   file_path, line_number, column_number, suggestion, code_snippet,
                   category, first_seen_at, last_seen_at, occurrence_count, resolved_at
            FROM build_errors
            WHERE build_run_id = ?
            ORDER BY severity DESC
            "#,
            build_run_id
        )
        .fetch_all(self.pool.as_ref())
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| BuildError {
                id: r.id,
                build_run_id: r.build_run_id,
                error_hash: r.error_hash,
                severity: ErrorSeverity::from_str(&r.severity),
                error_code: r.error_code,
                message: r.message,
                file_path: r.file_path,
                line_number: r.line_number.map(|n| n as i32),
                column_number: r.column_number.map(|n| n as i32),
                suggestion: r.suggestion,
                code_snippet: r.code_snippet,
                category: ErrorCategory::from_str(&r.category.unwrap_or_default()),
                first_seen_at: Utc.timestamp_opt(r.first_seen_at, 0).unwrap(),
                last_seen_at: Utc.timestamp_opt(r.last_seen_at, 0).unwrap(),
                occurrence_count: r.occurrence_count.unwrap_or(1) as i32,
                resolved_at: r.resolved_at.map(|t| Utc.timestamp_opt(t, 0).unwrap()),
            })
            .collect())
    }

    /// Get unresolved errors for a project
    pub async fn get_unresolved_errors(&self, project_id: &str, limit: i32) -> Result<Vec<BuildError>> {
        let rows = sqlx::query!(
            r#"
            SELECT e.id, e.build_run_id, e.error_hash, e.severity, e.error_code, e.message,
                   e.file_path, e.line_number, e.column_number, e.suggestion, e.code_snippet,
                   e.category, e.first_seen_at, e.last_seen_at, e.occurrence_count, e.resolved_at
            FROM build_errors e
            JOIN build_runs r ON e.build_run_id = r.id
            WHERE r.project_id = ? AND e.resolved_at IS NULL AND e.severity = 'error'
            ORDER BY e.occurrence_count DESC, e.last_seen_at DESC
            LIMIT ?
            "#,
            project_id,
            limit
        )
        .fetch_all(self.pool.as_ref())
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| BuildError {
                id: r.id,
                build_run_id: r.build_run_id,
                error_hash: r.error_hash,
                severity: ErrorSeverity::from_str(&r.severity),
                error_code: r.error_code,
                message: r.message,
                file_path: r.file_path,
                line_number: r.line_number.map(|n| n as i32),
                column_number: r.column_number.map(|n| n as i32),
                suggestion: r.suggestion,
                code_snippet: r.code_snippet,
                category: ErrorCategory::from_str(&r.category.unwrap_or_default()),
                first_seen_at: Utc.timestamp_opt(r.first_seen_at, 0).unwrap(),
                last_seen_at: Utc.timestamp_opt(r.last_seen_at, 0).unwrap(),
                occurrence_count: r.occurrence_count.unwrap_or(1) as i32,
                resolved_at: None,
            })
            .collect())
    }

    /// Mark errors as resolved
    pub async fn resolve_errors(&self, error_hashes: &[String]) -> Result<i64> {
        let now = Utc::now().timestamp();
        let mut resolved_count = 0i64;

        for hash in error_hashes {
            let result = sqlx::query!(
                r#"
                UPDATE build_errors
                SET resolved_at = ?
                WHERE error_hash = ? AND resolved_at IS NULL
                "#,
                now,
                hash
            )
            .execute(self.pool.as_ref())
            .await?;

            resolved_count += result.rows_affected() as i64;
        }

        info!("Resolved {} errors", resolved_count);
        Ok(resolved_count)
    }

    /// Auto-resolve errors that haven't recurred in recent successful builds
    pub async fn auto_resolve_stale_errors(&self, project_id: &str, min_builds: i32) -> Result<i64> {
        // Find error hashes that haven't appeared in the last N successful builds
        let rows = sqlx::query!(
            r#"
            SELECT DISTINCT e.error_hash
            FROM build_errors e
            JOIN build_runs r ON e.build_run_id = r.id
            WHERE r.project_id = ? AND e.resolved_at IS NULL
            AND e.error_hash NOT IN (
                SELECT DISTINCT e2.error_hash
                FROM build_errors e2
                JOIN build_runs r2 ON e2.build_run_id = r2.id
                WHERE r2.project_id = ? AND r2.exit_code != 0
                ORDER BY r2.started_at DESC
                LIMIT ?
            )
            "#,
            project_id,
            project_id,
            min_builds
        )
        .fetch_all(self.pool.as_ref())
        .await?;

        let hashes: Vec<String> = rows.into_iter().map(|r| r.error_hash).collect();

        if hashes.is_empty() {
            return Ok(0);
        }

        // Mark these as auto-resolved
        let now = Utc::now().timestamp();
        let mut resolved_count = 0i64;

        for hash in &hashes {
            let result = sqlx::query!(
                r#"
                UPDATE build_errors
                SET resolved_at = ?
                WHERE error_hash = ? AND resolved_at IS NULL
                "#,
                now,
                hash
            )
            .execute(self.pool.as_ref())
            .await?;

            resolved_count += result.rows_affected() as i64;
        }

        info!("Auto-resolved {} stale errors for project {}", resolved_count, project_id);
        Ok(resolved_count)
    }

    /// Get build statistics for a project
    pub async fn get_build_stats(&self, project_id: &str) -> Result<BuildStats> {
        // Total builds
        let total = sqlx::query!(
            r#"SELECT COUNT(*) as count FROM build_runs WHERE project_id = ?"#,
            project_id
        )
        .fetch_one(self.pool.as_ref())
        .await?
        .count as i64;

        // Successful builds
        let successful = sqlx::query!(
            r#"SELECT COUNT(*) as count FROM build_runs WHERE project_id = ? AND exit_code = 0"#,
            project_id
        )
        .fetch_one(self.pool.as_ref())
        .await?
        .count as i64;

        // Average duration
        let avg_duration = sqlx::query!(
            r#"SELECT AVG(duration_ms) as avg FROM build_runs WHERE project_id = ?"#,
            project_id
        )
        .fetch_one(self.pool.as_ref())
        .await?
        .avg
        .map(|v| v as f64)
        .unwrap_or(0.0);

        // Total errors
        let total_errors = sqlx::query!(
            r#"
            SELECT COUNT(*) as count FROM build_errors e
            JOIN build_runs r ON e.build_run_id = r.id
            WHERE r.project_id = ? AND e.severity = 'error'
            "#,
            project_id
        )
        .fetch_one(self.pool.as_ref())
        .await?
        .count as i64;

        // Resolved errors
        let resolved_errors = sqlx::query!(
            r#"
            SELECT COUNT(*) as count FROM build_errors e
            JOIN build_runs r ON e.build_run_id = r.id
            WHERE r.project_id = ? AND e.severity = 'error' AND e.resolved_at IS NOT NULL
            "#,
            project_id
        )
        .fetch_one(self.pool.as_ref())
        .await?
        .count as i64;

        // Most common errors
        let common_errors = sqlx::query!(
            r#"
            SELECT e.message, COUNT(*) as count
            FROM build_errors e
            JOIN build_runs r ON e.build_run_id = r.id
            WHERE r.project_id = ? AND e.severity = 'error'
            GROUP BY e.error_hash
            ORDER BY count DESC
            LIMIT 5
            "#,
            project_id
        )
        .fetch_all(self.pool.as_ref())
        .await?
        .into_iter()
        .map(|r| (r.message, r.count as i64))
        .collect();

        let success_rate = if total > 0 {
            successful as f64 / total as f64
        } else {
            0.0
        };

        Ok(BuildStats {
            project_id: project_id.to_string(),
            total_builds: total,
            successful_builds: successful,
            failed_builds: total - successful,
            success_rate,
            total_errors,
            resolved_errors,
            unresolved_errors: total_errors - resolved_errors,
            average_duration_ms: avg_duration,
            most_common_errors: common_errors,
        })
    }

    /// Store context injection record
    pub async fn store_context_injection(&self, injection: &BuildContextInjection) -> Result<i64> {
        let error_ids_json = serde_json::to_string(&injection.error_ids)?;
        let injected_at = injection.injected_at.timestamp();

        let id = sqlx::query!(
            r#"
            INSERT INTO build_context_injections (operation_id, build_run_id, error_ids, injected_at)
            VALUES (?, ?, ?, ?)
            "#,
            injection.operation_id,
            injection.build_run_id,
            error_ids_json,
            injected_at,
        )
        .execute(self.pool.as_ref())
        .await?
        .last_insert_rowid();

        debug!(
            "Stored context injection {} for operation {}",
            id, injection.operation_id
        );
        Ok(id)
    }

    /// Get errors to inject into context for an operation
    pub async fn get_errors_for_context(&self, project_id: &str, limit: i32) -> Result<Vec<BuildError>> {
        // Get most recent unresolved errors
        self.get_unresolved_errors(project_id, limit).await
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

        // Create tables
        sqlx::query(
            r#"
            CREATE TABLE build_runs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                project_id TEXT NOT NULL,
                operation_id TEXT,
                build_type TEXT NOT NULL,
                command TEXT NOT NULL,
                exit_code INTEGER NOT NULL,
                duration_ms INTEGER NOT NULL,
                started_at INTEGER NOT NULL,
                completed_at INTEGER NOT NULL,
                error_count INTEGER DEFAULT 0,
                warning_count INTEGER DEFAULT 0,
                triggered_by TEXT
            )
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            r#"
            CREATE TABLE build_errors (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                build_run_id INTEGER NOT NULL,
                error_hash TEXT NOT NULL,
                severity TEXT NOT NULL,
                error_code TEXT,
                message TEXT NOT NULL,
                file_path TEXT,
                line_number INTEGER,
                column_number INTEGER,
                suggestion TEXT,
                code_snippet TEXT,
                category TEXT,
                first_seen_at INTEGER NOT NULL,
                last_seen_at INTEGER NOT NULL,
                occurrence_count INTEGER DEFAULT 1,
                resolved_at INTEGER
            )
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            r#"
            CREATE TABLE build_context_injections (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                operation_id TEXT NOT NULL,
                build_run_id INTEGER NOT NULL,
                error_ids TEXT NOT NULL,
                injected_at INTEGER NOT NULL
            )
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        pool
    }

    #[tokio::test]
    async fn test_store_and_retrieve_build() {
        let pool = create_test_pool().await;
        let tracker = BuildTracker::new(Arc::new(pool));

        let run = BuildRun::new("test_project".to_string(), "cargo build".to_string());
        let id = tracker.store_build_run(&run).await.unwrap();

        let retrieved = tracker.get_build_run(id).await.unwrap().unwrap();
        assert_eq!(retrieved.project_id, "test_project");
        assert_eq!(retrieved.command, "cargo build");
    }

    #[tokio::test]
    async fn test_error_deduplication() {
        let pool = create_test_pool().await;
        let tracker = BuildTracker::new(Arc::new(pool));

        // Store a build run first
        let run = BuildRun::new("test_project".to_string(), "cargo build".to_string());
        let build_id = tracker.store_build_run(&run).await.unwrap();

        // Store same error twice
        let error = BuildError {
            id: None,
            build_run_id: build_id,
            error_hash: "test_hash".to_string(),
            severity: ErrorSeverity::Error,
            error_code: Some("E0308".to_string()),
            message: "mismatched types".to_string(),
            file_path: Some("src/main.rs".to_string()),
            line_number: Some(42),
            column_number: Some(10),
            suggestion: None,
            code_snippet: None,
            category: ErrorCategory::Type,
            first_seen_at: Utc::now(),
            last_seen_at: Utc::now(),
            occurrence_count: 1,
            resolved_at: None,
        };

        tracker.store_error(&error).await.unwrap();
        tracker.store_error(&error).await.unwrap();

        // Check that occurrence count increased
        let errors = tracker.get_errors_for_build(build_id).await.unwrap();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].occurrence_count, 2);
    }
}
