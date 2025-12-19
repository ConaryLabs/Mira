// src/daemon.rs
// Background daemon for continuous file watching and indexing
// Supports watching multiple project directories simultaneously

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use anyhow::Result;
use sqlx::sqlite::SqlitePool;
use tokio::time::interval;

use crate::indexer::{CodeIndexer, GitIndexer, Watcher};
use crate::tools::SemanticSearch;
use git2::Repository;

pub struct Daemon {
    project_paths: Vec<PathBuf>,
    db: SqlitePool,
    semantic: Arc<SemanticSearch>,
}

/// Handles to background daemon tasks (for integration with serve-http)
pub struct DaemonTasks {
    /// Watcher handles (kept alive to maintain file watching)
    _watchers: Vec<Watcher>,
    /// Git sync task handle
    _git_sync: tokio::task::JoinHandle<()>,
}

impl Daemon {
    /// Create daemon with shared db and semantic instances
    pub fn with_shared(
        project_paths: Vec<PathBuf>,
        db: SqlitePool,
        semantic: Arc<SemanticSearch>,
    ) -> Self {
        Self {
            project_paths,
            db,
            semantic,
        }
    }

    /// Spawn background indexing tasks without blocking
    /// Returns handles that must be kept alive for tasks to continue
    pub async fn spawn_background_tasks(&self) -> Result<DaemonTasks> {
        // Do initial index for all projects
        for project_path in &self.project_paths {
            if let Err(e) = self.initial_index(project_path).await {
                tracing::warn!("Initial index failed for {}: {}", project_path.display(), e);
            }
        }

        // Start file watcher for each project
        let mut watchers = Vec::new();
        for project_path in &self.project_paths {
            let mut watcher = Watcher::with_semantic(
                project_path,
                self.db.clone(),
                Some(self.semantic.clone()),
            );
            if let Err(e) = watcher.start().await {
                tracing::warn!("Failed to start watcher for {}: {}", project_path.display(), e);
            } else {
                tracing::info!("File watcher started for {}", project_path.display());
                watchers.push(watcher);
            }
        }

        // Periodic git sync for all projects (every 5 minutes)
        let db = self.db.clone();
        let semantic = self.semantic.clone();
        let project_paths = self.project_paths.clone();
        let git_sync = tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(300));
            loop {
                interval.tick().await;
                tracing::debug!("Running periodic sync for {} projects", project_paths.len());

                for project_path in &project_paths {
                    // First, sync from remote (fetch + pull + re-index changed files)
                    match sync_from_remote(project_path, &db, &semantic).await {
                        Ok(n) if n > 0 => {
                            tracing::info!(
                                "Remote sync for {}: pulled and re-indexed {} files",
                                project_path.display(),
                                n
                            );
                        }
                        Err(e) => {
                            tracing::debug!("Remote sync skipped for {}: {}", project_path.display(), e);
                        }
                        _ => {}
                    }

                    // Then sync git history (commits, cochange patterns)
                    let git_indexer = GitIndexer::new(db.clone());
                    match git_indexer.index_repository(project_path, 50).await {
                        Ok(stats) => {
                            if stats.commits_indexed > 0 {
                                tracing::info!(
                                    "Git sync for {}: {} commits, {} cochange patterns",
                                    project_path.display(),
                                    stats.commits_indexed,
                                    stats.cochange_patterns
                                );
                            }
                        }
                        Err(e) => tracing::warn!(
                            "Git sync failed for {}: {}",
                            project_path.display(),
                            e
                        ),
                    }
                }
            }
        });

        Ok(DaemonTasks {
            _watchers: watchers,
            _git_sync: git_sync,
        })
    }

    async fn initial_index(&self, project_path: &Path) -> Result<()> {
        // Check if we need initial indexing for this project
        // We check if any symbols exist for files in this project's path
        let path_pattern = format!("{}%", project_path.display());
        let count: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM code_symbols WHERE file_path LIKE $1"
        )
        .bind(&path_pattern)
        .fetch_one(&self.db)
        .await?;

        if count.0 == 0 {
            tracing::info!("Running initial index of {}", project_path.display());
            let mut code_indexer = CodeIndexer::with_semantic(
                self.db.clone(),
                Some(self.semantic.clone()),
            )?;
            let mut stats = code_indexer.index_directory(project_path).await?;

            let git_indexer = GitIndexer::new(self.db.clone());
            let git_stats = git_indexer.index_repository(project_path, 500).await?;
            stats.merge(git_stats);

            tracing::info!(
                "Initial index complete for {}: {} symbols, {} embeddings, {} commits",
                project_path.display(),
                stats.symbols_found,
                stats.embeddings_generated,
                stats.commits_indexed
            );
        } else {
            tracing::debug!(
                "Skipping initial index for {} ({} symbols already indexed)",
                project_path.display(),
                count.0
            );
        }

        Ok(())
    }
}


/// Sync from remote (fetch + pull) and re-index changed files
/// Returns the number of files that were re-indexed
async fn sync_from_remote(
    project_path: &Path,
    db: &SqlitePool,
    semantic: &Arc<SemanticSearch>,
) -> Result<usize> {
    let project_path = project_path.to_path_buf();

    // Do all git operations in a blocking task (git2 types aren't Send)
    let changed_files = {
        let project_path = project_path.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<PathBuf>> {
            fetch_and_get_changed_files(&project_path)
        })
        .await??
    };

    if changed_files.is_empty() {
        return Ok(0);
    }

    // Re-index changed files (async)
    let mut indexer = CodeIndexer::with_semantic(db.clone(), Some(semantic.clone()))?;
    let mut indexed_count = 0;

    for file in &changed_files {
        if file.exists() {
            // File exists - index it
            if let Err(e) = indexer.index_file(file).await {
                tracing::debug!("Failed to index {}: {}", file.display(), e);
            } else {
                indexed_count += 1;
            }
        } else {
            // File was deleted - remove from index
            if let Err(e) = indexer.delete_file(file).await {
                tracing::debug!("Failed to delete {} from index: {}", file.display(), e);
            } else {
                indexed_count += 1;
            }
        }
    }

    Ok(indexed_count)
}

/// Synchronous git operations: fetch, check for changes, pull, return changed files
fn fetch_and_get_changed_files(project_path: &Path) -> Result<Vec<PathBuf>> {
    // Open the repository
    let repo = Repository::open(project_path)?;

    // Skip if no origin remote (just check it exists)
    let _ = repo.find_remote("origin")?;

    // Get current HEAD commit
    let head = repo.head()?.peel_to_commit()?;
    let head_oid = head.id();

    // Fetch from origin using git command (handles auth better)
    let fetch_output = std::process::Command::new("git")
        .args(["fetch", "origin"])
        .current_dir(project_path)
        .output()?;

    if !fetch_output.status.success() {
        anyhow::bail!("git fetch failed");
    }

    // Re-open repo to see fetched refs
    let repo = Repository::open(project_path)?;

    // Check if we have a tracking branch
    let tracking_branch = repo
        .find_branch("origin/main", git2::BranchType::Remote)
        .or_else(|_| repo.find_branch("origin/master", git2::BranchType::Remote))?;

    let remote_oid = tracking_branch.get().peel_to_commit()?.id();

    // If already up to date, nothing to do
    if head_oid == remote_oid {
        return Ok(Vec::new());
    }

    // Get the list of changed files between HEAD and remote
    let head_commit = repo.find_commit(head_oid)?;
    let head_tree = head_commit.tree()?;
    let remote_commit = repo.find_commit(remote_oid)?;
    let remote_tree = remote_commit.tree()?;

    let diff = repo.diff_tree_to_tree(Some(&head_tree), Some(&remote_tree), None)?;

    let changed_files: Vec<PathBuf> = diff
        .deltas()
        .filter_map(|delta| {
            delta
                .new_file()
                .path()
                .or_else(|| delta.old_file().path())
                .map(|p| project_path.join(p))
        })
        .collect();

    if changed_files.is_empty() {
        return Ok(Vec::new());
    }

    // Fast-forward pull
    let pull_output = std::process::Command::new("git")
        .args(["pull", "--ff-only"])
        .current_dir(project_path)
        .output()?;

    if !pull_output.status.success() {
        anyhow::bail!("git pull --ff-only failed (possible merge conflict)");
    }

    Ok(changed_files)
}
