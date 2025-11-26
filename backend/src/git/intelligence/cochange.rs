// backend/src/git/intelligence/cochange.rs
// Co-change pattern detection: files that are frequently modified together

use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::collections::HashMap;
use tracing::{debug, info};

use super::commits::CommitService;

// ============================================================================
// Data Types
// ============================================================================

/// A co-change pattern between two files
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CochangePattern {
    pub id: Option<i64>,
    pub project_id: String,
    pub file_path_a: String,
    pub file_path_b: String,
    pub cochange_count: i64,
    pub total_changes_a: i64,
    pub total_changes_b: i64,
    pub confidence_score: f64,
    pub last_cochange: i64,
    pub embedding_point_id: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// A suggestion based on co-change patterns
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CochangeSuggestion {
    pub file_path: String,
    pub confidence: f64,
    pub cochange_count: i64,
    pub reason: String,
}

/// Configuration for co-change analysis
#[derive(Debug, Clone)]
pub struct CochangeConfig {
    /// Minimum co-changes to create a pattern
    pub min_cochanges: i64,
    /// Minimum confidence score to suggest
    pub min_confidence: f64,
    /// Maximum patterns per file to return
    pub max_patterns_per_file: i64,
}

impl Default for CochangeConfig {
    fn default() -> Self {
        Self {
            min_cochanges: 2,
            min_confidence: 0.3,
            max_patterns_per_file: 10,
        }
    }
}

// ============================================================================
// Co-change Service
// ============================================================================

/// Service for managing co-change patterns
pub struct CochangeService {
    pool: SqlitePool,
    config: CochangeConfig,
}

impl CochangeService {
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            pool,
            config: CochangeConfig::default(),
        }
    }

    pub fn with_config(pool: SqlitePool, config: CochangeConfig) -> Self {
        Self { pool, config }
    }

    // ========================================================================
    // Pattern Analysis
    // ========================================================================

    /// Analyze commits and build co-change patterns for a project
    pub async fn analyze_project(&self, project_id: &str) -> Result<usize> {
        info!("Analyzing co-change patterns for project {}", project_id);

        // Get all commits for the project
        let commit_service = CommitService::new(self.pool.clone());
        let commits = commit_service.get_recent_commits(project_id, 10000).await?;

        if commits.is_empty() {
            debug!("No commits found for project {}", project_id);
            return Ok(0);
        }

        // Track file changes per commit and co-occurrences
        let mut file_change_count: HashMap<String, i64> = HashMap::new();
        let mut cochange_count: HashMap<(String, String), i64> = HashMap::new();
        let mut last_cochange: HashMap<(String, String), i64> = HashMap::new();

        for commit in &commits {
            let files: Vec<String> = commit.file_changes.iter().map(|c| c.path.clone()).collect();

            // Count individual file changes
            for file in &files {
                *file_change_count.entry(file.clone()).or_insert(0) += 1;
            }

            // Count co-changes (pairs of files in same commit)
            for i in 0..files.len() {
                for j in (i + 1)..files.len() {
                    let (a, b) = if files[i] < files[j] {
                        (files[i].clone(), files[j].clone())
                    } else {
                        (files[j].clone(), files[i].clone())
                    };

                    let key = (a, b);
                    *cochange_count.entry(key.clone()).or_insert(0) += 1;
                    last_cochange.insert(key, commit.authored_at);
                }
            }
        }

        // Create patterns for significant co-changes
        let now = Utc::now().timestamp();
        let mut patterns_created = 0;

        for ((file_a, file_b), count) in cochange_count {
            if count < self.config.min_cochanges {
                continue;
            }

            let total_a = *file_change_count.get(&file_a).unwrap_or(&0);
            let total_b = *file_change_count.get(&file_b).unwrap_or(&0);

            // Calculate confidence as Jaccard-like coefficient
            // confidence = cochange_count / (total_a + total_b - cochange_count)
            let union = total_a + total_b - count;
            let confidence = if union > 0 {
                count as f64 / union as f64
            } else {
                0.0
            };

            if confidence < self.config.min_confidence {
                continue;
            }

            let last_time = *last_cochange.get(&(file_a.clone(), file_b.clone())).unwrap_or(&now);

            self.upsert_pattern(&CochangePattern {
                id: None,
                project_id: project_id.to_string(),
                file_path_a: file_a,
                file_path_b: file_b,
                cochange_count: count,
                total_changes_a: total_a,
                total_changes_b: total_b,
                confidence_score: confidence,
                last_cochange: last_time,
                embedding_point_id: None,
                created_at: now,
                updated_at: now,
            })
            .await?;

            patterns_created += 1;
        }

        info!(
            "Created/updated {} co-change patterns for project {}",
            patterns_created, project_id
        );

        Ok(patterns_created)
    }

    /// Insert or update a co-change pattern
    async fn upsert_pattern(&self, pattern: &CochangePattern) -> Result<i64> {
        let result = sqlx::query!(
            r#"
            INSERT INTO file_cochange_patterns (
                project_id, file_path_a, file_path_b, cochange_count,
                total_changes_a, total_changes_b, confidence_score,
                last_cochange, embedding_point_id, created_at, updated_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(project_id, file_path_a, file_path_b) DO UPDATE SET
                cochange_count = excluded.cochange_count,
                total_changes_a = excluded.total_changes_a,
                total_changes_b = excluded.total_changes_b,
                confidence_score = excluded.confidence_score,
                last_cochange = excluded.last_cochange,
                updated_at = excluded.updated_at
            RETURNING id
            "#,
            pattern.project_id,
            pattern.file_path_a,
            pattern.file_path_b,
            pattern.cochange_count,
            pattern.total_changes_a,
            pattern.total_changes_b,
            pattern.confidence_score,
            pattern.last_cochange,
            pattern.embedding_point_id,
            pattern.created_at,
            pattern.updated_at
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(result.id)
    }

    // ========================================================================
    // Pattern Queries
    // ========================================================================

    /// Get co-change suggestions for a file
    pub async fn get_suggestions(&self, project_id: &str, file_path: &str) -> Result<Vec<CochangeSuggestion>> {
        let rows = sqlx::query!(
            r#"
            SELECT
                CASE WHEN file_path_a = ? THEN file_path_b ELSE file_path_a END as "related_file: String",
                cochange_count, confidence_score
            FROM file_cochange_patterns
            WHERE project_id = ? AND (file_path_a = ? OR file_path_b = ?)
            ORDER BY confidence_score DESC, cochange_count DESC
            LIMIT ?
            "#,
            file_path,
            project_id,
            file_path,
            file_path,
            self.config.max_patterns_per_file
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .filter_map(|r| {
                let related_file = r.related_file?;
                Some(CochangeSuggestion {
                    file_path: related_file,
                    confidence: r.confidence_score,
                    cochange_count: r.cochange_count,
                    reason: format!(
                        "Changed together {} times ({:.0}% confidence)",
                        r.cochange_count,
                        r.confidence_score * 100.0
                    ),
                })
            })
            .collect())
    }

    /// Get all patterns for a project
    pub async fn get_patterns(&self, project_id: &str) -> Result<Vec<CochangePattern>> {
        let rows = sqlx::query!(
            r#"
            SELECT id, project_id, file_path_a, file_path_b, cochange_count,
                   total_changes_a, total_changes_b, confidence_score,
                   last_cochange, embedding_point_id, created_at, updated_at
            FROM file_cochange_patterns
            WHERE project_id = ?
            ORDER BY confidence_score DESC
            "#,
            project_id
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| CochangePattern {
                id: r.id,
                project_id: r.project_id,
                file_path_a: r.file_path_a,
                file_path_b: r.file_path_b,
                cochange_count: r.cochange_count,
                total_changes_a: r.total_changes_a,
                total_changes_b: r.total_changes_b,
                confidence_score: r.confidence_score,
                last_cochange: r.last_cochange,
                embedding_point_id: r.embedding_point_id,
                created_at: r.created_at,
                updated_at: r.updated_at,
            })
            .collect())
    }

    /// Get high-confidence patterns above a threshold
    pub async fn get_high_confidence_patterns(
        &self,
        project_id: &str,
        min_confidence: f64,
    ) -> Result<Vec<CochangePattern>> {
        let rows = sqlx::query!(
            r#"
            SELECT id, project_id, file_path_a, file_path_b, cochange_count,
                   total_changes_a, total_changes_b, confidence_score,
                   last_cochange, embedding_point_id, created_at, updated_at
            FROM file_cochange_patterns
            WHERE project_id = ? AND confidence_score >= ?
            ORDER BY confidence_score DESC
            "#,
            project_id,
            min_confidence
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| CochangePattern {
                id: r.id,
                project_id: r.project_id,
                file_path_a: r.file_path_a,
                file_path_b: r.file_path_b,
                cochange_count: r.cochange_count,
                total_changes_a: r.total_changes_a,
                total_changes_b: r.total_changes_b,
                confidence_score: r.confidence_score,
                last_cochange: r.last_cochange,
                embedding_point_id: r.embedding_point_id,
                created_at: r.created_at,
                updated_at: r.updated_at,
            })
            .collect())
    }

    /// Get patterns involving a specific file
    pub async fn get_patterns_for_file(&self, project_id: &str, file_path: &str) -> Result<Vec<CochangePattern>> {
        let rows = sqlx::query!(
            r#"
            SELECT id, project_id, file_path_a, file_path_b, cochange_count,
                   total_changes_a, total_changes_b, confidence_score,
                   last_cochange, embedding_point_id, created_at, updated_at
            FROM file_cochange_patterns
            WHERE project_id = ? AND (file_path_a = ? OR file_path_b = ?)
            ORDER BY confidence_score DESC
            "#,
            project_id,
            file_path,
            file_path
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| CochangePattern {
                id: r.id,
                project_id: r.project_id,
                file_path_a: r.file_path_a,
                file_path_b: r.file_path_b,
                cochange_count: r.cochange_count,
                total_changes_a: r.total_changes_a,
                total_changes_b: r.total_changes_b,
                confidence_score: r.confidence_score,
                last_cochange: r.last_cochange,
                embedding_point_id: r.embedding_point_id,
                created_at: r.created_at,
                updated_at: r.updated_at,
            })
            .collect())
    }

    // ========================================================================
    // Maintenance
    // ========================================================================

    /// Delete patterns for a project
    pub async fn delete_project_patterns(&self, project_id: &str) -> Result<u64> {
        let result = sqlx::query!(
            "DELETE FROM file_cochange_patterns WHERE project_id = ?",
            project_id
        )
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected())
    }

    /// Delete stale patterns (not updated recently)
    pub async fn delete_stale_patterns(&self, project_id: &str, max_age_days: i64) -> Result<u64> {
        let cutoff = Utc::now().timestamp() - (max_age_days * 24 * 60 * 60);

        let result = sqlx::query!(
            "DELETE FROM file_cochange_patterns WHERE project_id = ? AND updated_at < ?",
            project_id,
            cutoff
        )
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_confidence_calculation() {
        // If file A changed 10 times, file B changed 8 times, and they co-changed 6 times
        // Union = 10 + 8 - 6 = 12
        // Confidence = 6 / 12 = 0.5
        let cochange_count = 6;
        let total_a = 10;
        let total_b = 8;
        let union = total_a + total_b - cochange_count;
        let confidence = cochange_count as f64 / union as f64;
        assert!((confidence - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_suggestion_serialization() {
        let suggestion = CochangeSuggestion {
            file_path: "src/main.rs".to_string(),
            confidence: 0.75,
            cochange_count: 15,
            reason: "Changed together 15 times".to_string(),
        };

        let json = serde_json::to_string(&suggestion).unwrap();
        let deserialized: CochangeSuggestion = serde_json::from_str(&json).unwrap();

        assert_eq!(suggestion.file_path, deserialized.file_path);
        assert_eq!(suggestion.confidence, deserialized.confidence);
    }
}
