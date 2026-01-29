// crates/mira-server/src/background/fast_lane.rs
// Fast lane worker for time-sensitive tasks (embeddings, file indexing)
//
// This worker is woken immediately when new work is available via Notify,
// ensuring files become searchable as quickly as possible.

use crate::db::pool::DatabasePool;
use crate::embeddings::EmbeddingClient;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Notify, watch};

use super::embeddings;

/// Delay before first cycle
const INITIAL_DELAY_SECS: u64 = 5;
/// Delay between batches when there's active work
const ACTIVE_DELAY_MS: u64 = 100;
/// Periodic check interval when idle (no notification)
const IDLE_CHECK_SECS: u64 = 10;

/// Fast lane worker for time-sensitive background tasks
pub struct FastLaneWorker {
    pool: Arc<DatabasePool>,
    embeddings: Option<Arc<EmbeddingClient>>,
    shutdown: watch::Receiver<bool>,
    notify: Arc<Notify>,
}

impl FastLaneWorker {
    pub fn new(
        pool: Arc<DatabasePool>,
        embeddings: Option<Arc<EmbeddingClient>>,
        shutdown: watch::Receiver<bool>,
        notify: Arc<Notify>,
    ) -> Self {
        Self {
            pool,
            embeddings,
            shutdown,
            notify,
        }
    }

    /// Run the fast lane worker loop
    pub async fn run(mut self) {
        tracing::info!("Fast lane worker started");

        // Short initial delay
        tokio::time::sleep(Duration::from_secs(INITIAL_DELAY_SECS)).await;

        loop {
            // Check for shutdown
            if *self.shutdown.borrow() {
                tracing::info!("Fast lane worker shutting down");
                break;
            }

            // Process any pending embeddings
            let processed = self.process_embeddings().await;

            if processed > 0 {
                tracing::info!("Fast lane: processed {} embeddings", processed);
                // Quick loop back if there's work
                tokio::time::sleep(Duration::from_millis(ACTIVE_DELAY_MS)).await;
            } else {
                // Wait for notification or timeout
                tokio::select! {
                    _ = self.notify.notified() => {
                        tracing::debug!("Fast lane: woken by notify");
                    }
                    _ = tokio::time::sleep(Duration::from_secs(IDLE_CHECK_SECS)) => {
                        // Periodic check even without notification
                    }
                    _ = self.shutdown.changed() => {
                        if *self.shutdown.borrow() {
                            break;
                        }
                    }
                }
            }
        }
    }

    /// Process pending embeddings
    async fn process_embeddings(&self) -> usize {
        if let Some(ref emb) = self.embeddings {
            match embeddings::process_pending_embeddings(&self.pool, Some(emb)).await {
                Ok(count) => count,
                Err(e) => {
                    tracing::warn!("Fast lane embedding error: {}", e);
                    0
                }
            }
        } else {
            0
        }
    }
}
