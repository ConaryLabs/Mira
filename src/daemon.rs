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

/// Global PID file location - stored in ~/.mira/ for system-wide daemon
fn pid_file_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home).join(".mira").join("mira-daemon.pid")
}

pub struct Daemon {
    project_paths: Vec<PathBuf>,
    db: SqlitePool,
    semantic: Arc<SemanticSearch>,
}

impl Daemon {
    pub async fn new(
        project_paths: Vec<PathBuf>,
        database_url: &str,
        qdrant_url: Option<&str>,
        gemini_key: Option<String>,
    ) -> Result<Self> {
        let db = SqlitePool::connect(database_url).await?;
        let semantic = SemanticSearch::new(qdrant_url, gemini_key).await;

        Ok(Self {
            project_paths,
            db,
            semantic: Arc::new(semantic),
        })
    }

    /// Run the daemon - watches files and periodically syncs git for all projects
    pub async fn run(&self) -> Result<()> {
        // Write PID file
        let pid = std::process::id();
        let pid_path = pid_file_path();
        if let Some(parent) = pid_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&pid_path, pid.to_string())?;
        tracing::info!("Daemon started with PID {} (file: {})", pid, pid_path.display());

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
        let project_paths = self.project_paths.clone();
        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(300));
            loop {
                interval.tick().await;
                tracing::debug!("Running periodic git sync for {} projects", project_paths.len());

                for project_path in &project_paths {
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

        // Wait for shutdown signal
        tracing::info!(
            "Daemon running, watching {} project(s). Press Ctrl+C to stop.",
            self.project_paths.len()
        );
        tokio::signal::ctrl_c().await?;

        // Cleanup
        let _ = std::fs::remove_file(&pid_path);
        tracing::info!("Daemon stopped");
        Ok(())
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

/// Check if daemon is running (uses global PID file)
pub fn is_running() -> Option<u32> {
    let pid_path = pid_file_path();
    if let Ok(contents) = std::fs::read_to_string(&pid_path) {
        if let Ok(pid) = contents.trim().parse::<u32>() {
            // Check if process exists
            if Path::new(&format!("/proc/{}", pid)).exists() {
                return Some(pid);
            }
        }
    }
    None
}

/// Stop the daemon (uses global PID file)
pub fn stop() -> Result<bool> {
    if let Some(pid) = is_running() {
        // Send SIGTERM
        unsafe {
            libc::kill(pid as i32, libc::SIGTERM);
        }
        // Remove PID file
        let _ = std::fs::remove_file(pid_file_path());
        Ok(true)
    } else {
        Ok(false)
    }
}
