// crates/mira-server/src/background/slow_lane.rs
// Slow lane worker for LLM-dependent tasks (summaries, pondering, code health)
//
// Tasks are assigned priority levels and executed in priority order.
// When the previous cycle ran long, low-priority tasks are skipped.

use crate::db::pool::DatabasePool;
use crate::embeddings::EmbeddingClient;
use crate::llm::{LlmClient, ProviderFactory};
use std::sync::Arc;
use std::time::{Duration, Instant};
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
/// Maximum time a single background task is allowed to run before being cancelled
const TASK_TIMEOUT_SECS: u64 = 120;
/// If the previous cycle took longer than this, skip low-priority tasks
const LONG_CYCLE_THRESHOLD_SECS: u64 = 60;

/// Run documentation tasks every Nth cycle
const DOCUMENTATION_CYCLE_INTERVAL: u64 = 3;
/// Run pondering tasks every Nth cycle
const PONDERING_CYCLE_INTERVAL: u64 = 10;
/// Run outcome scanning every Nth cycle
const OUTCOME_SCAN_CYCLE_INTERVAL: u64 = 5;
/// Run team monitoring every Nth cycle
const TEAM_MONITOR_CYCLE_INTERVAL: u64 = 3;

/// Priority level for background tasks. Higher priority runs first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum TaskPriority {
    /// Must always run: session summaries, embeddings
    Critical = 0,
    /// Standard tasks: briefings, health, proactive
    Normal = 1,
    /// Deferrable under load: documentation, pondering, outcome scanning
    Low = 2,
}

/// A scheduled background task with metadata for priority ordering.
struct ScheduledTask {
    name: &'static str,
    priority: TaskPriority,
    /// None = run every cycle; Some(n) = run every nth cycle
    cycle_interval: Option<u64>,
}

impl ScheduledTask {
    /// Whether this task should run on the given cycle.
    fn should_run(&self, cycle: u64) -> bool {
        match self.cycle_interval {
            None => true,
            Some(interval) => cycle.is_multiple_of(interval),
        }
    }
}

/// Static task schedule. Order within the same priority is preserved.
fn task_schedule() -> Vec<ScheduledTask> {
    vec![
        // Critical: always run
        ScheduledTask { name: "stale sessions",     priority: TaskPriority::Critical, cycle_interval: None },
        ScheduledTask { name: "memory embeddings",  priority: TaskPriority::Critical, cycle_interval: None },
        // Normal: standard cadence
        ScheduledTask { name: "summaries",          priority: TaskPriority::Normal,   cycle_interval: None },
        ScheduledTask { name: "briefings",          priority: TaskPriority::Normal,   cycle_interval: None },
        ScheduledTask { name: "health issues",      priority: TaskPriority::Normal,   cycle_interval: None },
        ScheduledTask { name: "proactive items",    priority: TaskPriority::Normal,   cycle_interval: None },
        ScheduledTask { name: "entity backfills",   priority: TaskPriority::Normal,   cycle_interval: None },
        ScheduledTask { name: "team monitor",       priority: TaskPriority::Normal,   cycle_interval: Some(TEAM_MONITOR_CYCLE_INTERVAL) },
        // Low: deferrable under load
        ScheduledTask { name: "documentation tasks",priority: TaskPriority::Low,      cycle_interval: Some(DOCUMENTATION_CYCLE_INTERVAL) },
        ScheduledTask { name: "pondering insights", priority: TaskPriority::Low,      cycle_interval: Some(PONDERING_CYCLE_INTERVAL) },
        ScheduledTask { name: "insight cleanup",    priority: TaskPriority::Low,      cycle_interval: Some(PONDERING_CYCLE_INTERVAL) },
        ScheduledTask { name: "proactive cleanup",  priority: TaskPriority::Low,      cycle_interval: Some(PONDERING_CYCLE_INTERVAL) },
        ScheduledTask { name: "diff outcomes",      priority: TaskPriority::Low,      cycle_interval: Some(OUTCOME_SCAN_CYCLE_INTERVAL) },
    ]
}

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
    /// Duration of the previous cycle, used to decide whether to skip low-priority tasks
    last_cycle_duration: Duration,
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
            last_cycle_duration: Duration::ZERO,
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
            let start = Instant::now();
            let processed = self.process_batch().await;
            self.last_cycle_duration = start.elapsed();

            if processed > 0 {
                tracing::info!(
                    "Slow lane: processed {} items in {:.1}s",
                    processed,
                    self.last_cycle_duration.as_secs_f64()
                );
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

    /// Whether low-priority tasks should be skipped this cycle.
    fn skip_low_priority(&self) -> bool {
        self.last_cycle_duration.as_secs() >= LONG_CYCLE_THRESHOLD_SECS
    }

    /// Process a batch of background tasks.
    /// Each subsystem error is isolated -- a failure in one task does not prevent others
    /// from running, and does not trigger global backoff.
    /// LLM client is optional -- tasks produce heuristic/template fallbacks when absent.
    async fn process_batch(&mut self) -> usize {
        let mut processed = 0;

        // Increment cycle counter
        self.cycle_count += 1;

        let skip_low = self.skip_low_priority();
        if skip_low {
            tracing::info!(
                "Slow lane: previous cycle took {:.1}s (>{LONG_CYCLE_THRESHOLD_SECS}s), skipping low-priority tasks",
                self.last_cycle_duration.as_secs_f64()
            );
        }

        // Get LLM client for background tasks (optional -- fallbacks used when absent)
        let client: Option<Arc<dyn LlmClient>> = self.llm_factory.client_for_background();

        if client.is_none() {
            tracing::debug!("Slow lane: no LLM provider available, using fallbacks");
        }

        let client = client.as_ref();

        // Walk the schedule in definition order (already grouped by priority)
        for task in task_schedule() {
            // Skip tasks not due this cycle
            if !task.should_run(self.cycle_count) {
                continue;
            }

            // Skip low-priority tasks when under load
            if skip_low && task.priority == TaskPriority::Low {
                tracing::debug!("Slow lane: skipping low-priority task '{}'", task.name);
                continue;
            }

            processed += self.dispatch_task(task.name, client).await;
        }

        processed
    }

    /// Dispatch a named task to its implementation.
    async fn dispatch_task(
        &self,
        name: &str,
        client: Option<&Arc<dyn LlmClient>>,
    ) -> usize {
        match name {
            "stale sessions" => {
                Self::run_task(
                    name,
                    session_summaries::process_stale_sessions(&self.pool, client),
                )
                .await
            }
            "summaries" => {
                Self::run_task(
                    name,
                    summaries::process_queue(&self.code_pool, &self.pool, client),
                )
                .await
            }
            "briefings" => {
                Self::run_task(
                    name,
                    briefings::process_briefings(&self.pool, client),
                )
                .await
            }
            "documentation tasks" => {
                Self::run_task(
                    name,
                    documentation::process_documentation(
                        &self.pool,
                        &self.code_pool,
                        &self.llm_factory,
                    ),
                )
                .await
            }
            "health issues" => {
                Self::run_task(
                    name,
                    code_health::process_code_health(&self.pool, &self.code_pool, client),
                )
                .await
            }
            "pondering insights" => {
                Self::run_task(
                    name,
                    pondering::process_pondering(&self.pool, client),
                )
                .await
            }
            "insight cleanup" => {
                Self::run_task(
                    name,
                    pondering::cleanup_stale_insights(&self.pool),
                )
                .await
            }
            "proactive cleanup" => {
                Self::run_task(
                    name,
                    crate::proactive::background::cleanup_expired_suggestions(&self.pool),
                )
                .await
            }
            "diff outcomes" => {
                Self::run_task(
                    name,
                    outcome_scanner::process_outcome_scanning(&self.pool, &self.code_pool),
                )
                .await
            }
            "team monitor" => {
                Self::run_task(
                    name,
                    team_monitor::process_team_monitor(&self.pool),
                )
                .await
            }
            "proactive items" => {
                Self::run_task(
                    name,
                    crate::proactive::background::process_proactive(
                        &self.pool,
                        client,
                        self.cycle_count,
                    ),
                )
                .await
            }
            "entity backfills" => {
                Self::run_task(
                    name,
                    entity_extraction::process_entity_backfill(&self.pool),
                )
                .await
            }
            "memory embeddings" => {
                if let Some(ref emb) = self.embeddings {
                    Self::run_task(
                        name,
                        memory_embeddings::process_memory_embeddings(&self.pool, emb),
                    )
                    .await
                } else {
                    0
                }
            }
            _ => {
                tracing::warn!("Slow lane: unknown task '{}'", name);
                0
            }
        }
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
