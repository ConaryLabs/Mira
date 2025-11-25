// backend/src/git/intelligence/blame.rs
// Git blame annotation management

use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use tracing::debug;

// ============================================================================
// Data Types
// ============================================================================

/// A blame annotation for a single line
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlameAnnotation {
    pub id: Option<i64>,
    pub project_id: String,
    pub file_path: String,
    pub line_number: i64,
    pub commit_hash: String,
    pub author_name: String,
    pub author_email: String,
    pub authored_at: i64,
    pub line_content: String,
    pub file_hash: String,
    pub created_at: i64,
}

/// Blame summary for a file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlameFileSummary {
    pub file_path: String,
    pub total_lines: i64,
    pub authors: Vec<BlameAuthorStats>,
    pub oldest_line: Option<i64>,
    pub newest_line: Option<i64>,
}

/// Author statistics from blame
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlameAuthorStats {
    pub author_name: String,
    pub author_email: String,
    pub line_count: i64,
    pub percentage: f64,
    pub first_contribution: i64,
    pub last_contribution: i64,
}

/// Range of blame annotations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlameRange {
    pub start_line: i64,
    pub end_line: i64,
    pub commit_hash: String,
    pub author_name: String,
    pub author_email: String,
    pub authored_at: i64,
}

// ============================================================================
// Blame Service
// ============================================================================

/// Service for managing blame annotations
pub struct BlameService {
    pool: SqlitePool,
}

impl BlameService {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Compute hash for file content (used for cache invalidation)
    pub fn compute_file_hash(content: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(content.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    // ========================================================================
    // Annotation Storage
    // ========================================================================

    /// Store blame annotations for a file
    pub async fn store_annotations(&self, annotations: &[BlameAnnotation]) -> Result<usize> {
        if annotations.is_empty() {
            return Ok(0);
        }

        let now = Utc::now().timestamp();
        let mut count = 0;

        for annotation in annotations {
            sqlx::query!(
                r#"
                INSERT INTO blame_annotations (
                    project_id, file_path, line_number, commit_hash,
                    author_name, author_email, authored_at, line_content,
                    file_hash, created_at
                )
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                ON CONFLICT(project_id, file_path, line_number, file_hash) DO UPDATE SET
                    commit_hash = excluded.commit_hash,
                    author_name = excluded.author_name,
                    author_email = excluded.author_email,
                    authored_at = excluded.authored_at,
                    line_content = excluded.line_content
                "#,
                annotation.project_id,
                annotation.file_path,
                annotation.line_number,
                annotation.commit_hash,
                annotation.author_name,
                annotation.author_email,
                annotation.authored_at,
                annotation.line_content,
                annotation.file_hash,
                now
            )
            .execute(&self.pool)
            .await?;

            count += 1;
        }

        debug!("Stored {} blame annotations", count);
        Ok(count)
    }

    /// Get blame for a file
    pub async fn get_file_blame(
        &self,
        project_id: &str,
        file_path: &str,
        file_hash: Option<&str>,
    ) -> Result<Vec<BlameAnnotation>> {
        if let Some(hash) = file_hash {
            self.get_file_blame_by_hash(project_id, file_path, hash).await
        } else {
            self.get_file_blame_latest(project_id, file_path).await
        }
    }

    async fn get_file_blame_by_hash(
        &self,
        project_id: &str,
        file_path: &str,
        hash: &str,
    ) -> Result<Vec<BlameAnnotation>> {
        let rows = sqlx::query!(
            r#"
            SELECT id, project_id, file_path, line_number, commit_hash,
                   author_name, author_email, authored_at, line_content,
                   file_hash, created_at
            FROM blame_annotations
            WHERE project_id = ? AND file_path = ? AND file_hash = ?
            ORDER BY line_number
            "#,
            project_id,
            file_path,
            hash
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| BlameAnnotation {
                id: r.id,
                project_id: r.project_id,
                file_path: r.file_path,
                line_number: r.line_number,
                commit_hash: r.commit_hash,
                author_name: r.author_name,
                author_email: r.author_email,
                authored_at: r.authored_at,
                line_content: r.line_content,
                file_hash: r.file_hash,
                created_at: r.created_at,
            })
            .collect())
    }

    async fn get_file_blame_latest(
        &self,
        project_id: &str,
        file_path: &str,
    ) -> Result<Vec<BlameAnnotation>> {
        let rows = sqlx::query!(
            r#"
            SELECT id, project_id, file_path, line_number, commit_hash,
                   author_name, author_email, authored_at, line_content,
                   file_hash, created_at
            FROM blame_annotations
            WHERE project_id = ? AND file_path = ?
              AND created_at = (
                  SELECT MAX(created_at) FROM blame_annotations b2
                  WHERE b2.project_id = blame_annotations.project_id
                    AND b2.file_path = blame_annotations.file_path
                    AND b2.line_number = blame_annotations.line_number
              )
            ORDER BY line_number
            "#,
            project_id,
            file_path
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| BlameAnnotation {
                id: r.id,
                project_id: r.project_id,
                file_path: r.file_path,
                line_number: r.line_number,
                commit_hash: r.commit_hash,
                author_name: r.author_name,
                author_email: r.author_email,
                authored_at: r.authored_at,
                line_content: r.line_content,
                file_hash: r.file_hash,
                created_at: r.created_at,
            })
            .collect())
    }

    /// Get blame for a specific line range
    pub async fn get_line_range_blame(
        &self,
        project_id: &str,
        file_path: &str,
        start_line: i64,
        end_line: i64,
    ) -> Result<Vec<BlameAnnotation>> {
        let rows = sqlx::query!(
            r#"
            SELECT id, project_id, file_path, line_number, commit_hash,
                   author_name, author_email, authored_at, line_content,
                   file_hash, created_at
            FROM blame_annotations
            WHERE project_id = ? AND file_path = ?
              AND line_number >= ? AND line_number <= ?
            ORDER BY line_number
            "#,
            project_id,
            file_path,
            start_line,
            end_line
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| BlameAnnotation {
                id: r.id,
                project_id: r.project_id,
                file_path: r.file_path,
                line_number: r.line_number,
                commit_hash: r.commit_hash,
                author_name: r.author_name,
                author_email: r.author_email,
                authored_at: r.authored_at,
                line_content: r.line_content,
                file_hash: r.file_hash,
                created_at: r.created_at,
            })
            .collect())
    }

    // ========================================================================
    // Analysis
    // ========================================================================

    /// Get blame summary for a file
    pub async fn get_file_summary(
        &self,
        project_id: &str,
        file_path: &str,
    ) -> Result<BlameFileSummary> {
        // Get author stats
        let author_rows = sqlx::query!(
            r#"
            SELECT
                author_name, author_email,
                COUNT(*) as line_count,
                MIN(authored_at) as first_contribution,
                MAX(authored_at) as last_contribution
            FROM blame_annotations
            WHERE project_id = ? AND file_path = ?
            GROUP BY author_email
            ORDER BY line_count DESC
            "#,
            project_id,
            file_path
        )
        .fetch_all(&self.pool)
        .await?;

        let total_lines: i64 = author_rows.iter().map(|r| r.line_count as i64).sum();

        let authors: Vec<BlameAuthorStats> = author_rows
            .into_iter()
            .filter_map(|r| {
                let author_name = r.author_name?;
                let author_email = r.author_email?;
                Some(BlameAuthorStats {
                    author_name,
                    author_email,
                    line_count: r.line_count as i64,
                    percentage: if total_lines > 0 {
                        (r.line_count as f64 / total_lines as f64) * 100.0
                    } else {
                        0.0
                    },
                    first_contribution: r.first_contribution,
                    last_contribution: r.last_contribution,
                })
            })
            .collect();

        // Get oldest and newest lines
        let time_range = sqlx::query!(
            r#"
            SELECT MIN(authored_at) as oldest, MAX(authored_at) as newest
            FROM blame_annotations
            WHERE project_id = ? AND file_path = ?
            "#,
            project_id,
            file_path
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(BlameFileSummary {
            file_path: file_path.to_string(),
            total_lines,
            authors,
            oldest_line: time_range.oldest,
            newest_line: time_range.newest,
        })
    }

    /// Get contiguous blame ranges (group consecutive lines by same author/commit)
    pub async fn get_blame_ranges(
        &self,
        project_id: &str,
        file_path: &str,
    ) -> Result<Vec<BlameRange>> {
        let annotations = self.get_file_blame(project_id, file_path, None).await?;

        if annotations.is_empty() {
            return Ok(Vec::new());
        }

        let mut ranges: Vec<BlameRange> = Vec::new();
        let mut current_range: Option<BlameRange> = None;

        for annotation in annotations {
            match &mut current_range {
                Some(range)
                    if range.commit_hash == annotation.commit_hash
                        && range.end_line + 1 == annotation.line_number =>
                {
                    // Extend current range
                    range.end_line = annotation.line_number;
                }
                _ => {
                    // Start new range
                    if let Some(range) = current_range.take() {
                        ranges.push(range);
                    }
                    current_range = Some(BlameRange {
                        start_line: annotation.line_number,
                        end_line: annotation.line_number,
                        commit_hash: annotation.commit_hash,
                        author_name: annotation.author_name,
                        author_email: annotation.author_email,
                        authored_at: annotation.authored_at,
                    });
                }
            }
        }

        if let Some(range) = current_range {
            ranges.push(range);
        }

        Ok(ranges)
    }

    /// Find who last modified a specific line
    pub async fn get_line_author(
        &self,
        project_id: &str,
        file_path: &str,
        line_number: i64,
    ) -> Result<Option<BlameAnnotation>> {
        let row = sqlx::query!(
            r#"
            SELECT id, project_id, file_path, line_number, commit_hash,
                   author_name, author_email, authored_at, line_content,
                   file_hash, created_at
            FROM blame_annotations
            WHERE project_id = ? AND file_path = ? AND line_number = ?
            ORDER BY created_at DESC
            LIMIT 1
            "#,
            project_id,
            file_path,
            line_number
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| BlameAnnotation {
            id: r.id,
            project_id: r.project_id,
            file_path: r.file_path,
            line_number: r.line_number,
            commit_hash: r.commit_hash,
            author_name: r.author_name,
            author_email: r.author_email,
            authored_at: r.authored_at,
            line_content: r.line_content,
            file_hash: r.file_hash,
            created_at: r.created_at,
        }))
    }

    // ========================================================================
    // Maintenance
    // ========================================================================

    /// Check if blame is current for a file
    pub async fn is_blame_current(
        &self,
        project_id: &str,
        file_path: &str,
        file_hash: &str,
    ) -> Result<bool> {
        let count: i64 = sqlx::query_scalar!(
            r#"
            SELECT COUNT(*) as count FROM blame_annotations
            WHERE project_id = ? AND file_path = ? AND file_hash = ?
            "#,
            project_id,
            file_path,
            file_hash
        )
        .fetch_one(&self.pool)
        .await? as i64;

        Ok(count > 0)
    }

    /// Delete blame for a file
    pub async fn delete_file_blame(&self, project_id: &str, file_path: &str) -> Result<u64> {
        let result = sqlx::query!(
            "DELETE FROM blame_annotations WHERE project_id = ? AND file_path = ?",
            project_id,
            file_path
        )
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected())
    }

    /// Delete all blame for a project
    pub async fn delete_project_blame(&self, project_id: &str) -> Result<u64> {
        let result = sqlx::query!(
            "DELETE FROM blame_annotations WHERE project_id = ?",
            project_id
        )
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected())
    }

    /// Delete old blame data (keep only latest file_hash per file)
    pub async fn cleanup_old_blame(&self, project_id: &str) -> Result<u64> {
        let result = sqlx::query!(
            r#"
            DELETE FROM blame_annotations
            WHERE project_id = ?
              AND (file_path, file_hash) NOT IN (
                  SELECT file_path, file_hash
                  FROM blame_annotations b2
                  WHERE b2.project_id = ?
                  GROUP BY file_path
                  HAVING created_at = MAX(created_at)
              )
            "#,
            project_id,
            project_id
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
    fn test_file_hash() {
        let content = "fn main() {\n    println!(\"Hello\");\n}";
        let hash1 = BlameService::compute_file_hash(content);
        let hash2 = BlameService::compute_file_hash(content);
        let hash3 = BlameService::compute_file_hash("different content");

        assert_eq!(hash1, hash2);
        assert_ne!(hash1, hash3);
        assert_eq!(hash1.len(), 64); // SHA-256 hex
    }

    #[test]
    fn test_annotation_serialization() {
        let annotation = BlameAnnotation {
            id: Some(1),
            project_id: "proj1".to_string(),
            file_path: "src/main.rs".to_string(),
            line_number: 42,
            commit_hash: "abc123".to_string(),
            author_name: "Test User".to_string(),
            author_email: "test@example.com".to_string(),
            authored_at: 1700000000,
            line_content: "let x = 42;".to_string(),
            file_hash: "def456".to_string(),
            created_at: 1700000000,
        };

        let json = serde_json::to_string(&annotation).unwrap();
        let deserialized: BlameAnnotation = serde_json::from_str(&json).unwrap();

        assert_eq!(annotation.line_number, deserialized.line_number);
        assert_eq!(annotation.commit_hash, deserialized.commit_hash);
    }
}
