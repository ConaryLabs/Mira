// src/daemon.rs
// Background daemon for continuous file watching and indexing

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use anyhow::Result;
use sqlx::sqlite::SqlitePool;
use tokio::time::interval;

use crate::indexer::{CodeIndexer, GitIndexer, Watcher};
use crate::tools::SemanticSearch;

const PID_FILE: &str = "data/mira-daemon.pid";

pub struct Daemon {
    project_path: PathBuf,
    db: SqlitePool,
    semantic: Arc<SemanticSearch>,
}

impl Daemon {
    pub async fn new(project_path: &Path, database_url: &str, qdrant_url: Option<&str>, gemini_key: Option<String>) -> Result<Self> {
        let db = SqlitePool::connect(database_url).await?;
        let semantic = SemanticSearch::new(qdrant_url, gemini_key).await;

        Ok(Self {
            project_path: project_path.to_path_buf(),
            db,
            semantic: Arc::new(semantic),
        })
    }

    /// Run the daemon - watches files and periodically syncs git
    pub async fn run(&self) -> Result<()> {
        // Write PID file
        let pid = std::process::id();
        let pid_path = self.project_path.join(PID_FILE);
        if let Some(parent) = pid_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&pid_path, pid.to_string())?;
        tracing::info!("Daemon started with PID {} (file: {})", pid, pid_path.display());

        // Do initial index if needed
        self.initial_index().await?;

        // Start file watcher
        let mut watcher = Watcher::with_semantic(
            &self.project_path,
            self.db.clone(),
            Some(self.semantic.clone()),
        );
        watcher.start().await?;
        tracing::info!("File watcher started for {}", self.project_path.display());

        // Periodic git sync (every 5 minutes)
        let db = self.db.clone();
        let project_path = self.project_path.clone();
        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(300));
            loop {
                interval.tick().await;
                tracing::debug!("Running periodic git sync");
                let git_indexer = GitIndexer::new(db.clone());
                match git_indexer.index_repository(&project_path, 50).await {
                    Ok(stats) => {
                        if stats.commits_indexed > 0 {
                            tracing::info!(
                                "Git sync: {} commits, {} cochange patterns",
                                stats.commits_indexed,
                                stats.cochange_patterns
                            );
                        }
                    }
                    Err(e) => tracing::warn!("Git sync failed: {}", e),
                }
            }
        });

        // Wait for shutdown signal
        tracing::info!("Daemon running. Press Ctrl+C to stop.");
        tokio::signal::ctrl_c().await?;

        // Cleanup
        let _ = std::fs::remove_file(&pid_path);
        tracing::info!("Daemon stopped");
        Ok(())
    }

    async fn initial_index(&self) -> Result<()> {
        // Check if we need initial indexing
        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM code_symbols")
            .fetch_one(&self.db)
            .await?;

        if count.0 == 0 {
            tracing::info!("Running initial index of {}", self.project_path.display());
            let mut code_indexer = CodeIndexer::with_semantic(
                self.db.clone(),
                Some(self.semantic.clone()),
            )?;
            let mut stats = code_indexer.index_directory(&self.project_path).await?;

            let git_indexer = GitIndexer::new(self.db.clone());
            let git_stats = git_indexer.index_repository(&self.project_path, 500).await?;
            stats.merge(git_stats);

            tracing::info!(
                "Initial index complete: {} symbols, {} embeddings, {} commits",
                stats.symbols_found,
                stats.embeddings_generated,
                stats.commits_indexed
            );
        }

        Ok(())
    }
}

/// Check if daemon is running
pub fn is_running(project_path: &Path) -> Option<u32> {
    let pid_path = project_path.join(PID_FILE);
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

/// Stop the daemon
pub fn stop(project_path: &Path) -> Result<bool> {
    if let Some(pid) = is_running(project_path) {
        // Send SIGTERM
        unsafe {
            libc::kill(pid as i32, libc::SIGTERM);
        }
        // Remove PID file
        let _ = std::fs::remove_file(project_path.join(PID_FILE));
        Ok(true)
    } else {
        Ok(false)
    }
}
