// src/watcher/mod.rs
// Real-time file system watching for code intelligence updates

pub mod config;
pub mod events;
pub mod processor;
pub mod registry;

use anyhow::Result;
use notify_debouncer_full::{new_debouncer, DebounceEventResult, Debouncer, RecommendedCache};
use sqlx::SqlitePool;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use crate::memory::features::code_intelligence::CodeIntelligenceService;

pub use config::WatcherConfig;
pub use events::FileChangeEvent;
pub use processor::EventProcessor;
pub use registry::WatchRegistry;

/// Main file watcher service that coordinates watching, debouncing, and processing
pub struct WatcherService {
    pool: SqlitePool,
    config: WatcherConfig,
    registry: Arc<WatchRegistry>,
    processor: Arc<EventProcessor>,
    /// Channel to send shutdown signal
    shutdown_tx: Option<mpsc::Sender<()>>,
}

impl WatcherService {
    /// Create a new watcher service
    pub fn new(
        pool: SqlitePool,
        code_intelligence: Arc<CodeIntelligenceService>,
        config: WatcherConfig,
    ) -> Self {
        let registry = Arc::new(WatchRegistry::new());
        let processor = Arc::new(EventProcessor::new(
            pool.clone(),
            code_intelligence,
            registry.clone(),
            config.clone(),
        ));

        Self {
            pool,
            config,
            registry,
            processor,
            shutdown_tx: None,
        }
    }

    /// Start the file watcher service
    ///
    /// This spawns a background task that:
    /// 1. Creates a debounced file watcher
    /// 2. Listens for file system events
    /// 3. Processes events through the EventProcessor
    pub async fn start(&mut self) -> Result<()> {
        info!(
            "Starting file watcher service (debounce: {}ms, batch: {}ms)",
            self.config.debounce_ms, self.config.batch_ms
        );

        let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);
        self.shutdown_tx = Some(shutdown_tx);

        let processor = self.processor.clone();
        let registry = self.registry.clone();
        let debounce_duration = Duration::from_millis(self.config.debounce_ms);
        let batch_duration = Duration::from_millis(self.config.batch_ms);

        // Spawn the watcher task
        tokio::spawn(async move {
            // Create event channel
            let (event_tx, mut event_rx) = mpsc::channel::<DebounceEventResult>(1000);

            // Create the debounced watcher
            let watcher_result: Result<Debouncer<notify::RecommendedWatcher, RecommendedCache>, _> =
                new_debouncer(debounce_duration, None, move |result| {
                    let _ = event_tx.blocking_send(result);
                });

            let debouncer = match watcher_result {
                Ok(d) => d,
                Err(e) => {
                    error!("Failed to create file watcher: {}", e);
                    return;
                }
            };

            // Wrap debouncer in Arc<Mutex> and store in registry
            let debouncer = Arc::new(parking_lot::Mutex::new(debouncer));
            registry.set_debouncer(debouncer);

            info!("File watcher started successfully");

            // Batch collection for processing
            let mut pending_events: Vec<FileChangeEvent> = Vec::new();
            let mut batch_timer = tokio::time::interval(batch_duration);

            loop {
                tokio::select! {
                    // Handle shutdown signal
                    _ = shutdown_rx.recv() => {
                        info!("File watcher received shutdown signal");
                        break;
                    }

                    // Handle file system events
                    Some(result) = event_rx.recv() => {
                        match result {
                            Ok(events) => {
                                for event in events {
                                    if let Some(file_event) = FileChangeEvent::from_debounced(&event, &registry) {
                                        debug!("File event: {:?}", file_event);
                                        pending_events.push(file_event);
                                    }
                                }
                            }
                            Err(errors) => {
                                for e in errors {
                                    warn!("Watch error: {:?}", e);
                                }
                            }
                        }
                    }

                    // Process batched events
                    _ = batch_timer.tick() => {
                        if !pending_events.is_empty() {
                            let events = std::mem::take(&mut pending_events);
                            if let Err(e) = processor.process_batch(events).await {
                                error!("Failed to process file events: {}", e);
                            }
                        }
                    }
                }
            }

            info!("File watcher stopped");
        });

        Ok(())
    }

    /// Stop the file watcher service
    pub async fn stop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(()).await;
            info!("Sent shutdown signal to file watcher");
        }
    }

    /// Register a repository directory to watch
    pub async fn watch_repository(
        &self,
        attachment_id: String,
        project_id: String,
        path: PathBuf,
    ) -> Result<()> {
        self.registry
            .watch_repository(attachment_id, project_id, path)
            .await
    }

    /// Unregister a repository from watching
    pub async fn unwatch_repository(&self, attachment_id: &str) -> Result<()> {
        self.registry.unwatch_repository(attachment_id).await
    }

    /// Mark that a git operation just completed (to suppress redundant events)
    pub fn mark_git_operation(&self, attachment_id: &str) {
        self.registry.mark_git_operation(attachment_id);
    }

    /// Get the watch registry for external access
    pub fn registry(&self) -> Arc<WatchRegistry> {
        self.registry.clone()
    }

    /// Register all existing imported repositories for watching
    ///
    /// This scans the database for repositories with import_status = 'complete'
    /// and registers them with the file watcher.
    pub async fn register_existing_repositories(&self) -> Result<usize> {
        info!("Scanning for existing repositories to watch");

        // Query all completed attachments
        let rows: Vec<(String, String, String)> = sqlx::query_as(
            r#"
            SELECT id, project_id, local_path
            FROM git_repo_attachments
            WHERE import_status = 'complete'
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        let mut registered = 0;

        for (attachment_id, project_id, local_path) in rows {
            let path = PathBuf::from(&local_path);

            // Skip if path doesn't exist
            if !path.exists() {
                warn!(
                    "Skipping non-existent repository path: {} (attachment: {})",
                    local_path, attachment_id
                );
                continue;
            }

            // Register with watcher
            match self
                .watch_repository(attachment_id.clone(), project_id.clone(), path)
                .await
            {
                Ok(()) => {
                    debug!(
                        "Registered repository for watching: {} (project: {})",
                        attachment_id, project_id
                    );
                    registered += 1;
                }
                Err(e) => {
                    warn!(
                        "Failed to register repository {} for watching: {}",
                        attachment_id, e
                    );
                }
            }
        }

        info!("Registered {} existing repositories for file watching", registered);
        Ok(registered)
    }
}
