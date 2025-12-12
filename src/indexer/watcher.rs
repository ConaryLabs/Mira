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

pub struct Watcher {
    path: PathBuf,
    db: SqlitePool,
    semantic: Option<Arc<SemanticSearch>>,
    shutdown_tx: Option<mpsc::Sender<()>>,
}

impl Watcher {
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

        let (event_tx, mut event_rx) = mpsc::channel::<PathBuf>(100);

        let path = self.path.clone();
        let db = self.db.clone();

        // Spawn the watcher thread
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            let event_tx = event_tx.clone();

            let mut watcher = RecommendedWatcher::new(
                move |res: Result<Event, notify::Error>| {
                    if let Ok(event) = res {
                        match event.kind {
                            EventKind::Create(_) | EventKind::Modify(_) => {
                                for path in event.paths {
                                    // Only process code files
                                    if let Some(ext) = path.extension() {
                                        let ext = ext.to_string_lossy();
                                        if matches!(ext.as_ref(), "rs" | "py" | "ts" | "tsx" | "js" | "jsx") {
                                            let _ = rt.block_on(event_tx.send(path));
                                        }
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                },
                Config::default().with_poll_interval(Duration::from_secs(2)),
            ).unwrap();

            watcher.watch(&path, RecursiveMode::Recursive).unwrap();

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
                    Some(path) = event_rx.recv() => {
                        // Debounce: wait a bit in case there are more changes
                        tokio::time::sleep(Duration::from_millis(500)).await;

                        // Drain any pending events for the same file
                        while event_rx.try_recv().is_ok() {}

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
    pub async fn stop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(()).await;
        }
    }
}
