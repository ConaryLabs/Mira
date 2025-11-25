// backend/src/git/intelligence/expertise.rs
// Author expertise scoring based on git history

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

/// Author expertise record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthorExpertise {
    pub id: Option<i64>,
    pub project_id: String,
    pub author_email: String,
    pub author_name: String,
    pub file_pattern: String,
    pub domain: Option<String>,
    pub commit_count: i64,
    pub line_count: i64,
    pub last_contribution: i64,
    pub first_contribution: i64,
    pub expertise_score: f64,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Query for finding experts
#[derive(Debug, Clone, Default)]
pub struct ExpertiseQuery {
    pub project_id: String,
    pub file_pattern: Option<String>,
    pub domain: Option<String>,
    pub min_score: Option<f64>,
    pub limit: Option<i64>,
}

/// Expert recommendation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpertRecommendation {
    pub author_name: String,
    pub author_email: String,
    pub expertise_score: f64,
    pub commit_count: i64,
    pub last_active: i64,
    pub matching_patterns: Vec<String>,
}

/// Expertise statistics for a project
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExpertiseStats {
    pub total_authors: i64,
    pub total_patterns: i64,
    pub avg_expertise_score: f64,
    pub top_contributors: Vec<(String, f64)>,
}

// ============================================================================
// Expertise Service
// ============================================================================

/// Service for managing author expertise
pub struct ExpertiseService {
    pool: SqlitePool,
}

impl ExpertiseService {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    // ========================================================================
    // Expertise Analysis
    // ========================================================================

    /// Analyze git history and compute expertise scores
    pub async fn analyze_project(&self, project_id: &str) -> Result<usize> {
        info!("Analyzing author expertise for project {}", project_id);

        let commit_service = CommitService::new(self.pool.clone());
        let commits = commit_service.get_recent_commits(project_id, 10000).await?;

        if commits.is_empty() {
            debug!("No commits found for expertise analysis");
            return Ok(0);
        }

        // Aggregate contributions by author and file pattern
        #[derive(Default)]
        struct AuthorFileStats {
            commit_count: i64,
            line_count: i64,
            first_contribution: i64,
            last_contribution: i64,
            author_name: String,
        }

        let mut stats: HashMap<(String, String), AuthorFileStats> = HashMap::new();

        for commit in &commits {
            let author_email = commit.author_email.clone();
            let author_name = commit.author_name.clone();

            for file_change in &commit.file_changes {
                // Extract file pattern (directory + extension)
                let pattern = extract_file_pattern(&file_change.path);

                let key = (author_email.clone(), pattern);
                let entry = stats.entry(key).or_default();

                entry.commit_count += 1;
                entry.line_count += file_change.insertions + file_change.deletions;
                entry.author_name = author_name.clone();

                if entry.first_contribution == 0 || commit.authored_at < entry.first_contribution {
                    entry.first_contribution = commit.authored_at;
                }
                if commit.authored_at > entry.last_contribution {
                    entry.last_contribution = commit.authored_at;
                }
            }
        }

        // Compute expertise scores and store
        let now = Utc::now().timestamp();
        let mut records_created = 0;

        // Find max values for normalization
        let max_commits = stats.values().map(|s| s.commit_count).max().unwrap_or(1);
        let max_lines = stats.values().map(|s| s.line_count).max().unwrap_or(1);

        for ((author_email, pattern), author_stats) in stats {
            // Calculate expertise score (0-100)
            // Factors: commit count (40%), line count (30%), recency (30%)
            let commit_score = (author_stats.commit_count as f64 / max_commits as f64) * 40.0;
            let line_score = (author_stats.line_count as f64 / max_lines as f64) * 30.0;

            // Recency: decay over 365 days
            let days_since_last = (now - author_stats.last_contribution) / (24 * 60 * 60);
            let recency_score = ((365.0 - days_since_last.min(365) as f64) / 365.0) * 30.0;

            let expertise_score = commit_score + line_score + recency_score;

            // Infer domain from file pattern
            let domain = infer_domain(&pattern);

            self.upsert_expertise(&AuthorExpertise {
                id: None,
                project_id: project_id.to_string(),
                author_email: author_email.clone(),
                author_name: author_stats.author_name,
                file_pattern: pattern,
                domain,
                commit_count: author_stats.commit_count,
                line_count: author_stats.line_count,
                last_contribution: author_stats.last_contribution,
                first_contribution: author_stats.first_contribution,
                expertise_score,
                created_at: now,
                updated_at: now,
            })
            .await?;

            records_created += 1;
        }

        info!(
            "Created/updated {} expertise records for project {}",
            records_created, project_id
        );

        Ok(records_created)
    }

    /// Insert or update expertise record
    async fn upsert_expertise(&self, expertise: &AuthorExpertise) -> Result<i64> {
        let result = sqlx::query!(
            r#"
            INSERT INTO author_expertise (
                project_id, author_email, author_name, file_pattern, domain,
                commit_count, line_count, last_contribution, first_contribution,
                expertise_score, created_at, updated_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(project_id, author_email, file_pattern) DO UPDATE SET
                author_name = excluded.author_name,
                domain = excluded.domain,
                commit_count = excluded.commit_count,
                line_count = excluded.line_count,
                last_contribution = excluded.last_contribution,
                expertise_score = excluded.expertise_score,
                updated_at = excluded.updated_at
            RETURNING id
            "#,
            expertise.project_id,
            expertise.author_email,
            expertise.author_name,
            expertise.file_pattern,
            expertise.domain,
            expertise.commit_count,
            expertise.line_count,
            expertise.last_contribution,
            expertise.first_contribution,
            expertise.expertise_score,
            expertise.created_at,
            expertise.updated_at
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(result.id)
    }

    // ========================================================================
    // Expert Queries
    // ========================================================================

    /// Find experts for a file
    pub async fn find_experts_for_file(
        &self,
        project_id: &str,
        file_path: &str,
        limit: i64,
    ) -> Result<Vec<ExpertRecommendation>> {
        let pattern = extract_file_pattern(file_path);

        let rows = sqlx::query!(
            r#"
            SELECT author_name, author_email, expertise_score, commit_count,
                   last_contribution, file_pattern
            FROM author_expertise
            WHERE project_id = ? AND file_pattern = ?
            ORDER BY expertise_score DESC
            LIMIT ?
            "#,
            project_id,
            pattern,
            limit
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| ExpertRecommendation {
                author_name: r.author_name,
                author_email: r.author_email,
                expertise_score: r.expertise_score,
                commit_count: r.commit_count,
                last_active: r.last_contribution,
                matching_patterns: vec![r.file_pattern],
            })
            .collect())
    }

    /// Find experts for a domain
    pub async fn find_experts_for_domain(
        &self,
        project_id: &str,
        domain: &str,
        limit: i64,
    ) -> Result<Vec<ExpertRecommendation>> {
        let rows = sqlx::query!(
            r#"
            SELECT author_name, author_email,
                   SUM(expertise_score) as total_score,
                   SUM(commit_count) as total_commits,
                   MAX(last_contribution) as last_active,
                   GROUP_CONCAT(file_pattern) as patterns
            FROM author_expertise
            WHERE project_id = ? AND domain = ?
            GROUP BY author_email
            ORDER BY total_score DESC
            LIMIT ?
            "#,
            project_id,
            domain,
            limit
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .filter_map(|r| {
                let author_name = r.author_name?;
                let author_email = r.author_email?;
                Some(ExpertRecommendation {
                    author_name,
                    author_email,
                    expertise_score: r.total_score,
                    commit_count: r.total_commits,
                    last_active: r.last_active,
                    matching_patterns: r.patterns.split(',').map(String::from).collect(),
                })
            })
            .collect())
    }

    /// Get top experts for a project
    pub async fn get_top_experts(
        &self,
        project_id: &str,
        limit: i64,
    ) -> Result<Vec<ExpertRecommendation>> {
        let rows = sqlx::query!(
            r#"
            SELECT author_name, author_email,
                   SUM(expertise_score) as total_score,
                   SUM(commit_count) as total_commits,
                   MAX(last_contribution) as last_active,
                   GROUP_CONCAT(file_pattern) as patterns
            FROM author_expertise
            WHERE project_id = ?
            GROUP BY author_email
            ORDER BY total_score DESC
            LIMIT ?
            "#,
            project_id,
            limit
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .filter_map(|r| {
                let author_name = r.author_name?;
                let author_email = r.author_email?;
                Some(ExpertRecommendation {
                    author_name,
                    author_email,
                    expertise_score: r.total_score,
                    commit_count: r.total_commits,
                    last_active: r.last_active,
                    matching_patterns: r.patterns.split(',').map(String::from).collect(),
                })
            })
            .collect())
    }

    /// Get expertise for a specific author
    pub async fn get_author_expertise(
        &self,
        project_id: &str,
        author_email: &str,
    ) -> Result<Vec<AuthorExpertise>> {
        let rows = sqlx::query!(
            r#"
            SELECT id, project_id, author_email, author_name, file_pattern, domain,
                   commit_count, line_count, last_contribution, first_contribution,
                   expertise_score, created_at, updated_at
            FROM author_expertise
            WHERE project_id = ? AND author_email = ?
            ORDER BY expertise_score DESC
            "#,
            project_id,
            author_email
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| AuthorExpertise {
                id: r.id,
                project_id: r.project_id,
                author_email: r.author_email,
                author_name: r.author_name,
                file_pattern: r.file_pattern,
                domain: r.domain,
                commit_count: r.commit_count,
                line_count: r.line_count,
                last_contribution: r.last_contribution,
                first_contribution: r.first_contribution,
                expertise_score: r.expertise_score,
                created_at: r.created_at,
                updated_at: r.updated_at,
            })
            .collect())
    }

    /// Get expertise statistics for a project
    pub async fn get_stats(&self, project_id: &str) -> Result<ExpertiseStats> {
        let total_authors: i64 = sqlx::query_scalar!(
            "SELECT COUNT(DISTINCT author_email) as count FROM author_expertise WHERE project_id = ?",
            project_id
        )
        .fetch_one(&self.pool)
        .await? as i64;

        let total_patterns: i64 = sqlx::query_scalar!(
            "SELECT COUNT(DISTINCT file_pattern) as count FROM author_expertise WHERE project_id = ?",
            project_id
        )
        .fetch_one(&self.pool)
        .await? as i64;

        let avg_score: f64 = sqlx::query_scalar!(
            "SELECT COALESCE(AVG(expertise_score), 0.0) as avg FROM author_expertise WHERE project_id = ?",
            project_id
        )
        .fetch_one(&self.pool)
        .await?;

        let top_rows = sqlx::query!(
            r#"
            SELECT author_email, SUM(expertise_score) as total_score
            FROM author_expertise
            WHERE project_id = ?
            GROUP BY author_email
            ORDER BY total_score DESC
            LIMIT 5
            "#,
            project_id
        )
        .fetch_all(&self.pool)
        .await?;

        let top_contributors: Vec<(String, f64)> = top_rows
            .into_iter()
            .map(|r| (r.author_email, r.total_score))
            .collect();

        Ok(ExpertiseStats {
            total_authors,
            total_patterns,
            avg_expertise_score: avg_score,
            top_contributors,
        })
    }

    // ========================================================================
    // Maintenance
    // ========================================================================

    /// Delete expertise data for a project
    pub async fn delete_project_expertise(&self, project_id: &str) -> Result<u64> {
        let result = sqlx::query!(
            "DELETE FROM author_expertise WHERE project_id = ?",
            project_id
        )
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected())
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Extract a pattern from a file path (directory + extension)
fn extract_file_pattern(file_path: &str) -> String {
    let path = std::path::Path::new(file_path);

    // Get parent directory (first level only)
    let dir = path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|s| s.to_str())
        .unwrap_or("root");

    // Get extension
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("no_ext");

    format!("{}/*.{}", dir, ext)
}

/// Infer domain from file pattern
fn infer_domain(pattern: &str) -> Option<String> {
    let pattern_lower = pattern.to_lowercase();

    if pattern_lower.contains("test") || pattern_lower.contains("spec") {
        return Some("testing".to_string());
    }
    if pattern_lower.contains("api") || pattern_lower.contains("route") {
        return Some("api".to_string());
    }
    if pattern_lower.contains("auth") || pattern_lower.contains("user") {
        return Some("authentication".to_string());
    }
    if pattern_lower.contains("db") || pattern_lower.contains("model") || pattern_lower.contains("migration") {
        return Some("database".to_string());
    }
    if pattern_lower.contains("ui") || pattern_lower.contains("component") || pattern_lower.contains("view") {
        return Some("frontend".to_string());
    }
    if pattern_lower.contains("util") || pattern_lower.contains("helper") || pattern_lower.contains("lib") {
        return Some("utilities".to_string());
    }
    if pattern_lower.contains("config") || pattern_lower.contains("setting") {
        return Some("configuration".to_string());
    }
    if pattern_lower.ends_with(".rs") {
        return Some("rust".to_string());
    }
    if pattern_lower.ends_with(".ts") || pattern_lower.ends_with(".tsx") {
        return Some("typescript".to_string());
    }
    if pattern_lower.ends_with(".py") {
        return Some("python".to_string());
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_file_pattern() {
        assert_eq!(extract_file_pattern("src/main.rs"), "src/*.rs");
        assert_eq!(extract_file_pattern("tests/unit/test_foo.py"), "unit/*.py");
        assert_eq!(extract_file_pattern("README.md"), "root/*.md");
    }

    #[test]
    fn test_infer_domain() {
        assert_eq!(infer_domain("tests/*.rs"), Some("testing".to_string()));
        assert_eq!(infer_domain("api/*.ts"), Some("api".to_string()));
        assert_eq!(infer_domain("auth/*.rs"), Some("authentication".to_string()));
        assert_eq!(infer_domain("components/*.tsx"), Some("frontend".to_string()));
        assert_eq!(infer_domain("random/*.rs"), Some("rust".to_string()));
    }

    #[test]
    fn test_expertise_serialization() {
        let expertise = AuthorExpertise {
            id: Some(1),
            project_id: "proj1".to_string(),
            author_email: "test@example.com".to_string(),
            author_name: "Test User".to_string(),
            file_pattern: "src/*.rs".to_string(),
            domain: Some("rust".to_string()),
            commit_count: 50,
            line_count: 2000,
            last_contribution: 1700000000,
            first_contribution: 1690000000,
            expertise_score: 75.5,
            created_at: 1700000000,
            updated_at: 1700000000,
        };

        let json = serde_json::to_string(&expertise).unwrap();
        let deserialized: AuthorExpertise = serde_json::from_str(&json).unwrap();

        assert_eq!(expertise.author_email, deserialized.author_email);
        assert_eq!(expertise.expertise_score, deserialized.expertise_score);
    }
}
