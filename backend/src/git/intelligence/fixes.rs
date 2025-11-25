// backend/src/git/intelligence/fixes.rs
// Historical fix matching: Learn from past bug fixes to suggest solutions

use anyhow::Result;
use chrono::Utc;
use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use tracing::debug;

// ============================================================================
// Data Types
// ============================================================================

/// A historical fix record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoricalFix {
    pub id: Option<i64>,
    pub project_id: String,
    pub error_pattern: String,
    pub error_category: String,
    pub fix_commit_hash: String,
    pub files_modified: Vec<String>,
    pub fix_description: Option<String>,
    pub fixed_at: i64,
    pub similarity_hash: String,
    pub embedding_point_id: Option<String>,
    pub created_at: i64,
}

/// A potential fix match for a current error
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FixMatch {
    pub fix: HistoricalFix,
    pub similarity_score: f64,
    pub match_reason: String,
}

/// Error categories for classification
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ErrorCategory {
    CompileError,
    RuntimeError,
    TypeMismatch,
    NullReference,
    BoundsError,
    ImportError,
    SyntaxError,
    LogicError,
    ConfigError,
    DependencyError,
    TestFailure,
    Unknown,
}

impl ErrorCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::CompileError => "compile_error",
            Self::RuntimeError => "runtime_error",
            Self::TypeMismatch => "type_mismatch",
            Self::NullReference => "null_reference",
            Self::BoundsError => "bounds_error",
            Self::ImportError => "import_error",
            Self::SyntaxError => "syntax_error",
            Self::LogicError => "logic_error",
            Self::ConfigError => "config_error",
            Self::DependencyError => "dependency_error",
            Self::TestFailure => "test_failure",
            Self::Unknown => "unknown",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "compile_error" => Self::CompileError,
            "runtime_error" => Self::RuntimeError,
            "type_mismatch" => Self::TypeMismatch,
            "null_reference" => Self::NullReference,
            "bounds_error" => Self::BoundsError,
            "import_error" => Self::ImportError,
            "syntax_error" => Self::SyntaxError,
            "logic_error" => Self::LogicError,
            "config_error" => Self::ConfigError,
            "dependency_error" => Self::DependencyError,
            "test_failure" => Self::TestFailure,
            _ => Self::Unknown,
        }
    }
}

/// Configuration for fix matching
#[derive(Debug, Clone)]
pub struct FixMatchConfig {
    /// Minimum similarity score to return a match
    pub min_similarity: f64,
    /// Maximum number of matches to return
    pub max_matches: i64,
    /// Boost score for same category matches
    pub category_boost: f64,
    /// Boost score for same file matches
    pub file_boost: f64,
}

impl Default for FixMatchConfig {
    fn default() -> Self {
        Self {
            min_similarity: 0.3,
            max_matches: 5,
            category_boost: 0.2,
            file_boost: 0.15,
        }
    }
}

/// Statistics about historical fixes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FixStats {
    pub total_fixes: i64,
    pub by_category: Vec<(String, i64)>,
    pub most_fixed_files: Vec<(String, i64)>,
    pub recent_fixes_count: i64,
}

// ============================================================================
// Fix Service
// ============================================================================

/// Service for managing historical fixes
pub struct FixService {
    pool: SqlitePool,
    config: FixMatchConfig,
}

impl FixService {
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            pool,
            config: FixMatchConfig::default(),
        }
    }

    pub fn with_config(pool: SqlitePool, config: FixMatchConfig) -> Self {
        Self { pool, config }
    }

    /// Compute similarity hash for error pattern
    /// This allows quick lookups of potentially similar errors
    pub fn compute_similarity_hash(error_pattern: &str, category: ErrorCategory) -> String {
        // Normalize the error pattern for hashing
        let normalized = Self::normalize_error_pattern(error_pattern);

        let mut hasher = Sha256::new();
        hasher.update(category.as_str().as_bytes());
        hasher.update(b":");
        hasher.update(normalized.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    /// Normalize an error pattern by removing variable parts
    fn normalize_error_pattern(pattern: &str) -> String {
        // Remove line numbers, file paths, and variable identifiers
        let mut normalized = pattern.to_lowercase();

        // Remove quoted strings (often variable content)
        normalized = Regex::new(r#""[^"]*""#)
            .map(|re| re.replace_all(&normalized, "\"_\"").to_string())
            .unwrap_or(normalized);

        // Remove single-quoted strings
        normalized = Regex::new(r"'[^']*'")
            .map(|re| re.replace_all(&normalized, "'_'").to_string())
            .unwrap_or(normalized);

        // Remove numbers (line numbers, counts, etc.)
        normalized = Regex::new(r"\b\d+\b")
            .map(|re| re.replace_all(&normalized, "_").to_string())
            .unwrap_or(normalized);

        // Remove file paths
        normalized = Regex::new(r"(/[^\s]+)+")
            .map(|re| re.replace_all(&normalized, "_path_").to_string())
            .unwrap_or(normalized);

        // Collapse whitespace
        normalized = Regex::new(r"\s+")
            .map(|re| re.replace_all(&normalized, " ").to_string())
            .unwrap_or(normalized);

        normalized.trim().to_string()
    }

    /// Classify an error into a category based on keywords
    pub fn classify_error(error_text: &str) -> ErrorCategory {
        let lower = error_text.to_lowercase();

        if lower.contains("cannot find") || lower.contains("not found") || lower.contains("unresolved") {
            return ErrorCategory::ImportError;
        }
        if lower.contains("type mismatch") || lower.contains("expected type") || lower.contains("incompatible types") {
            return ErrorCategory::TypeMismatch;
        }
        if lower.contains("null") || lower.contains("none") || lower.contains("undefined") {
            return ErrorCategory::NullReference;
        }
        if lower.contains("index out of") || lower.contains("bounds") || lower.contains("overflow") {
            return ErrorCategory::BoundsError;
        }
        if lower.contains("syntax") || lower.contains("parse error") || lower.contains("unexpected token") {
            return ErrorCategory::SyntaxError;
        }
        if lower.contains("runtime") || lower.contains("panic") || lower.contains("exception") {
            return ErrorCategory::RuntimeError;
        }
        if lower.contains("compile") || lower.contains("build failed") || lower.contains("cannot compile") {
            return ErrorCategory::CompileError;
        }
        if lower.contains("config") || lower.contains("configuration") || lower.contains("env") {
            return ErrorCategory::ConfigError;
        }
        if lower.contains("dependency") || lower.contains("package") || lower.contains("version") {
            return ErrorCategory::DependencyError;
        }
        if lower.contains("test failed") || lower.contains("assertion") || lower.contains("expect") {
            return ErrorCategory::TestFailure;
        }

        ErrorCategory::Unknown
    }

    // ========================================================================
    // Fix Recording
    // ========================================================================

    /// Record a historical fix
    pub async fn record_fix(&self, fix: &HistoricalFix) -> Result<i64> {
        let now = Utc::now().timestamp();
        let files_json = serde_json::to_string(&fix.files_modified)?;

        let result = sqlx::query!(
            r#"
            INSERT INTO historical_fixes (
                project_id, error_pattern, error_category, fix_commit_hash,
                files_modified, fix_description, fixed_at, similarity_hash,
                embedding_point_id, created_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT DO NOTHING
            RETURNING id
            "#,
            fix.project_id,
            fix.error_pattern,
            fix.error_category,
            fix.fix_commit_hash,
            files_json,
            fix.fix_description,
            fix.fixed_at,
            fix.similarity_hash,
            fix.embedding_point_id,
            now
        )
        .fetch_optional(&self.pool)
        .await?;

        if let Some(row) = result {
            debug!("Recorded historical fix with id {}", row.id);
            Ok(row.id)
        } else {
            // Fix already exists
            Ok(0)
        }
    }

    /// Extract and record a fix from a commit message
    /// Returns true if a fix was detected and recorded
    pub async fn extract_fix_from_commit(
        &self,
        project_id: &str,
        commit_hash: &str,
        commit_message: &str,
        files_modified: &[String],
        committed_at: i64,
    ) -> Result<bool> {
        // Check if this looks like a fix commit
        let lower_msg = commit_message.to_lowercase();
        let is_fix = lower_msg.contains("fix")
            || lower_msg.contains("bug")
            || lower_msg.contains("resolve")
            || lower_msg.contains("patch")
            || lower_msg.contains("repair")
            || lower_msg.contains("correct");

        if !is_fix {
            return Ok(false);
        }

        // Try to extract the error pattern from the commit message
        // Look for patterns like "fix: error message" or "fixes #123: description"
        let error_pattern = Self::extract_error_pattern(commit_message);
        let category = Self::classify_error(&error_pattern);
        let similarity_hash = Self::compute_similarity_hash(&error_pattern, category);

        let fix = HistoricalFix {
            id: None,
            project_id: project_id.to_string(),
            error_pattern,
            error_category: category.as_str().to_string(),
            fix_commit_hash: commit_hash.to_string(),
            files_modified: files_modified.to_vec(),
            fix_description: Some(commit_message.to_string()),
            fixed_at: committed_at,
            similarity_hash,
            embedding_point_id: None,
            created_at: Utc::now().timestamp(),
        };

        self.record_fix(&fix).await?;
        Ok(true)
    }

    /// Extract error pattern from commit message
    fn extract_error_pattern(message: &str) -> String {
        // Try common patterns
        let lines: Vec<&str> = message.lines().collect();

        // First line often contains the fix description
        let first_line = lines.first().unwrap_or(&"");

        // Remove common prefixes
        let pattern = first_line
            .trim_start_matches("fix:")
            .trim_start_matches("Fix:")
            .trim_start_matches("FIX:")
            .trim_start_matches("fix(")
            .trim_start_matches("Fix(")
            .trim_start_matches("bug:")
            .trim_start_matches("Bug:")
            .trim();

        // If there's a closing paren from conventional commits, find the actual message
        if let Some(paren_pos) = pattern.find("):") {
            return pattern[paren_pos + 2..].trim().to_string();
        }

        pattern.to_string()
    }

    // ========================================================================
    // Fix Matching
    // ========================================================================

    /// Find similar fixes for a given error
    pub async fn find_similar_fixes(
        &self,
        project_id: &str,
        error_pattern: &str,
        affected_files: Option<&[String]>,
    ) -> Result<Vec<FixMatch>> {
        let category = Self::classify_error(error_pattern);
        let similarity_hash = Self::compute_similarity_hash(error_pattern, category);

        // First, try exact similarity hash match
        let exact_matches = self.find_by_similarity_hash(project_id, &similarity_hash).await?;

        if !exact_matches.is_empty() {
            return Ok(exact_matches
                .into_iter()
                .map(|fix| FixMatch {
                    fix,
                    similarity_score: 1.0,
                    match_reason: "Exact error pattern match".to_string(),
                })
                .take(self.config.max_matches as usize)
                .collect());
        }

        // Try category-based matching with scoring
        let category_matches = self.find_by_category(project_id, category).await?;

        let mut scored_matches: Vec<FixMatch> = category_matches
            .into_iter()
            .map(|fix| {
                let mut score = self.config.category_boost;
                let mut reasons = vec!["Same error category"];

                // Boost if files overlap
                if let Some(files) = affected_files {
                    let overlap = files.iter()
                        .filter(|f| fix.files_modified.contains(f))
                        .count();
                    if overlap > 0 {
                        score += self.config.file_boost * (overlap as f64 / files.len() as f64);
                        reasons.push("Modified same files");
                    }
                }

                // Boost for pattern similarity (simple word overlap)
                let pattern_similarity = Self::compute_pattern_similarity(error_pattern, &fix.error_pattern);
                score += pattern_similarity * (1.0 - self.config.category_boost - self.config.file_boost);

                if pattern_similarity > 0.5 {
                    reasons.push("Similar error pattern");
                }

                FixMatch {
                    fix,
                    similarity_score: score.min(1.0),
                    match_reason: reasons.join(", "),
                }
            })
            .filter(|m| m.similarity_score >= self.config.min_similarity)
            .collect();

        // Sort by score descending
        scored_matches.sort_by(|a, b| b.similarity_score.partial_cmp(&a.similarity_score).unwrap());

        Ok(scored_matches
            .into_iter()
            .take(self.config.max_matches as usize)
            .collect())
    }

    /// Compute simple word-based pattern similarity
    fn compute_pattern_similarity(pattern1: &str, pattern2: &str) -> f64 {
        let words1: std::collections::HashSet<&str> = pattern1
            .split_whitespace()
            .filter(|w| w.len() > 2)
            .collect();
        let words2: std::collections::HashSet<&str> = pattern2
            .split_whitespace()
            .filter(|w| w.len() > 2)
            .collect();

        if words1.is_empty() || words2.is_empty() {
            return 0.0;
        }

        let intersection = words1.intersection(&words2).count();
        let union = words1.union(&words2).count();

        if union == 0 {
            0.0
        } else {
            intersection as f64 / union as f64
        }
    }

    /// Find fixes by exact similarity hash
    async fn find_by_similarity_hash(&self, project_id: &str, hash: &str) -> Result<Vec<HistoricalFix>> {
        let rows = sqlx::query!(
            r#"
            SELECT id, project_id, error_pattern, error_category, fix_commit_hash,
                   files_modified, fix_description, fixed_at, similarity_hash,
                   embedding_point_id, created_at
            FROM historical_fixes
            WHERE project_id = ? AND similarity_hash = ?
            ORDER BY fixed_at DESC
            "#,
            project_id,
            hash
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| HistoricalFix {
                id: r.id,
                project_id: r.project_id,
                error_pattern: r.error_pattern,
                error_category: r.error_category,
                fix_commit_hash: r.fix_commit_hash,
                files_modified: serde_json::from_str(&r.files_modified).unwrap_or_default(),
                fix_description: r.fix_description,
                fixed_at: r.fixed_at,
                similarity_hash: r.similarity_hash,
                embedding_point_id: r.embedding_point_id,
                created_at: r.created_at,
            })
            .collect())
    }

    /// Find fixes by category
    async fn find_by_category(&self, project_id: &str, category: ErrorCategory) -> Result<Vec<HistoricalFix>> {
        let category_str = category.as_str();
        let rows = sqlx::query!(
            r#"
            SELECT id, project_id, error_pattern, error_category, fix_commit_hash,
                   files_modified, fix_description, fixed_at, similarity_hash,
                   embedding_point_id, created_at
            FROM historical_fixes
            WHERE project_id = ? AND error_category = ?
            ORDER BY fixed_at DESC
            LIMIT 50
            "#,
            project_id,
            category_str
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| HistoricalFix {
                id: r.id,
                project_id: r.project_id,
                error_pattern: r.error_pattern,
                error_category: r.error_category,
                fix_commit_hash: r.fix_commit_hash,
                files_modified: serde_json::from_str(&r.files_modified).unwrap_or_default(),
                fix_description: r.fix_description,
                fixed_at: r.fixed_at,
                similarity_hash: r.similarity_hash,
                embedding_point_id: r.embedding_point_id,
                created_at: r.created_at,
            })
            .collect())
    }

    // ========================================================================
    // Queries
    // ========================================================================

    /// Get a fix by ID
    pub async fn get_fix(&self, id: i64) -> Result<Option<HistoricalFix>> {
        let row = sqlx::query!(
            r#"
            SELECT id, project_id, error_pattern, error_category, fix_commit_hash,
                   files_modified, fix_description, fixed_at, similarity_hash,
                   embedding_point_id, created_at
            FROM historical_fixes
            WHERE id = ?
            "#,
            id
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| HistoricalFix {
            id: Some(r.id),
            project_id: r.project_id,
            error_pattern: r.error_pattern,
            error_category: r.error_category,
            fix_commit_hash: r.fix_commit_hash,
            files_modified: serde_json::from_str(&r.files_modified).unwrap_or_default(),
            fix_description: r.fix_description,
            fixed_at: r.fixed_at,
            similarity_hash: r.similarity_hash,
            embedding_point_id: r.embedding_point_id,
            created_at: r.created_at,
        }))
    }

    /// Get fix by commit hash
    pub async fn get_fix_by_commit(&self, project_id: &str, commit_hash: &str) -> Result<Option<HistoricalFix>> {
        let row = sqlx::query!(
            r#"
            SELECT id, project_id, error_pattern, error_category, fix_commit_hash,
                   files_modified, fix_description, fixed_at, similarity_hash,
                   embedding_point_id, created_at
            FROM historical_fixes
            WHERE project_id = ? AND fix_commit_hash = ?
            "#,
            project_id,
            commit_hash
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| HistoricalFix {
            id: r.id,
            project_id: r.project_id,
            error_pattern: r.error_pattern,
            error_category: r.error_category,
            fix_commit_hash: r.fix_commit_hash,
            files_modified: serde_json::from_str(&r.files_modified).unwrap_or_default(),
            fix_description: r.fix_description,
            fixed_at: r.fixed_at,
            similarity_hash: r.similarity_hash,
            embedding_point_id: r.embedding_point_id,
            created_at: r.created_at,
        }))
    }

    /// Get all fixes for a project
    pub async fn get_project_fixes(&self, project_id: &str) -> Result<Vec<HistoricalFix>> {
        let rows = sqlx::query!(
            r#"
            SELECT id, project_id, error_pattern, error_category, fix_commit_hash,
                   files_modified, fix_description, fixed_at, similarity_hash,
                   embedding_point_id, created_at
            FROM historical_fixes
            WHERE project_id = ?
            ORDER BY fixed_at DESC
            "#,
            project_id
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| HistoricalFix {
                id: r.id,
                project_id: r.project_id,
                error_pattern: r.error_pattern,
                error_category: r.error_category,
                fix_commit_hash: r.fix_commit_hash,
                files_modified: serde_json::from_str(&r.files_modified).unwrap_or_default(),
                fix_description: r.fix_description,
                fixed_at: r.fixed_at,
                similarity_hash: r.similarity_hash,
                embedding_point_id: r.embedding_point_id,
                created_at: r.created_at,
            })
            .collect())
    }

    /// Get recent fixes
    pub async fn get_recent_fixes(&self, project_id: &str, limit: i64) -> Result<Vec<HistoricalFix>> {
        let rows = sqlx::query!(
            r#"
            SELECT id, project_id, error_pattern, error_category, fix_commit_hash,
                   files_modified, fix_description, fixed_at, similarity_hash,
                   embedding_point_id, created_at
            FROM historical_fixes
            WHERE project_id = ?
            ORDER BY fixed_at DESC
            LIMIT ?
            "#,
            project_id,
            limit
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| HistoricalFix {
                id: r.id,
                project_id: r.project_id,
                error_pattern: r.error_pattern,
                error_category: r.error_category,
                fix_commit_hash: r.fix_commit_hash,
                files_modified: serde_json::from_str(&r.files_modified).unwrap_or_default(),
                fix_description: r.fix_description,
                fixed_at: r.fixed_at,
                similarity_hash: r.similarity_hash,
                embedding_point_id: r.embedding_point_id,
                created_at: r.created_at,
            })
            .collect())
    }

    /// Get statistics for historical fixes
    pub async fn get_stats(&self, project_id: &str) -> Result<FixStats> {
        // Total fixes
        let total: i64 = sqlx::query_scalar!(
            "SELECT COUNT(*) as count FROM historical_fixes WHERE project_id = ?",
            project_id
        )
        .fetch_one(&self.pool)
        .await? as i64;

        // By category
        let category_rows = sqlx::query!(
            r#"
            SELECT error_category, COUNT(*) as count
            FROM historical_fixes
            WHERE project_id = ?
            GROUP BY error_category
            ORDER BY count DESC
            "#,
            project_id
        )
        .fetch_all(&self.pool)
        .await?;

        let by_category: Vec<(String, i64)> = category_rows
            .into_iter()
            .map(|r| (r.error_category, r.count as i64))
            .collect();

        // Most fixed files (parse JSON and count)
        let all_fixes = self.get_project_fixes(project_id).await?;
        let mut file_counts: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
        for fix in &all_fixes {
            for file in &fix.files_modified {
                *file_counts.entry(file.clone()).or_insert(0) += 1;
            }
        }
        let mut most_fixed: Vec<(String, i64)> = file_counts.into_iter().collect();
        most_fixed.sort_by(|a, b| b.1.cmp(&a.1));
        most_fixed.truncate(10);

        // Recent fixes (last 30 days)
        let thirty_days_ago = Utc::now().timestamp() - (30 * 24 * 60 * 60);
        let recent: i64 = sqlx::query_scalar!(
            "SELECT COUNT(*) as count FROM historical_fixes WHERE project_id = ? AND fixed_at > ?",
            project_id,
            thirty_days_ago
        )
        .fetch_one(&self.pool)
        .await? as i64;

        Ok(FixStats {
            total_fixes: total,
            by_category,
            most_fixed_files: most_fixed,
            recent_fixes_count: recent,
        })
    }

    // ========================================================================
    // Maintenance
    // ========================================================================

    /// Delete fixes for a project
    pub async fn delete_project_fixes(&self, project_id: &str) -> Result<u64> {
        let result = sqlx::query!(
            "DELETE FROM historical_fixes WHERE project_id = ?",
            project_id
        )
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected())
    }

    /// Update embedding point ID for a fix
    pub async fn set_embedding(&self, fix_id: i64, point_id: &str) -> Result<()> {
        sqlx::query!(
            "UPDATE historical_fixes SET embedding_point_id = ? WHERE id = ?",
            point_id,
            fix_id
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_classification() {
        assert_eq!(
            FixService::classify_error("cannot find module 'foo'"),
            ErrorCategory::ImportError
        );
        assert_eq!(
            FixService::classify_error("type mismatch: expected i32, got String"),
            ErrorCategory::TypeMismatch
        );
        assert_eq!(
            FixService::classify_error("null pointer exception"),
            ErrorCategory::NullReference
        );
        assert_eq!(
            FixService::classify_error("index out of bounds"),
            ErrorCategory::BoundsError
        );
        assert_eq!(
            FixService::classify_error("syntax error on line 42"),
            ErrorCategory::SyntaxError
        );
        assert_eq!(
            FixService::classify_error("random message"),
            ErrorCategory::Unknown
        );
    }

    #[test]
    fn test_similarity_hash() {
        let hash1 = FixService::compute_similarity_hash("cannot find module", ErrorCategory::ImportError);
        let hash2 = FixService::compute_similarity_hash("cannot find module", ErrorCategory::ImportError);
        let hash3 = FixService::compute_similarity_hash("type mismatch", ErrorCategory::TypeMismatch);

        assert_eq!(hash1, hash2);
        assert_ne!(hash1, hash3);
        assert_eq!(hash1.len(), 64); // SHA-256 hex
    }

    #[test]
    fn test_normalize_error_pattern() {
        let normalized = FixService::normalize_error_pattern(
            "Error at /home/user/src/main.rs:42: cannot find \"variable_name\""
        );

        // Should remove numbers, paths, and quoted strings
        assert!(!normalized.contains("/home"));
        assert!(!normalized.contains("42"));
        assert!(!normalized.contains("variable_name"));
    }

    #[test]
    fn test_pattern_similarity() {
        let sim1 = FixService::compute_pattern_similarity(
            "cannot find module foo",
            "cannot find module bar"
        );
        let sim2 = FixService::compute_pattern_similarity(
            "cannot find module",
            "type mismatch error"
        );

        assert!(sim1 > sim2);
        assert!(sim1 > 0.5);
    }

    #[test]
    fn test_extract_error_pattern() {
        assert_eq!(
            FixService::extract_error_pattern("fix: handle null pointer"),
            "handle null pointer"
        );
        assert_eq!(
            FixService::extract_error_pattern("fix(parser): handle edge case"),
            "handle edge case"
        );
    }

    #[test]
    fn test_fix_serialization() {
        let fix = HistoricalFix {
            id: Some(1),
            project_id: "proj1".to_string(),
            error_pattern: "cannot find module".to_string(),
            error_category: "import_error".to_string(),
            fix_commit_hash: "abc123".to_string(),
            files_modified: vec!["src/main.rs".to_string()],
            fix_description: Some("Fixed import".to_string()),
            fixed_at: 1700000000,
            similarity_hash: "def456".to_string(),
            embedding_point_id: None,
            created_at: 1700000000,
        };

        let json = serde_json::to_string(&fix).unwrap();
        let deserialized: HistoricalFix = serde_json::from_str(&json).unwrap();

        assert_eq!(fix.error_pattern, deserialized.error_pattern);
        assert_eq!(fix.files_modified, deserialized.files_modified);
    }
}
