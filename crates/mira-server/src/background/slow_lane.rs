// crates/mira-server/src/background/slow_lane.rs
// Slow lane worker for LLM-dependent tasks (summaries, pondering, code health)
//
// These tasks are less time-sensitive and can run on a longer interval
// without blocking the fast lane.

use crate::db::pool::DatabasePool;
use crate::llm::{LlmClient, ProviderFactory};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;

use super::{briefings, capabilities, code_health, documentation, pondering, session_summaries, summaries};

/// Slow lane worker for LLM-dependent background tasks
pub struct SlowLaneWorker {
    pool: Arc<DatabasePool>,
    llm_factory: Arc<ProviderFactory>,
    shutdown: watch::Receiver<bool>,
    cycle_count: u64,
}

impl SlowLaneWorker {
    pub fn new(
        pool: Arc<DatabasePool>,
        llm_factory: Arc<ProviderFactory>,
        shutdown: watch::Receiver<bool>,
    ) -> Self {
        Self {
            pool,
            llm_factory,
            shutdown,
            cycle_count: 0,
        }
    }

    /// Run the slow lane worker loop
    pub async fn run(mut self) {
        tracing::info!("Slow lane worker started");

        // Initial delay to let the service start up
        tokio::time::sleep(Duration::from_secs(30)).await;

        loop {
            // Check for shutdown
            if *self.shutdown.borrow() {
                tracing::info!("Slow lane worker shutting down");
                break;
            }

            // Process LLM-dependent tasks
            match self.process_batch().await {
                Ok(processed) if processed > 0 => {
                    tracing::info!("Slow lane: processed {} items", processed);
                    // Short delay between batches when there's work
                    tokio::time::sleep(Duration::from_secs(10)).await;
                }
                Ok(_) => {
                    // No work found, sleep longer
                    tokio::time::sleep(Duration::from_secs(60)).await;
                }
                Err(e) => {
                    tracing::warn!("Slow lane error: {}", e);
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

    /// Process a batch of LLM-dependent tasks
    async fn process_batch(&mut self) -> Result<usize, String> {
        let mut processed = 0;

        // Increment cycle counter
        self.cycle_count += 1;

        // Get LLM client for background tasks
        let client = match self
            .llm_factory
            .client_for_role("background", &self.pool)
            .await
        {
            Ok(c) => c,
            Err(e) => {
                tracing::debug!("Slow lane: no LLM provider available: {}", e);
                return Ok(0);
            }
        };

        // Process stale sessions (close and summarize)
        processed += self.process_stale_sessions(&client).await?;

        // Process summaries (rate limited)
        processed += self.process_summaries(&client).await?;

        // Process project briefings
        processed += self.process_briefings(&client).await?;

        // Process capabilities inventory
        processed += self.process_capabilities(&client).await?;

        // Process documentation tasks (every 3rd cycle)
        if self.cycle_count.is_multiple_of(3) {
            processed += self.process_documentation().await?;
        }

        // Process code health
        processed += self.process_code_health(&client).await?;

        // Process pondering (every 10th cycle)
        if self.cycle_count.is_multiple_of(10) {
            processed += self.process_pondering(&client).await?;
        }

        Ok(processed)
    }

    async fn process_summaries(&self, client: &Arc<dyn LlmClient>) -> Result<usize, String> {
        let count = summaries::process_queue(&self.pool, client).await?;
        if count > 0 {
            tracing::info!("Slow lane: processed {} summaries", count);
        }
        Ok(count)
    }

    async fn process_briefings(&self, client: &Arc<dyn LlmClient>) -> Result<usize, String> {
        let count = briefings::process_briefings(&self.pool, client).await?;
        if count > 0 {
            tracing::info!("Slow lane: processed {} briefings", count);
        }
        Ok(count)
    }

    async fn process_capabilities(&self, client: &Arc<dyn LlmClient>) -> Result<usize, String> {
        let count = capabilities::process_capabilities(&self.pool, client, None).await?;
        if count > 0 {
            tracing::info!("Slow lane: processed {} capabilities", count);
        }
        Ok(count)
    }

    async fn process_documentation(&self) -> Result<usize, String> {
        let count = documentation::process_documentation(&self.pool, &self.llm_factory).await?;
        if count > 0 {
            tracing::info!("Slow lane: processed {} documentation tasks", count);
        }
        Ok(count)
    }

    async fn process_code_health(&self, client: &Arc<dyn LlmClient>) -> Result<usize, String> {
        let count = code_health::process_code_health(&self.pool, Some(client)).await?;
        if count > 0 {
            tracing::info!("Slow lane: processed {} health issues", count);
        }
        Ok(count)
    }

    async fn process_pondering(&self, client: &Arc<dyn LlmClient>) -> Result<usize, String> {
        let count = pondering::process_pondering(&self.pool, client).await?;
        if count > 0 {
            tracing::info!("Slow lane: generated {} pondering insights", count);
        }
        Ok(count)
    }

    async fn process_stale_sessions(&self, client: &Arc<dyn LlmClient>) -> Result<usize, String> {
        let count = session_summaries::process_stale_sessions(&self.pool, client).await?;
        if count > 0 {
            tracing::info!("Slow lane: closed {} stale sessions", count);
        }
        Ok(count)
    }
}
