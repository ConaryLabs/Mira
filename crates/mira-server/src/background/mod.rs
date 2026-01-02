// crates/mira-server/src/background/mod.rs
// Background worker for idle-time processing

mod scanner;
mod embeddings;
mod summaries;
pub mod watcher;

use crate::db::Database;
use crate::embeddings::EmbeddingClient;
use crate::web::deepseek::DeepSeekClient;
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

/// Work item types
#[derive(Debug)]
pub enum WorkItem {
    /// File needs embedding (project_id, file_path, content)
    Embedding { project_id: i64, file_path: String, content: String },
    /// Module needs summary (project_id, module_id, module_path)
    Summary { project_id: i64, module_id: i64, module_path: String },
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

        // First, check for pending embeddings (use batch API)
        if self.embeddings.is_some() {
            processed += self.process_embedding_batch().await?;
        }

        // Then, process summaries one at a time (rate limited)
        if self.deepseek.is_some() {
            processed += self.process_summary_queue().await?;
        }

        Ok(processed)
    }

    /// Process embeddings using OpenAI Batch API
    async fn process_embedding_batch(&self) -> Result<usize, String> {
        embeddings::process_batch(&self.db, self.embeddings.as_ref().unwrap()).await
    }

    /// Process summaries with rate limiting
    async fn process_summary_queue(&self) -> Result<usize, String> {
        summaries::process_queue(&self.db, self.deepseek.as_ref().unwrap()).await
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
