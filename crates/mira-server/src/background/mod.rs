// crates/mira-server/src/background/mod.rs
// Background workers for idle-time processing
//
// Split into two lanes:
// - Fast lane: embeddings/indexing (woken immediately by Notify)
// - Slow lane: LLM tasks (summaries, pondering, code health)

mod briefings;
mod capabilities;
pub mod diff_analysis;
pub mod documentation;
mod embeddings;
mod fast_lane;
mod pondering;
mod slow_lane;
mod summaries;
pub mod code_health;
pub mod watcher;

use crate::db::pool::DatabasePool;
use crate::embeddings::EmbeddingClient;
use crate::llm::ProviderFactory;
use std::sync::Arc;
use tokio::sync::{watch, Notify};

pub use fast_lane::FastLaneWorker;
pub use slow_lane::SlowLaneWorker;

/// Handle for waking the fast lane worker
#[derive(Clone)]
pub struct FastLaneNotify {
    notify: Arc<Notify>,
}

impl FastLaneNotify {
    /// Wake the fast lane worker to process pending embeddings
    pub fn wake(&self) {
        self.notify.notify_one();
    }
}

/// Spawn both background workers
///
/// Returns:
/// - shutdown sender (send true to stop all workers)
/// - fast lane notify handle (call .wake() after queuing embeddings)
pub fn spawn(
    pool: Arc<DatabasePool>,
    embeddings: Option<Arc<EmbeddingClient>>,
    llm_factory: Arc<ProviderFactory>,
) -> (watch::Sender<bool>, FastLaneNotify) {
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let notify = Arc::new(Notify::new());

    // Spawn fast lane worker (embeddings)
    let fast_lane = FastLaneWorker::new(
        pool.clone(),
        embeddings,
        shutdown_rx.clone(),
        notify.clone(),
    );
    tokio::spawn(async move {
        fast_lane.run().await;
    });

    // Spawn slow lane worker (LLM tasks)
    let slow_lane = SlowLaneWorker::new(
        pool,
        llm_factory,
        shutdown_rx,
    );
    tokio::spawn(async move {
        slow_lane.run().await;
    });

    let fast_lane_notify = FastLaneNotify { notify };

    (shutdown_tx, fast_lane_notify)
}

// Keep the old BackgroundWorker for backwards compatibility during transition
// TODO: Remove after confirming new workers work correctly

use crate::llm::LlmClient;
use std::time::Duration;

/// Legacy background worker (deprecated - use spawn() instead)
#[deprecated(note = "Use spawn() which creates FastLane and SlowLane workers")]
pub struct BackgroundWorker {
    pool: Arc<DatabasePool>,
    embeddings: Option<Arc<EmbeddingClient>>,
    llm_factory: Arc<ProviderFactory>,
    shutdown: watch::Receiver<bool>,
    cycle_count: u64,
}

#[allow(deprecated)]
impl BackgroundWorker {
    pub fn new(
        pool: Arc<DatabasePool>,
        embeddings: Option<Arc<EmbeddingClient>>,
        llm_factory: Arc<ProviderFactory>,
        shutdown: watch::Receiver<bool>,
    ) -> Self {
        Self { pool, embeddings, llm_factory, shutdown, cycle_count: 0 }
    }

    pub async fn run(mut self) {
        tracing::info!("Background worker started (legacy mode)");

        tokio::time::sleep(Duration::from_secs(30)).await;

        loop {
            if *self.shutdown.borrow() {
                tracing::info!("Background worker shutting down");
                break;
            }

            match self.process_batch().await {
                Ok(processed) if processed > 0 => {
                    tracing::info!("Background worker processed {} items", processed);
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
                Ok(_) => {
                    tokio::time::sleep(Duration::from_secs(60)).await;
                }
                Err(e) => {
                    tracing::warn!("Background worker error: {}", e);
                    tokio::time::sleep(Duration::from_secs(120)).await;
                }
            }

            if self.shutdown.has_changed().unwrap_or(false) && *self.shutdown.borrow() {
                break;
            }
        }
    }

    async fn process_batch(&mut self) -> Result<usize, String> {
        let mut processed = 0;
        self.cycle_count += 1;

        if let Some(ref emb) = self.embeddings {
            let count = self.process_pending_embeddings(emb).await?;
            if count > 0 {
                tracing::info!("Background: processed {} pending embeddings", count);
            }
            processed += count;
        }

        match self.llm_factory.client_for_role("background", &self.pool).await {
            Ok(client) => {
                let count = self.process_summary_queue(&client).await?;
                if count > 0 {
                    tracing::info!("Background: processed {} summaries", count);
                }
                processed += count;

                let count = self.process_briefings(&client).await?;
                if count > 0 {
                    tracing::info!("Background: processed {} briefings", count);
                }
                processed += count;

                let count = self.process_capabilities(&client).await?;
                if count > 0 {
                    tracing::info!("Background: processed {} capabilities", count);
                }
                processed += count;

                if self.cycle_count % 3 == 0 {
                    let count = self.process_documentation(&self.llm_factory).await?;
                    if count > 0 {
                        tracing::info!("Background: processed {} documentation tasks", count);
                    }
                    processed += count;
                }

                let count = self.process_code_health(&client).await?;
                if count > 0 {
                    tracing::info!("Background: processed {} health issues", count);
                }
                processed += count;

                if self.cycle_count % 10 == 0 {
                    let count = self.process_pondering(&client).await?;
                    if count > 0 {
                        tracing::info!("Background: generated {} pondering insights", count);
                    }
                    processed += count;
                }
            }
            Err(e) => {
                tracing::debug!("Background: no LLM provider available: {}", e);
            }
        }

        Ok(processed)
    }

    async fn process_summary_queue(&self, client: &Arc<dyn LlmClient>) -> Result<usize, String> {
        summaries::process_queue(&self.pool, client).await
    }

    async fn process_briefings(&self, client: &Arc<dyn LlmClient>) -> Result<usize, String> {
        briefings::process_briefings(&self.pool, client).await
    }

    async fn process_capabilities(&self, client: &Arc<dyn LlmClient>) -> Result<usize, String> {
        capabilities::process_capabilities(&self.pool, client, self.embeddings.as_ref()).await
    }

    async fn process_code_health(&self, client: &Arc<dyn LlmClient>) -> Result<usize, String> {
        code_health::process_code_health(&self.pool, Some(client)).await
    }

    async fn process_documentation(&self, client: &Arc<ProviderFactory>) -> Result<usize, String> {
        documentation::process_documentation(&self.pool, client).await
    }

    async fn process_pending_embeddings(&self, client: &Arc<EmbeddingClient>) -> Result<usize, String> {
        embeddings::process_pending_embeddings(&self.pool, Some(client)).await
    }

    async fn process_pondering(&self, client: &Arc<dyn LlmClient>) -> Result<usize, String> {
        pondering::process_pondering(&self.pool, client).await
    }
}
