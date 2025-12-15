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
use crate::tools::ingest;

/// Event types for the file watcher
enum FileEvent {
    /// Code file was created or modified - needs (re)indexing
    CodeChanged(PathBuf),
    /// Code file was deleted - needs cleanup
    CodeDeleted(PathBuf),
    /// Document file was created or modified - needs (re)ingesting
    DocChanged(PathBuf),
    /// Document file was deleted - needs cleanup
    DocDeleted(PathBuf),
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
                            if let Some(ext) = path.extension() {
                                let ext = ext.to_string_lossy();
                                let is_code = matches!(ext.as_ref(), "rs" | "py" | "ts" | "tsx" | "js" | "jsx");
                                let is_doc = matches!(ext.as_ref(), "md" | "markdown" | "pdf" | "txt");

                                let file_event = match (is_code, is_doc, &event.kind) {
                                    (true, _, EventKind::Create(_) | EventKind::Modify(_)) => {
                                        Some(FileEvent::CodeChanged(path))
                                    }
                                    (true, _, EventKind::Remove(_)) => {
                                        Some(FileEvent::CodeDeleted(path))
                                    }
                                    (_, true, EventKind::Create(_) | EventKind::Modify(_)) => {
                                        Some(FileEvent::DocChanged(path))
                                    }
                                    (_, true, EventKind::Remove(_)) => {
                                        Some(FileEvent::DocDeleted(path))
                                    }
                                    _ => None,
                                };

                                if let Some(fe) = file_event {
                                    let _ = rt.block_on(event_tx.send(fe));
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

        // Clone for document handling (separate from code indexer)
        let db_for_docs = self.db.clone();
        let semantic_for_docs = self.semantic.clone();

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
                            FileEvent::CodeChanged(path) => {
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
                                            tracing::debug!(
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
                            FileEvent::CodeDeleted(path) => {
                                match indexer.delete_file(&path).await {
                                    Ok(()) => {
                                        tracing::info!("Cleaned up deleted code file: {}", path.display());
                                    }
                                    Err(e) => {
                                        tracing::warn!("Failed to clean up {}: {}", path.display(), e);
                                    }
                                }
                            }
                            FileEvent::DocChanged(path) => {
                                let path_str = path.to_string_lossy();
                                let semantic_ref = semantic_for_docs.as_deref();
                                match ingest::update_document(&db_for_docs, semantic_ref, &path_str).await {
                                    Ok(Some(result)) => {
                                        tracing::info!(
                                            "Ingested document {}: {} chunks, ~{} tokens",
                                            result.name,
                                            result.chunk_count,
                                            result.total_tokens
                                        );
                                    }
                                    Ok(None) => {
                                        tracing::debug!("Document unchanged: {}", path.display());
                                    }
                                    Err(e) => {
                                        tracing::warn!("Failed to ingest document {}: {}", path.display(), e);
                                    }
                                }
                            }
                            FileEvent::DocDeleted(path) => {
                                let path_str = path.to_string_lossy();
                                let semantic_ref = semantic_for_docs.as_deref();
                                match ingest::delete_document_by_path(&db_for_docs, semantic_ref, &path_str).await {
                                    Ok(true) => {
                                        tracing::info!("Removed deleted document: {}", path.display());
                                    }
                                    Ok(false) => {}
                                    Err(e) => {
                                        tracing::warn!("Failed to remove document {}: {}", path.display(), e);
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
