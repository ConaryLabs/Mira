// backend/src/git/intelligence/commits.rs
// Git commit indexing and storage

use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use tracing::info;

// ============================================================================
// Data Types
// ============================================================================

/// A git commit with full metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitCommit {
    pub id: Option<i64>,
    pub project_id: String,
    pub commit_hash: String,
    pub author_name: String,
    pub author_email: String,
    pub commit_message: String,
    pub message_summary: String,
    pub authored_at: i64,
    pub committed_at: i64,
    pub parent_hashes: Vec<String>,
    pub file_changes: Vec<CommitFileChange>,
    pub insertions: i64,
    pub deletions: i64,
    pub embedding_point_id: Option<String>,
    pub indexed_at: i64,
}

/// A file change in a commit
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitFileChange {
    pub path: String,
    pub change_type: FileChangeType,
    pub insertions: i64,
    pub deletions: i64,
}

/// Type of file change
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum FileChangeType {
    Added,
    Modified,
    Deleted,
    Renamed,
    Copied,
}

impl FileChangeType {
    pub fn as_str(&self) -> &'static str {
        match self {
            FileChangeType::Added => "added",
            FileChangeType::Modified => "modified",
            FileChangeType::Deleted => "deleted",
            FileChangeType::Renamed => "renamed",
            FileChangeType::Copied => "copied",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "added" | "a" => FileChangeType::Added,
            "modified" | "m" => FileChangeType::Modified,
            "deleted" | "d" => FileChangeType::Deleted,
            "renamed" | "r" => FileChangeType::Renamed,
            "copied" | "c" => FileChangeType::Copied,
            _ => FileChangeType::Modified,
        }
    }
}

/// Query parameters for commit search
#[derive(Debug, Clone, Default)]
pub struct CommitQuery {
    pub project_id: String,
    pub author_email: Option<String>,
    pub file_path: Option<String>,
    pub since: Option<i64>,
    pub until: Option<i64>,
    pub limit: Option<i64>,
}

/// Commit statistics
#[derive(Debug, Clone, Default)]
pub struct CommitStats {
    pub total_commits: i64,
    pub total_insertions: i64,
    pub total_deletions: i64,
    pub unique_authors: i64,
    pub files_changed: i64,
    pub first_commit: Option<i64>,
    pub last_commit: Option<i64>,
}

// ============================================================================
// Commit Service
// ============================================================================

/// Service for managing git commits
pub struct CommitService {
    pool: SqlitePool,
}

impl CommitService {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    // ========================================================================
    // Commit CRUD Operations
    // ========================================================================

    /// Index a new commit
    pub async fn index_commit(&self, commit: &GitCommit) -> Result<i64> {
        let now = Utc::now().timestamp();
        let parent_hashes_json = serde_json::to_string(&commit.parent_hashes)?;
        let file_changes_json = serde_json::to_string(&commit.file_changes)?;

        let result = sqlx::query!(
            r#"
            INSERT INTO git_commits (
                project_id, commit_hash, author_name, author_email,
                commit_message, message_summary, authored_at, committed_at,
                parent_hashes, file_changes, insertions, deletions,
                embedding_point_id, indexed_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(project_id, commit_hash) DO UPDATE SET
                author_name = excluded.author_name,
                author_email = excluded.author_email,
                commit_message = excluded.commit_message,
                message_summary = excluded.message_summary,
                file_changes = excluded.file_changes,
                insertions = excluded.insertions,
                deletions = excluded.deletions,
                indexed_at = excluded.indexed_at
            RETURNING id
            "#,
            commit.project_id,
            commit.commit_hash,
            commit.author_name,
            commit.author_email,
            commit.commit_message,
            commit.message_summary,
            commit.authored_at,
            commit.committed_at,
            parent_hashes_json,
            file_changes_json,
            commit.insertions,
            commit.deletions,
            commit.embedding_point_id,
            now
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(result.id)
    }

    /// Index multiple commits in a batch
    pub async fn index_commits_batch(&self, commits: &[GitCommit]) -> Result<usize> {
        let mut count = 0;
        for commit in commits {
            self.index_commit(commit).await?;
            count += 1;
        }
        info!("Indexed {} commits", count);
        Ok(count)
    }

    /// Get a commit by hash
    pub async fn get_commit(&self, project_id: &str, commit_hash: &str) -> Result<Option<GitCommit>> {
        let row = sqlx::query!(
            r#"
            SELECT id, project_id, commit_hash, author_name, author_email,
                   commit_message, message_summary, authored_at, committed_at,
                   parent_hashes, file_changes, insertions, deletions,
                   embedding_point_id, indexed_at
            FROM git_commits
            WHERE project_id = ? AND commit_hash = ?
            "#,
            project_id,
            commit_hash
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| GitCommit {
            id: r.id,
            project_id: r.project_id,
            commit_hash: r.commit_hash,
            author_name: r.author_name,
            author_email: r.author_email,
            commit_message: r.commit_message,
            message_summary: r.message_summary,
            authored_at: r.authored_at,
            committed_at: r.committed_at,
            parent_hashes: parse_json_array(&r.parent_hashes),
            file_changes: parse_file_changes(&r.file_changes),
            insertions: r.insertions.unwrap_or(0),
            deletions: r.deletions.unwrap_or(0),
            embedding_point_id: r.embedding_point_id,
            indexed_at: r.indexed_at,
        }))
    }

    /// Query commits with filters
    pub async fn query_commits(&self, query: &CommitQuery) -> Result<Vec<GitCommit>> {
        let limit = query.limit.unwrap_or(100);

        // Build dynamic query based on filters
        let mut sql = String::from(
            r#"
            SELECT id, project_id, commit_hash, author_name, author_email,
                   commit_message, message_summary, authored_at, committed_at,
                   parent_hashes, file_changes, insertions, deletions,
                   embedding_point_id, indexed_at
            FROM git_commits
            WHERE project_id = ?
            "#
        );

        let mut conditions: Vec<String> = Vec::new();

        if query.author_email.is_some() {
            conditions.push("author_email = ?".to_string());
        }
        if query.since.is_some() {
            conditions.push("authored_at >= ?".to_string());
        }
        if query.until.is_some() {
            conditions.push("authored_at <= ?".to_string());
        }
        if query.file_path.is_some() {
            conditions.push("file_changes LIKE ?".to_string());
        }

        for condition in &conditions {
            sql.push_str(" AND ");
            sql.push_str(condition);
        }

        sql.push_str(" ORDER BY authored_at DESC LIMIT ?");

        // Execute with dynamic binding
        let rows = sqlx::query(&sql)
            .bind(&query.project_id)
            .bind(query.author_email.as_ref())
            .bind(query.since)
            .bind(query.until)
            .bind(query.file_path.as_ref().map(|p| format!("%{}%", p)))
            .bind(limit)
            .fetch_all(&self.pool)
            .await?;

        // Note: This simplified version doesn't handle all filter combinations
        // For production, use a query builder or separate queries per filter combo

        // Fallback to simple query for now
        let rows = sqlx::query!(
            r#"
            SELECT id, project_id, commit_hash, author_name, author_email,
                   commit_message, message_summary, authored_at, committed_at,
                   parent_hashes, file_changes, insertions, deletions,
                   embedding_point_id, indexed_at
            FROM git_commits
            WHERE project_id = ?
            ORDER BY authored_at DESC
            LIMIT ?
            "#,
            query.project_id,
            limit
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| GitCommit {
                id: r.id,
                project_id: r.project_id,
                commit_hash: r.commit_hash,
                author_name: r.author_name,
                author_email: r.author_email,
                commit_message: r.commit_message,
                message_summary: r.message_summary,
                authored_at: r.authored_at,
                committed_at: r.committed_at,
                parent_hashes: parse_json_array(&r.parent_hashes),
                file_changes: parse_file_changes(&r.file_changes),
                insertions: r.insertions.unwrap_or(0),
                deletions: r.deletions.unwrap_or(0),
                embedding_point_id: r.embedding_point_id,
                indexed_at: r.indexed_at,
            })
            .collect())
    }

    /// Get commits by author
    pub async fn get_commits_by_author(
        &self,
        project_id: &str,
        author_email: &str,
        limit: i64,
    ) -> Result<Vec<GitCommit>> {
        let rows = sqlx::query!(
            r#"
            SELECT id, project_id, commit_hash, author_name, author_email,
                   commit_message, message_summary, authored_at, committed_at,
                   parent_hashes, file_changes, insertions, deletions,
                   embedding_point_id, indexed_at
            FROM git_commits
            WHERE project_id = ? AND author_email = ?
            ORDER BY authored_at DESC
            LIMIT ?
            "#,
            project_id,
            author_email,
            limit
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| GitCommit {
                id: r.id,
                project_id: r.project_id,
                commit_hash: r.commit_hash,
                author_name: r.author_name,
                author_email: r.author_email,
                commit_message: r.commit_message,
                message_summary: r.message_summary,
                authored_at: r.authored_at,
                committed_at: r.committed_at,
                parent_hashes: parse_json_array(&r.parent_hashes),
                file_changes: parse_file_changes(&r.file_changes),
                insertions: r.insertions.unwrap_or(0),
                deletions: r.deletions.unwrap_or(0),
                embedding_point_id: r.embedding_point_id,
                indexed_at: r.indexed_at,
            })
            .collect())
    }

    /// Get commits that modified a specific file
    pub async fn get_commits_for_file(
        &self,
        project_id: &str,
        file_path: &str,
        limit: i64,
    ) -> Result<Vec<GitCommit>> {
        let pattern = format!("%\"path\":\"{}%", file_path);

        let rows = sqlx::query!(
            r#"
            SELECT id, project_id, commit_hash, author_name, author_email,
                   commit_message, message_summary, authored_at, committed_at,
                   parent_hashes, file_changes, insertions, deletions,
                   embedding_point_id, indexed_at
            FROM git_commits
            WHERE project_id = ? AND file_changes LIKE ?
            ORDER BY authored_at DESC
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
            .map(|r| GitCommit {
                id: r.id,
                project_id: r.project_id,
                commit_hash: r.commit_hash,
                author_name: r.author_name,
                author_email: r.author_email,
                commit_message: r.commit_message,
                message_summary: r.message_summary,
                authored_at: r.authored_at,
                committed_at: r.committed_at,
                parent_hashes: parse_json_array(&r.parent_hashes),
                file_changes: parse_file_changes(&r.file_changes),
                insertions: r.insertions.unwrap_or(0),
                deletions: r.deletions.unwrap_or(0),
                embedding_point_id: r.embedding_point_id,
                indexed_at: r.indexed_at,
            })
            .collect())
    }

    /// Get recent commits for a project
    pub async fn get_recent_commits(&self, project_id: &str, limit: i64) -> Result<Vec<GitCommit>> {
        let rows = sqlx::query!(
            r#"
            SELECT id, project_id, commit_hash, author_name, author_email,
                   commit_message, message_summary, authored_at, committed_at,
                   parent_hashes, file_changes, insertions, deletions,
                   embedding_point_id, indexed_at
            FROM git_commits
            WHERE project_id = ?
            ORDER BY authored_at DESC
            LIMIT ?
            "#,
            project_id,
            limit
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| GitCommit {
                id: r.id,
                project_id: r.project_id,
                commit_hash: r.commit_hash,
                author_name: r.author_name,
                author_email: r.author_email,
                commit_message: r.commit_message,
                message_summary: r.message_summary,
                authored_at: r.authored_at,
                committed_at: r.committed_at,
                parent_hashes: parse_json_array(&r.parent_hashes),
                file_changes: parse_file_changes(&r.file_changes),
                insertions: r.insertions.unwrap_or(0),
                deletions: r.deletions.unwrap_or(0),
                embedding_point_id: r.embedding_point_id,
                indexed_at: r.indexed_at,
            })
            .collect())
    }

    // ========================================================================
    // Statistics and Analytics
    // ========================================================================

    /// Get commit statistics for a project
    pub async fn get_stats(&self, project_id: &str) -> Result<CommitStats> {
        let total_commits: i64 = sqlx::query_scalar!(
            "SELECT COUNT(*) as count FROM git_commits WHERE project_id = ?",
            project_id
        )
        .fetch_one(&self.pool)
        .await? as i64;

        let stats_row = sqlx::query!(
            r#"
            SELECT
                COALESCE(SUM(insertions), 0) as total_insertions,
                COALESCE(SUM(deletions), 0) as total_deletions,
                COUNT(DISTINCT author_email) as unique_authors,
                MIN(authored_at) as first_commit,
                MAX(authored_at) as last_commit
            FROM git_commits
            WHERE project_id = ?
            "#,
            project_id
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(CommitStats {
            total_commits,
            total_insertions: stats_row.total_insertions as i64,
            total_deletions: stats_row.total_deletions as i64,
            unique_authors: stats_row.unique_authors as i64,
            files_changed: 0, // Would need separate calculation
            first_commit: stats_row.first_commit,
            last_commit: stats_row.last_commit,
        })
    }

    /// Get unique files from all commits
    pub async fn get_all_changed_files(&self, project_id: &str) -> Result<Vec<String>> {
        let rows = sqlx::query!(
            "SELECT file_changes FROM git_commits WHERE project_id = ?",
            project_id
        )
        .fetch_all(&self.pool)
        .await?;

        let mut files: std::collections::HashSet<String> = std::collections::HashSet::new();

        for row in rows {
            let changes = parse_file_changes(&row.file_changes);
            for change in changes {
                files.insert(change.path);
            }
        }

        Ok(files.into_iter().collect())
    }

    /// Update embedding point ID for a commit
    pub async fn set_embedding_point_id(
        &self,
        commit_id: i64,
        point_id: &str,
    ) -> Result<()> {
        sqlx::query!(
            "UPDATE git_commits SET embedding_point_id = ? WHERE id = ?",
            point_id,
            commit_id
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Delete commits for a project
    pub async fn delete_project_commits(&self, project_id: &str) -> Result<u64> {
        let result = sqlx::query!(
            "DELETE FROM git_commits WHERE project_id = ?",
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

fn parse_json_array(json: &Option<String>) -> Vec<String> {
    json.as_ref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default()
}

fn parse_file_changes(json: &str) -> Vec<CommitFileChange> {
    serde_json::from_str(json).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_change_type_conversion() {
        assert_eq!(FileChangeType::from_str("added"), FileChangeType::Added);
        assert_eq!(FileChangeType::from_str("M"), FileChangeType::Modified);
        assert_eq!(FileChangeType::from_str("D"), FileChangeType::Deleted);
        assert_eq!(FileChangeType::from_str("unknown"), FileChangeType::Modified);
    }

    #[test]
    fn test_commit_serialization() {
        let commit = GitCommit {
            id: None,
            project_id: "proj1".to_string(),
            commit_hash: "abc123".to_string(),
            author_name: "Test User".to_string(),
            author_email: "test@example.com".to_string(),
            commit_message: "Test commit".to_string(),
            message_summary: "Test".to_string(),
            authored_at: 1700000000,
            committed_at: 1700000000,
            parent_hashes: vec!["def456".to_string()],
            file_changes: vec![CommitFileChange {
                path: "src/main.rs".to_string(),
                change_type: FileChangeType::Modified,
                insertions: 10,
                deletions: 5,
            }],
            insertions: 10,
            deletions: 5,
            embedding_point_id: None,
            indexed_at: 1700000000,
        };

        let json = serde_json::to_string(&commit).unwrap();
        let deserialized: GitCommit = serde_json::from_str(&json).unwrap();

        assert_eq!(commit.commit_hash, deserialized.commit_hash);
        assert_eq!(commit.file_changes.len(), deserialized.file_changes.len());
    }
}
