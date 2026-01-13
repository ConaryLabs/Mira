// crates/mira-server/src/background/mod.rs
// Background worker for idle-time processing

mod summaries;
mod briefings;
mod capabilities;
pub mod code_health;
pub mod watcher;

use crate::db::Database;
use crate::embeddings::EmbeddingClient;
use crate::llm::DeepSeekClient;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;

/// Background worker configuration
pub struct BackgroundWorker {
    db: Arc<Database>,
    embeddings: Option<Arc<EmbeddingClient>>,
    deepseek: Option<Arc<DeepSeekClient>>,
    shutdown: watch::Receiver<bool>,
}

impl BackgroundWorker {
    pub fn new(
        db: Arc<Database>,
        embeddings: Option<Arc<EmbeddingClient>>,
        deepseek: Option<Arc<DeepSeekClient>>,
        shutdown: watch::Receiver<bool>,
    ) -> Self {
        Self { db, embeddings, deepseek, shutdown }
    }

    /// Start the background worker loop
    pub async fn run(self) {
        tracing::info!("Background worker started");

        // Initial delay to let the service start up
        tokio::time::sleep(Duration::from_secs(30)).await;

        loop {
            // Check for shutdown
            if *self.shutdown.borrow() {
                tracing::info!("Background worker shutting down");
                break;
            }

            // Scan for work
            match self.process_batch().await {
                Ok(processed) if processed > 0 => {
                    tracing::info!("Background worker processed {} items", processed);
                    // Short delay between batches when there's work
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
                Ok(_) => {
                    // No work found, sleep longer
                    tokio::time::sleep(Duration::from_secs(60)).await;
                }
                Err(e) => {
                    tracing::warn!("Background worker error: {}", e);
                    // Back off on errors
                    tokio::time::sleep(Duration::from_secs(120)).await;
                }
            }

            // Check shutdown again before next iteration
            if self.shutdown.has_changed().unwrap_or(false) && *self.shutdown.borrow() {
                break;
            }
        }
    }

    /// Process a batch of work items
    async fn process_batch(&self) -> Result<usize, String> {
        let mut processed = 0;

        // Process summaries one at a time (rate limited)
        if self.deepseek.is_some() {
            let count = self.process_summary_queue().await?;
            if count > 0 {
                tracing::info!("Background: processed {} summaries", count);
            }
            processed += count;
        }

        // Process project briefings (What's New since last session)
        if self.deepseek.is_some() {
            let count = self.process_briefings().await?;
            if count > 0 {
                tracing::info!("Background: processed {} briefings", count);
            }
            processed += count;
        }

        // Process capabilities inventory (periodic codebase scan)
        if self.deepseek.is_some() {
            let count = self.process_capabilities().await?;
            if count > 0 {
                tracing::info!("Background: processed {} capabilities", count);
            }
            processed += count;
        }

        // Process code health (cargo warnings, TODOs, unused functions)
        let count = self.process_code_health().await?;
        if count > 0 {
            tracing::info!("Background: processed {} health issues", count);
        }
        processed += count;

        Ok(processed)
    }

    /// Process summaries with rate limiting
    async fn process_summary_queue(&self) -> Result<usize, String> {
        summaries::process_queue(&self.db, self.deepseek.as_ref().unwrap()).await
    }

    /// Process project briefings (What's New since last session)
    async fn process_briefings(&self) -> Result<usize, String> {
        briefings::process_briefings(&self.db, self.deepseek.as_ref().unwrap()).await
    }

    /// Process capabilities inventory (periodic codebase scan)
    async fn process_capabilities(&self) -> Result<usize, String> {
        capabilities::process_capabilities(
            &self.db,
            self.deepseek.as_ref().unwrap(),
            self.embeddings.as_ref(),
        ).await
    }

    /// Process code health (compiler warnings, TODOs, unused code, complexity)
    async fn process_code_health(&self) -> Result<usize, String> {
        code_health::process_code_health(&self.db, self.deepseek.as_ref()).await
    }
}

/// Spawn the background worker
pub fn spawn(
    db: Arc<Database>,
    embeddings: Option<Arc<EmbeddingClient>>,
    deepseek: Option<Arc<DeepSeekClient>>,
) -> watch::Sender<bool> {
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    let worker = BackgroundWorker::new(db, embeddings, deepseek, shutdown_rx);

    tokio::spawn(async move {
        worker.run().await;
    });

    shutdown_tx
}
