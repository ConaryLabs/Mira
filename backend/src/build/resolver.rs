// src/build/resolver.rs
// Error resolution tracking and historical fix lookup

use anyhow::{Context, Result};
use chrono::{TimeZone, Utc};
use sqlx::SqlitePool;
use std::sync::Arc;
use tracing::{debug, info};

use super::tracker::BuildTracker;
use super::types::*;

/// Error resolver links errors to fixes and finds historical resolutions
pub struct ErrorResolver {
    pool: Arc<SqlitePool>,
    tracker: Arc<BuildTracker>,
}

impl ErrorResolver {
    pub fn new(pool: Arc<SqlitePool>, tracker: Arc<BuildTracker>) -> Self {
        Self { pool, tracker }
    }

    /// Record a resolution for an error
    pub async fn record_resolution(
        &self,
        error_hash: &str,
        resolution_type: ResolutionType,
        files_changed: Vec<String>,
        commit_hash: Option<&str>,
        resolution_time_ms: Option<i64>,
        notes: Option<&str>,
    ) -> Result<i64> {
        let now = Utc::now();
        let resolved_at = now.timestamp();
        let files_json = serde_json::to_string(&files_changed)?;
        let resolution_type_str = resolution_type.as_str();

        // Insert resolution record
        let id = sqlx::query!(
            r#"
            INSERT INTO error_resolutions (
                error_hash, resolution_type, files_changed, commit_hash,
                resolution_time_ms, resolved_at, notes
            ) VALUES (?, ?, ?, ?, ?, ?, ?)
            "#,
            error_hash,
            resolution_type_str,
            files_json,
            commit_hash,
            resolution_time_ms,
            resolved_at,
            notes,
        )
        .execute(self.pool.as_ref())
        .await
        .context("Failed to insert resolution")?
        .last_insert_rowid();

        // Mark the error as resolved
        self.tracker.resolve_errors(&[error_hash.to_string()]).await?;

        info!(
            "Recorded resolution {} for error {} (type: {:?})",
            id, error_hash, resolution_type
        );
        Ok(id)
    }

    /// Find historical resolutions for an error hash
    pub async fn find_resolutions(&self, error_hash: &str) -> Result<Vec<ErrorResolution>> {
        let rows = sqlx::query!(
            r#"
            SELECT id, error_hash, resolution_type, files_changed, commit_hash,
                   resolution_time_ms, resolved_at, notes
            FROM error_resolutions
            WHERE error_hash = ?
            ORDER BY resolved_at DESC
            "#,
            error_hash
        )
        .fetch_all(self.pool.as_ref())
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| {
                let files: Vec<String> =
                    serde_json::from_str(&r.files_changed.unwrap_or_default()).unwrap_or_default();

                ErrorResolution {
                    id: r.id,
                    error_hash: r.error_hash,
                    resolution_type: ResolutionType::from_str(&r.resolution_type),
                    files_changed: files,
                    commit_hash: r.commit_hash,
                    resolution_time_ms: r.resolution_time_ms,
                    resolved_at: Utc.timestamp_opt(r.resolved_at, 0).unwrap(),
                    notes: r.notes,
                }
            })
            .collect())
    }

    /// Find similar errors that have been resolved
    pub async fn find_similar_resolutions(&self, error: &BuildError) -> Result<Vec<SimilarResolution>> {
        let mut similar = Vec::new();

        // 1. Exact hash match
        let exact = self.find_resolutions(&error.error_hash).await?;
        for res in exact {
            similar.push(SimilarResolution {
                resolution: res,
                similarity_score: 1.0,
                match_reason: "Exact error hash match".to_string(),
            });
        }

        // 2. Same error code match
        if let Some(ref code) = error.error_code {
            let code_matches = self.find_resolutions_by_error_code(code).await?;
            for res in code_matches {
                if !similar.iter().any(|s| s.resolution.error_hash == res.error_hash) {
                    similar.push(SimilarResolution {
                        resolution: res,
                        similarity_score: 0.8,
                        match_reason: format!("Same error code: {}", code),
                    });
                }
            }
        }

        // 3. Same category match
        let category_matches = self
            .find_resolutions_by_category(error.category)
            .await?;
        for res in category_matches.into_iter().take(3) {
            if !similar.iter().any(|s| s.resolution.error_hash == res.error_hash) {
                similar.push(SimilarResolution {
                    resolution: res,
                    similarity_score: 0.5,
                    match_reason: format!("Same category: {}", error.category.as_str()),
                });
            }
        }

        // Sort by similarity score
        similar.sort_by(|a, b| b.similarity_score.partial_cmp(&a.similarity_score).unwrap());

        debug!(
            "Found {} similar resolutions for error {}",
            similar.len(),
            error.error_hash
        );
        Ok(similar)
    }

    /// Find resolutions by error code
    async fn find_resolutions_by_error_code(&self, error_code: &str) -> Result<Vec<ErrorResolution>> {
        let rows = sqlx::query!(
            r#"
            SELECT DISTINCT r.id, r.error_hash, r.resolution_type, r.files_changed,
                   r.commit_hash, r.resolution_time_ms, r.resolved_at, r.notes
            FROM error_resolutions r
            JOIN build_errors e ON r.error_hash = e.error_hash
            WHERE e.error_code = ?
            ORDER BY r.resolved_at DESC
            LIMIT 5
            "#,
            error_code
        )
        .fetch_all(self.pool.as_ref())
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| {
                let files: Vec<String> =
                    serde_json::from_str(&r.files_changed.unwrap_or_default()).unwrap_or_default();

                ErrorResolution {
                    id: r.id,
                    error_hash: r.error_hash,
                    resolution_type: ResolutionType::from_str(&r.resolution_type),
                    files_changed: files,
                    commit_hash: r.commit_hash,
                    resolution_time_ms: r.resolution_time_ms,
                    resolved_at: Utc.timestamp_opt(r.resolved_at, 0).unwrap(),
                    notes: r.notes,
                }
            })
            .collect())
    }

    /// Find resolutions by error category
    async fn find_resolutions_by_category(
        &self,
        category: ErrorCategory,
    ) -> Result<Vec<ErrorResolution>> {
        let category_str = category.as_str();

        let rows = sqlx::query!(
            r#"
            SELECT DISTINCT r.id, r.error_hash, r.resolution_type, r.files_changed,
                   r.commit_hash, r.resolution_time_ms, r.resolved_at, r.notes
            FROM error_resolutions r
            JOIN build_errors e ON r.error_hash = e.error_hash
            WHERE e.category = ?
            ORDER BY r.resolved_at DESC
            LIMIT 5
            "#,
            category_str
        )
        .fetch_all(self.pool.as_ref())
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| {
                let files: Vec<String> =
                    serde_json::from_str(&r.files_changed.unwrap_or_default()).unwrap_or_default();

                ErrorResolution {
                    id: r.id,
                    error_hash: r.error_hash,
                    resolution_type: ResolutionType::from_str(&r.resolution_type),
                    files_changed: files,
                    commit_hash: r.commit_hash,
                    resolution_time_ms: r.resolution_time_ms,
                    resolved_at: Utc.timestamp_opt(r.resolved_at, 0).unwrap(),
                    notes: r.notes,
                }
            })
            .collect())
    }

    /// Link a successful build to resolved errors
    /// Called when a build succeeds after previous failures
    pub async fn link_build_to_resolutions(
        &self,
        project_id: &str,
        commit_hash: Option<&str>,
        files_changed: Vec<String>,
    ) -> Result<i64> {
        // Get unresolved errors for this project
        let unresolved = self.tracker.get_unresolved_errors(project_id, 100).await?;

        if unresolved.is_empty() {
            return Ok(0);
        }

        let mut resolved_count = 0i64;
        let now = Utc::now();

        for error in unresolved {
            // Check if any of the changed files are related to this error
            let is_related = error.file_path.as_ref().map_or(false, |error_file| {
                files_changed.iter().any(|f| {
                    // Check if the changed file could fix this error
                    f.ends_with(error_file) || error_file.ends_with(f)
                })
            });

            if is_related || files_changed.is_empty() {
                // Record as auto-resolved
                let resolution_type = ResolutionType::AutoResolved;
                let files_json = serde_json::to_string(&files_changed)?;
                let resolved_at = now.timestamp();
                let resolution_type_str = resolution_type.as_str();

                sqlx::query!(
                    r#"
                    INSERT INTO error_resolutions (
                        error_hash, resolution_type, files_changed, commit_hash,
                        resolution_time_ms, resolved_at, notes
                    ) VALUES (?, ?, ?, ?, NULL, ?, 'Auto-resolved by successful build')
                    "#,
                    error.error_hash,
                    resolution_type_str,
                    files_json,
                    commit_hash,
                    resolved_at,
                )
                .execute(self.pool.as_ref())
                .await?;

                // Mark error as resolved
                self.tracker
                    .resolve_errors(&[error.error_hash.clone()])
                    .await?;
                resolved_count += 1;
            }
        }

        if resolved_count > 0 {
            info!(
                "Auto-resolved {} errors for project {} (commit: {:?})",
                resolved_count, project_id, commit_hash
            );
        }

        Ok(resolved_count)
    }

    /// Get resolution statistics
    pub async fn get_resolution_stats(&self, project_id: &str) -> Result<ResolutionStats> {
        // Total resolutions
        let total = sqlx::query!(
            r#"
            SELECT COUNT(*) as count FROM error_resolutions r
            JOIN build_errors e ON r.error_hash = e.error_hash
            JOIN build_runs b ON e.build_run_id = b.id
            WHERE b.project_id = ?
            "#,
            project_id
        )
        .fetch_one(self.pool.as_ref())
        .await?
        .count as i64;

        // By type
        let by_type = sqlx::query!(
            r#"
            SELECT r.resolution_type, COUNT(*) as count
            FROM error_resolutions r
            JOIN build_errors e ON r.error_hash = e.error_hash
            JOIN build_runs b ON e.build_run_id = b.id
            WHERE b.project_id = ?
            GROUP BY r.resolution_type
            "#,
            project_id
        )
        .fetch_all(self.pool.as_ref())
        .await?
        .into_iter()
        .map(|r| (ResolutionType::from_str(&r.resolution_type), r.count as i64))
        .collect();

        // Average resolution time
        let avg_time = sqlx::query!(
            r#"
            SELECT AVG(resolution_time_ms) as avg
            FROM error_resolutions r
            JOIN build_errors e ON r.error_hash = e.error_hash
            JOIN build_runs b ON e.build_run_id = b.id
            WHERE b.project_id = ? AND r.resolution_time_ms IS NOT NULL
            "#,
            project_id
        )
        .fetch_one(self.pool.as_ref())
        .await?
        .avg
        .map(|v| v as f64);

        Ok(ResolutionStats {
            project_id: project_id.to_string(),
            total_resolutions: total,
            by_type,
            average_resolution_time_ms: avg_time,
        })
    }

    /// Format errors for context injection into LLM prompts
    pub fn format_errors_for_context(&self, errors: &[BuildError]) -> String {
        if errors.is_empty() {
            return String::new();
        }

        let mut context = String::new();
        context.push_str("## Recent Build Errors\n\n");

        for (i, error) in errors.iter().enumerate().take(5) {
            context.push_str(&format!(
                "### Error {}: {}\n",
                i + 1,
                error.error_code.as_deref().unwrap_or("Unknown")
            ));

            context.push_str(&format!("- **Message**: {}\n", error.message));

            if let Some(ref file) = error.file_path {
                context.push_str(&format!("- **File**: {}", file));
                if let Some(line) = error.line_number {
                    context.push_str(&format!(":{}", line));
                }
                context.push('\n');
            }

            if let Some(ref suggestion) = error.suggestion {
                context.push_str(&format!("- **Suggestion**: {}\n", suggestion));
            }

            context.push_str(&format!(
                "- **Category**: {}\n",
                error.category.as_str()
            ));
            context.push_str(&format!("- **Occurrences**: {}\n", error.occurrence_count));
            context.push('\n');
        }

        context
    }
}

/// Similar resolution with match info
#[derive(Debug, Clone)]
pub struct SimilarResolution {
    pub resolution: ErrorResolution,
    pub similarity_score: f64,
    pub match_reason: String,
}

/// Resolution statistics
#[derive(Debug, Clone)]
pub struct ResolutionStats {
    pub project_id: String,
    pub total_resolutions: i64,
    pub by_type: Vec<(ResolutionType, i64)>,
    pub average_resolution_time_ms: Option<f64>,
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
            CREATE TABLE error_resolutions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                error_hash TEXT NOT NULL,
                resolution_type TEXT NOT NULL,
                files_changed TEXT,
                commit_hash TEXT,
                resolution_time_ms INTEGER,
                resolved_at INTEGER NOT NULL,
                notes TEXT
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
    async fn test_record_and_find_resolution() {
        let pool = Arc::new(create_test_pool().await);
        let tracker = Arc::new(BuildTracker::new(pool.clone()));
        let resolver = ErrorResolver::new(pool, tracker);

        // Record a resolution
        let id = resolver
            .record_resolution(
                "test_hash",
                ResolutionType::CodeChange,
                vec!["src/main.rs".to_string()],
                Some("abc123"),
                Some(5000),
                Some("Fixed the bug"),
            )
            .await
            .unwrap();

        assert!(id > 0);

        // Find the resolution
        let resolutions = resolver.find_resolutions("test_hash").await.unwrap();
        assert_eq!(resolutions.len(), 1);
        assert_eq!(resolutions[0].commit_hash, Some("abc123".to_string()));
    }

    #[tokio::test]
    async fn test_format_errors_for_context() {
        let pool = Arc::new(
            SqlitePoolOptions::new()
                .connect(":memory:")
                .await
                .unwrap()
        );
        let tracker = Arc::new(BuildTracker::new(pool.clone()));
        let resolver = ErrorResolver::new(pool, tracker);

        let errors = vec![BuildError {
            id: Some(1),
            build_run_id: 1,
            error_hash: "test".to_string(),
            severity: ErrorSeverity::Error,
            error_code: Some("E0308".to_string()),
            message: "mismatched types".to_string(),
            file_path: Some("src/main.rs".to_string()),
            line_number: Some(42),
            column_number: None,
            suggestion: Some("try using .into()".to_string()),
            code_snippet: None,
            category: ErrorCategory::Type,
            first_seen_at: Utc::now(),
            last_seen_at: Utc::now(),
            occurrence_count: 3,
            resolved_at: None,
        }];

        let context = resolver.format_errors_for_context(&errors);
        assert!(context.contains("E0308"));
        assert!(context.contains("mismatched types"));
        assert!(context.contains("src/main.rs:42"));
        assert!(context.contains("try using .into()"));
    }
}
