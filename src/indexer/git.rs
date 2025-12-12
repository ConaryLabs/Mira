// src/indexer/git.rs
// Git history indexing using git2

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use anyhow::{anyhow, Result};
use sqlx::sqlite::SqlitePool;
use chrono::Utc;
use git2::{Repository, Sort};

use super::IndexStats;

pub struct GitIndexer {
    db: SqlitePool,
}

/// Data extracted from a commit (Send-safe)
#[derive(Debug, Clone)]
struct CommitData {
    hash: String,
    author_name: String,
    author_email: String,
    message: String,
    files_changed: Vec<String>,
    insertions: usize,
    deletions: usize,
    committed_at: i64,
}

/// Cochange pattern (Send-safe)
#[derive(Debug, Clone)]
struct CochangePattern {
    file_a: String,
    file_b: String,
    cochange_count: usize,
    confidence: f64,
}

impl GitIndexer {
    pub fn new(db: SqlitePool) -> Self {
        Self { db }
    }

    /// Index git repository - commits and cochange patterns
    pub async fn index_repository(&self, path: &Path, commit_limit: usize) -> Result<IndexStats> {
        let path = path.to_path_buf();
        tracing::info!("Starting git indexing for {:?} (limit: {})", path, commit_limit);

        // Run git operations in blocking thread (git2 isn't Send)
        let (commits, patterns) = tokio::task::spawn_blocking(move || {
            tracing::info!("spawn_blocking started, calling extract_git_data");
            let result = Self::extract_git_data(&path, commit_limit);
            tracing::info!("extract_git_data completed: {:?}", result.as_ref().map(|(c, p)| (c.len(), p.len())));
            result
        }).await??;
        tracing::info!("spawn_blocking completed: {} commits, {} patterns", commits.len(), patterns.len());

        let mut stats = IndexStats::default();
        stats.commits_indexed = commits.len();
        stats.cochange_patterns = patterns.len();

        // Store in database
        tracing::info!("Storing {} commits in database", commits.len());
        self.store_commits(&commits).await?;
        tracing::info!("Storing {} cochange patterns in database", patterns.len());
        self.store_cochange_patterns(&patterns).await?;
        tracing::info!("Git indexing complete");

        Ok(stats)
    }

    /// Index recent commits only
    #[allow(dead_code)] // API for future incremental indexing
    pub async fn index_recent(&self, path: &Path, limit: usize) -> Result<IndexStats> {
        self.index_repository(path, limit).await
    }

    /// Extract git data synchronously (called from spawn_blocking)
    fn extract_git_data(path: &PathBuf, limit: usize) -> Result<(Vec<CommitData>, Vec<CochangePattern>)> {
        tracing::debug!("extract_git_data: opening repo at {:?}", path);
        let repo = Repository::open(path)
            .map_err(|e| anyhow!("Failed to open git repository: {}", e))?;
        tracing::debug!("extract_git_data: repo opened successfully");

        let mut revwalk = repo.revwalk()?;
        revwalk.push_head()?;
        revwalk.set_sorting(Sort::TIME)?;
        tracing::debug!("extract_git_data: revwalk initialized");

        let mut commits = Vec::new();

        for (count, oid_result) in revwalk.enumerate() {
            if count >= limit {
                break;
            }

            if count % 50 == 0 {
                tracing::debug!("extract_git_data: processing commit {} of {}", count, limit);
            }

            let oid = oid_result?;
            let commit = repo.find_commit(oid)?;

            let hash = commit.id().to_string();
            let author = commit.author();
            let author_name = author.name().unwrap_or("").to_string();
            let author_email = author.email().unwrap_or("").to_string();
            let message = commit.message().unwrap_or("").to_string();
            let committed_at = commit.time().seconds();

            // Get files changed
            let files_changed = Self::get_changed_files(&repo, &commit)?;
            let (insertions, deletions) = Self::get_diff_stats(&repo, &commit)?;

            commits.push(CommitData {
                hash,
                author_name,
                author_email,
                message,
                files_changed,
                insertions,
                deletions,
                committed_at,
            });
        }
        tracing::debug!("extract_git_data: extracted {} commits, computing cochange patterns", commits.len());

        // Compute cochange patterns
        let patterns = Self::compute_cochange_patterns(&commits);
        tracing::debug!("extract_git_data: computed {} cochange patterns", patterns.len());

        Ok((commits, patterns))
    }

    fn get_changed_files(repo: &Repository, commit: &git2::Commit) -> Result<Vec<String>> {
        let mut files = Vec::new();

        let tree = commit.tree()?;

        // Get parent tree (or empty tree for first commit)
        let parent_tree = if commit.parent_count() > 0 {
            Some(commit.parent(0)?.tree()?)
        } else {
            None
        };

        let diff = repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&tree), None)?;

        for delta in diff.deltas() {
            if let Some(path) = delta.new_file().path() {
                files.push(path.to_string_lossy().to_string());
            } else if let Some(path) = delta.old_file().path() {
                files.push(path.to_string_lossy().to_string());
            }
        }

        Ok(files)
    }

    fn get_diff_stats(repo: &Repository, commit: &git2::Commit) -> Result<(usize, usize)> {
        let tree = commit.tree()?;

        let parent_tree = if commit.parent_count() > 0 {
            Some(commit.parent(0)?.tree()?)
        } else {
            None
        };

        let diff = repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&tree), None)?;
        let stats = diff.stats()?;

        Ok((stats.insertions(), stats.deletions()))
    }

    fn compute_cochange_patterns(commits: &[CommitData]) -> Vec<CochangePattern> {
        let mut pair_counts: HashMap<(String, String), usize> = HashMap::new();
        let mut file_counts: HashMap<String, usize> = HashMap::new();

        // Count file occurrences and pair occurrences
        for commit in commits {
            let files = &commit.files_changed;

            // Skip commits with too many files (likely bulk changes)
            if files.len() > 50 {
                continue;
            }

            // Count individual files
            for file in files {
                *file_counts.entry(file.clone()).or_insert(0) += 1;
            }

            // Count pairs (order-independent)
            for i in 0..files.len() {
                for j in (i + 1)..files.len() {
                    let (a, b) = if files[i] < files[j] {
                        (files[i].clone(), files[j].clone())
                    } else {
                        (files[j].clone(), files[i].clone())
                    };
                    *pair_counts.entry((a, b)).or_insert(0) += 1;
                }
            }
        }

        // Calculate confidence scores and filter significant patterns
        let mut patterns = Vec::new();

        for ((file_a, file_b), count) in pair_counts {
            // Only include pairs that changed together at least 2 times
            if count < 2 {
                continue;
            }

            // Calculate confidence as: cochange_count / min(count_a, count_b)
            let count_a = *file_counts.get(&file_a).unwrap_or(&1);
            let count_b = *file_counts.get(&file_b).unwrap_or(&1);
            let min_count = count_a.min(count_b);
            let confidence = count as f64 / min_count as f64;

            // Only include patterns with reasonable confidence
            if confidence >= 0.2 {
                patterns.push(CochangePattern {
                    file_a,
                    file_b,
                    cochange_count: count,
                    confidence,
                });
            }
        }

        // Sort by confidence descending
        patterns.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap());

        patterns
    }

    async fn store_commits(&self, commits: &[CommitData]) -> Result<()> {
        let now = Utc::now().timestamp();

        // Use transaction for batch insert (much faster than individual inserts)
        let mut tx = self.db.begin().await?;

        for commit in commits {
            let files_json = serde_json::to_string(&commit.files_changed)?;

            sqlx::query(r#"
                INSERT INTO git_commits
                (commit_hash, author_name, author_email, message, files_changed, insertions, deletions, committed_at, indexed_at)
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
                ON CONFLICT(commit_hash) DO UPDATE SET
                    indexed_at = excluded.indexed_at
            "#)
            .bind(&commit.hash)
            .bind(&commit.author_name)
            .bind(&commit.author_email)
            .bind(&commit.message)
            .bind(&files_json)
            .bind(commit.insertions as i32)
            .bind(commit.deletions as i32)
            .bind(commit.committed_at)
            .bind(now)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    async fn store_cochange_patterns(&self, patterns: &[CochangePattern]) -> Result<()> {
        let now = Utc::now().timestamp();

        // Use transaction for batch insert
        let mut tx = self.db.begin().await?;

        for pattern in patterns {
            sqlx::query(r#"
                INSERT INTO cochange_patterns (file_a, file_b, cochange_count, confidence, last_seen)
                VALUES ($1, $2, $3, $4, $5)
                ON CONFLICT(file_a, file_b) DO UPDATE SET
                    cochange_count = excluded.cochange_count,
                    confidence = excluded.confidence,
                    last_seen = excluded.last_seen
            "#)
            .bind(&pattern.file_a)
            .bind(&pattern.file_b)
            .bind(pattern.cochange_count as i32)
            .bind(pattern.confidence)
            .bind(now)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_extract_git_data() {
        let path = PathBuf::from("/home/peter/Mira");
        let result = GitIndexer::extract_git_data(&path, 10);
        match result {
            Ok((commits, patterns)) => {
                println!("Extracted {} commits, {} patterns", commits.len(), patterns.len());
                for commit in commits.iter().take(3) {
                    println!("  {} - {}", &commit.hash[..8], &commit.message.lines().next().unwrap_or(""));
                }
                assert!(!commits.is_empty(), "Should have commits");
            }
            Err(e) => panic!("Failed to extract git data: {}", e),
        }
    }
}
