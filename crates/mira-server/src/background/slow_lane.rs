// crates/mira-server/src/background/slow_lane.rs
// Slow lane worker for LLM-dependent tasks (summaries, pondering, code health)
//
// These tasks are less time-sensitive and can run on a longer interval
// without blocking the fast lane.

use crate::db::pool::DatabasePool;
use crate::embeddings::EmbeddingClient;
use crate::llm::{LlmClient, ProviderFactory};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;
use tokio::time::timeout;

use super::{
    briefings, code_health, documentation, entity_extraction, memory_embeddings, outcome_scanner,
    pondering, session_summaries, summaries, team_monitor,
};

/// Delay before first cycle to let the service start up
const INITIAL_DELAY_SECS: u64 = 30;
/// Delay between batches when there's active work
const ACTIVE_DELAY_SECS: u64 = 10;
/// Delay when no work is found (idle polling)
const IDLE_DELAY_SECS: u64 = 60;
/// Run documentation tasks every Nth cycle
const DOCUMENTATION_CYCLE_INTERVAL: u64 = 3;
/// Run pondering tasks every Nth cycle
const PONDERING_CYCLE_INTERVAL: u64 = 10;
/// Run outcome scanning every Nth cycle
const OUTCOME_SCAN_CYCLE_INTERVAL: u64 = 5;
/// Run team monitoring every Nth cycle
const TEAM_MONITOR_CYCLE_INTERVAL: u64 = 3;
/// Maximum time a single background task is allowed to run before being cancelled
const TASK_TIMEOUT_SECS: u64 = 120;

/// Slow lane worker for LLM-dependent background tasks
pub struct SlowLaneWorker {
    /// Main database pool (sessions, memory, LLM usage, etc.)
    pool: Arc<DatabasePool>,
    /// Code index database pool (code_symbols, vec_code, codebase_modules, etc.)
    code_pool: Arc<DatabasePool>,
    /// Embedding client for memory re-embedding
    embeddings: Option<Arc<EmbeddingClient>>,
    llm_factory: Arc<ProviderFactory>,
    shutdown: watch::Receiver<bool>,
    cycle_count: u64,
}

impl SlowLaneWorker {
    pub fn new(
        pool: Arc<DatabasePool>,
        code_pool: Arc<DatabasePool>,
        embeddings: Option<Arc<EmbeddingClient>>,
        llm_factory: Arc<ProviderFactory>,
        shutdown: watch::Receiver<bool>,
    ) -> Self {
        Self {
            pool,
            code_pool,
            embeddings,
            llm_factory,
            shutdown,
            cycle_count: 0,
        }
    }

    /// Run the slow lane worker loop
    pub async fn run(mut self) {
        tracing::info!("Slow lane worker started");

        // Initial delay to let the service start up
        tokio::time::sleep(Duration::from_secs(INITIAL_DELAY_SECS)).await;

        loop {
            // Check for shutdown
            if *self.shutdown.borrow() {
                tracing::info!("Slow lane worker shutting down");
                break;
            }

            // Process LLM-dependent tasks (errors are isolated per-subsystem)
            let processed = self.process_batch().await;
            if processed > 0 {
                tracing::info!("Slow lane: processed {} items", processed);
                // Short delay between batches when there's work
                tokio::time::sleep(Duration::from_secs(ACTIVE_DELAY_SECS)).await;
            } else {
                // No work found, sleep longer
                tokio::time::sleep(Duration::from_secs(IDLE_DELAY_SECS)).await;
            }

            // Check shutdown again before next iteration
            if self.shutdown.has_changed().unwrap_or(false) && *self.shutdown.borrow() {
                break;
            }
        }
    }

    /// Process a batch of background tasks.
    /// Each subsystem error is isolated — a failure in one task does not prevent others
    /// from running, and does not trigger global backoff.
    /// LLM client is optional — tasks produce heuristic/template fallbacks when absent.
    async fn process_batch(&mut self) -> usize {
        let mut processed = 0;

        // Increment cycle counter
        self.cycle_count += 1;

        // Get LLM client for background tasks (optional — fallbacks used when absent)
        let client: Option<Arc<dyn LlmClient>> = self
            .llm_factory
            .client_for_background();

        if client.is_none() {
            tracing::debug!("Slow lane: no LLM provider available, using fallbacks");
        }

        let client = client.as_ref();

        processed += Self::run_task(
            "stale sessions",
            session_summaries::process_stale_sessions(&self.pool, client),
        )
        .await;

        processed += Self::run_task(
            "summaries",
            summaries::process_queue(&self.code_pool, &self.pool, client),
        )
        .await;

        processed += Self::run_task(
            "briefings",
            briefings::process_briefings(&self.pool, client),
        )
        .await;

        if self
            .cycle_count
            .is_multiple_of(DOCUMENTATION_CYCLE_INTERVAL)
        {
            processed += Self::run_task(
                "documentation tasks",
                documentation::process_documentation(
                    &self.pool,
                    &self.code_pool,
                    &self.llm_factory,
                ),
            )
            .await;
        }

        processed += Self::run_task(
            "health issues",
            code_health::process_code_health(&self.pool, &self.code_pool, client),
        )
        .await;

        if self.cycle_count.is_multiple_of(PONDERING_CYCLE_INTERVAL) {
            processed += Self::run_task(
                "pondering insights",
                pondering::process_pondering(&self.pool, client),
            )
            .await;
        }

        if self.cycle_count.is_multiple_of(OUTCOME_SCAN_CYCLE_INTERVAL) {
            processed += Self::run_task(
                "diff outcomes",
                outcome_scanner::process_outcome_scanning(&self.pool, &self.code_pool),
            )
            .await;
        }

        if self.cycle_count.is_multiple_of(TEAM_MONITOR_CYCLE_INTERVAL) {
            processed += Self::run_task(
                "team monitor",
                team_monitor::process_team_monitor(&self.pool),
            )
            .await;
        }

        processed += Self::run_task(
            "proactive items",
            crate::proactive::background::process_proactive(&self.pool, client, self.cycle_count),
        )
        .await;

        processed += Self::run_task(
            "entity backfills",
            entity_extraction::process_entity_backfill(&self.pool),
        )
        .await;

        // Re-embed memory facts that need embeddings (after provider change or new facts)
        if let Some(ref emb) = self.embeddings {
            processed += Self::run_task(
                "memory embeddings",
                memory_embeddings::process_memory_embeddings(&self.pool, emb),
            )
            .await;
        }

        processed
    }

    /// Run a background task with a timeout. Errors and timeouts are caught and
    /// logged so that one failing subsystem cannot starve others.
    async fn run_task(
        name: &str,
        fut: impl std::future::Future<Output = Result<usize, String>>,
    ) -> usize {
        match timeout(Duration::from_secs(TASK_TIMEOUT_SECS), fut).await {
            Ok(Ok(count)) => {
                if count > 0 {
                    tracing::info!("Slow lane: processed {} {}", count, name);
                }
                count
            }
            Ok(Err(e)) => {
                tracing::warn!("Slow lane task '{}' failed: {}", name, e);
                0
            }
            Err(_) => {
                tracing::warn!(
                    "Slow lane task '{}' timed out after {}s",
                    name,
                    TASK_TIMEOUT_SECS
                );
                0
            }
        }
    }
}
