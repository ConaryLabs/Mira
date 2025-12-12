// src/indexer/watcher.rs
// Background file watcher for incremental indexing with semantic embeddings

use std::path::{Path, PathBuf};
use std::time::Duration;
use std::sync::Arc;
use anyhow::Result;
use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher as NotifyWatcher, Event, EventKind};
use tokio::sync::mpsc;
use sqlx::sqlite::SqlitePool;

use super::CodeIndexer;
use crate::tools::SemanticSearch;

/// Event types for the file watcher
enum FileEvent {
    /// File was created or modified - needs (re)indexing
    Changed(PathBuf),
    /// File was deleted - needs cleanup
    Deleted(PathBuf),
}

pub struct Watcher {
    path: PathBuf,
    db: SqlitePool,
    semantic: Option<Arc<SemanticSearch>>,
    shutdown_tx: Option<mpsc::Sender<()>>,
}

impl Watcher {
    #[allow(dead_code)] // Used by daemon
    pub fn new(path: &Path, db: SqlitePool) -> Self {
        Self {
            path: path.to_path_buf(),
            db,
            semantic: None,
            shutdown_tx: None,
        }
    }

    pub fn with_semantic(path: &Path, db: SqlitePool, semantic: Option<Arc<SemanticSearch>>) -> Self {
        Self {
            path: path.to_path_buf(),
            db,
            semantic,
            shutdown_tx: None,
        }
    }

    /// Start watching for file changes
    /// Returns a handle that can be used to stop the watcher
    pub async fn start(&mut self) -> Result<()> {
        let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);
        self.shutdown_tx = Some(shutdown_tx);

        let (event_tx, mut event_rx) = mpsc::channel::<FileEvent>(100);

        let path = self.path.clone();
        let db = self.db.clone();

        // Spawn the watcher thread
        std::thread::spawn(move || {
            let rt = match tokio::runtime::Runtime::new() {
                Ok(rt) => rt,
                Err(e) => {
                    tracing::error!("Failed to create tokio runtime for watcher: {}", e);
                    return;
                }
            };
            let event_tx = event_tx.clone();

            let mut watcher = match RecommendedWatcher::new(
                move |res: Result<Event, notify::Error>| {
                    if let Ok(event) = res {
                        for path in event.paths {
                            // Only process code files
                            if let Some(ext) = path.extension() {
                                let ext = ext.to_string_lossy();
                                if matches!(ext.as_ref(), "rs" | "py" | "ts" | "tsx" | "js" | "jsx") {
                                    let file_event = match event.kind {
                                        EventKind::Create(_) | EventKind::Modify(_) => {
                                            Some(FileEvent::Changed(path))
                                        }
                                        EventKind::Remove(_) => {
                                            Some(FileEvent::Deleted(path))
                                        }
                                        _ => None,
                                    };
                                    if let Some(fe) = file_event {
                                        let _ = rt.block_on(event_tx.send(fe));
                                    }
                                }
                            }
                        }
                    }
                },
                Config::default().with_poll_interval(Duration::from_secs(2)),
            ) {
                Ok(w) => w,
                Err(e) => {
                    tracing::error!("Failed to create file watcher: {}", e);
                    return;
                }
            };

            if let Err(e) = watcher.watch(&path, RecursiveMode::Recursive) {
                tracing::error!("Failed to start watching {}: {}", path.display(), e);
                return;
            }

            // Keep thread alive
            loop {
                std::thread::sleep(Duration::from_secs(1));
            }
        });

        let semantic = self.semantic.clone();

        // Spawn the indexer task
        tokio::spawn(async move {
            let mut indexer = match CodeIndexer::with_semantic(db, semantic) {
                Ok(i) => i,
                Err(e) => {
                    tracing::error!("Failed to create code indexer: {}", e);
                    return;
                }
            };

            loop {
                tokio::select! {
                    Some(event) = event_rx.recv() => {
                        // Debounce: wait a bit in case there are more changes
                        tokio::time::sleep(Duration::from_millis(500)).await;

                        // Drain any pending events (simple debounce)
                        while event_rx.try_recv().is_ok() {}

                        match event {
                            FileEvent::Changed(path) => {
                                // Index the file
                                match indexer.index_file(&path).await {
                                    Ok(stats) => {
                                        if stats.embeddings_generated > 0 {
                                            tracing::info!(
                                                "Indexed {}: {} symbols, {} imports, {} embeddings",
                                                path.display(),
                                                stats.symbols_found,
                                                stats.imports_found,
                                                stats.embeddings_generated
                                            );
                                        } else {
                                            tracing::info!(
                                                "Indexed {}: {} symbols, {} imports",
                                                path.display(),
                                                stats.symbols_found,
                                                stats.imports_found
                                            );
                                        }
                                    }
                                    Err(e) => {
                                        tracing::warn!("Failed to index {}: {}", path.display(), e);
                                    }
                                }
                            }
                            FileEvent::Deleted(path) => {
                                // Clean up deleted file
                                match indexer.delete_file(&path).await {
                                    Ok(()) => {
                                        tracing::info!("Cleaned up deleted file: {}", path.display());
                                    }
                                    Err(e) => {
                                        tracing::warn!("Failed to clean up {}: {}", path.display(), e);
                                    }
                                }
                            }
                        }
                    }
                    _ = shutdown_rx.recv() => {
                        tracing::info!("Watcher shutting down");
                        break;
                    }
                }
            }
        });

        Ok(())
    }

    /// Stop the watcher
    #[allow(dead_code)] // For graceful shutdown
    pub async fn stop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(()).await;
        }
    }
}
