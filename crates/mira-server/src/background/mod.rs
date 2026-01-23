// crates/mira-server/src/background/mod.rs
// Background worker for idle-time processing

mod briefings;
mod capabilities;
pub mod diff_analysis;
pub mod documentation;
mod embeddings;
mod summaries;
pub mod code_health;
pub mod watcher;

use crate::db::Database;
use crate::embeddings::EmbeddingClient;
use crate::llm::{DeepSeekClient, ProviderFactory};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;

/// Background worker configuration
pub struct BackgroundWorker {
    db: Arc<Database>,
    embeddings: Option<Arc<EmbeddingClient>>,
    deepseek: Option<Arc<DeepSeekClient>>,
    llm_factory: Arc<ProviderFactory>,
    shutdown: watch::Receiver<bool>,
    cycle_count: u64,
}

impl BackgroundWorker {
    pub fn new(
        db: Arc<Database>,
        embeddings: Option<Arc<EmbeddingClient>>,
        deepseek: Option<Arc<DeepSeekClient>>,
        shutdown: watch::Receiver<bool>,
    ) -> Self {
        // Create provider factory with all available LLM clients
        let llm_factory = Arc::new(ProviderFactory::new());

        Self { db, embeddings, deepseek, llm_factory, shutdown, cycle_count: 0 }
    }

    /// Start the background worker loop
    pub async fn run(mut self) {
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
    async fn process_batch(&mut self) -> Result<usize, String> {
        let mut processed = 0;

        // Increment cycle counter
        self.cycle_count += 1;

        // Process pending embeddings first (highest priority - enables search for new files)
        if let Some(ref emb) = self.embeddings {
            let count = self.process_pending_embeddings(emb).await?;
            if count > 0 {
                tracing::info!("Background: processed {} pending embeddings", count);
            }
            processed += count;
        }

        // Process DeepSeek-dependent tasks (summaries, briefings, capabilities)
        if let Some(ref ds) = self.deepseek {
            // Process summaries one at a time (rate limited)
            let count = self.process_summary_queue(ds).await?;
            if count > 0 {
                tracing::info!("Background: processed {} summaries", count);
            }
            processed += count;

            // Process project briefings (What's New since last session)
            let count = self.process_briefings(ds).await?;
            if count > 0 {
                tracing::info!("Background: processed {} briefings", count);
            }
            processed += count;

            // Process capabilities inventory (periodic codebase scan)
            let count = self.process_capabilities(ds).await?;
            if count > 0 {
                tracing::info!("Background: processed {} capabilities", count);
            }
            processed += count;

            // Process documentation tasks (lower priority - run every 3rd cycle)
            if self.cycle_count % 3 == 0 {
                let count = self.process_documentation(&self.llm_factory).await?;
                if count > 0 {
                    tracing::info!("Background: processed {} documentation tasks", count);
                }
                processed += count;
            }
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
    async fn process_summary_queue(&self, client: &Arc<DeepSeekClient>) -> Result<usize, String> {
        summaries::process_queue(&self.db, client).await
    }

    /// Process project briefings (What's New since last session)
    async fn process_briefings(&self, client: &Arc<DeepSeekClient>) -> Result<usize, String> {
        briefings::process_briefings(&self.db, client).await
    }

    /// Process capabilities inventory (periodic codebase scan)
    async fn process_capabilities(&self, client: &Arc<DeepSeekClient>) -> Result<usize, String> {
        capabilities::process_capabilities(
            &self.db,
            client,
            self.embeddings.as_ref(),
        ).await
    }

    /// Process code health (compiler warnings, TODOs, unused code, complexity)
    async fn process_code_health(&self) -> Result<usize, String> {
        code_health::process_code_health(&self.db, self.deepseek.as_ref()).await
    }

    /// Process documentation tasks (gap detection and draft generation)
    async fn process_documentation(&self, client: &Arc<ProviderFactory>) -> Result<usize, String> {
        documentation::process_documentation(&self.db, client).await
    }

    /// Process pending embeddings from file watcher queue
    async fn process_pending_embeddings(&self, client: &Arc<EmbeddingClient>) -> Result<usize, String> {
        embeddings::process_pending_embeddings(&self.db, Some(client)).await
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
