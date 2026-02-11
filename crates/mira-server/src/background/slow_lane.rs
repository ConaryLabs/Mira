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
/// Run LLM health analysis every Nth cycle (expensive, LLM-dependent)
const HEALTH_LLM_CYCLE_INTERVAL: u64 = 3;
/// Run data retention every Nth cycle (~10 min interval at 60s idle)
const DATA_RETENTION_CYCLE_INTERVAL: u64 = 10;

/// Priority level for background tasks. Lower numeric value = higher priority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TaskPriority {
    /// Must always run: session summaries, embeddings
    Critical = 0,
    /// Standard tasks: briefings, health, proactive
    Normal = 1,
    /// Deferrable under load: documentation, pondering, outcome scanning
    Low = 2,
}

/// Enumeration of all background task types.
/// Using an enum ensures compile-time exhaustiveness — adding a new task variant
/// without handling it in `dispatch_task` will cause a compiler error.
#[derive(Debug, Clone, Copy)]
enum BackgroundTask {
    StaleSessions,
    MemoryEmbeddings,
    Summaries,
    Briefings,
    HealthFastScans,
    HealthLlmComplexity,
    HealthLlmErrorQuality,
    HealthModuleAnalysis,
    ProactiveItems,
    EntityBackfills,
    TeamMonitor,
    DocumentationTasks,
    PonderingInsights,
    InsightCleanup,
    ProactiveCleanup,
    DiffOutcomes,
    DataRetention,
}

impl std::fmt::Display for BackgroundTask {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::StaleSessions => write!(f, "stale sessions"),
            Self::MemoryEmbeddings => write!(f, "memory embeddings"),
            Self::Summaries => write!(f, "summaries"),
            Self::Briefings => write!(f, "briefings"),
            Self::HealthFastScans => write!(f, "health: fast scans"),
            Self::HealthLlmComplexity => write!(f, "health: LLM complexity"),
            Self::HealthLlmErrorQuality => write!(f, "health: LLM error quality"),
            Self::HealthModuleAnalysis => write!(f, "health: module analysis"),
            Self::ProactiveItems => write!(f, "proactive items"),
            Self::EntityBackfills => write!(f, "entity backfills"),
            Self::TeamMonitor => write!(f, "team monitor"),
            Self::DocumentationTasks => write!(f, "documentation tasks"),
            Self::PonderingInsights => write!(f, "pondering insights"),
            Self::InsightCleanup => write!(f, "insight cleanup"),
            Self::ProactiveCleanup => write!(f, "proactive cleanup"),
            Self::DiffOutcomes => write!(f, "diff outcomes"),
            Self::DataRetention => write!(f, "data retention"),
        }
    }
}

/// A scheduled background task with metadata for priority ordering.
struct ScheduledTask {
    task: BackgroundTask,
    priority: TaskPriority,
    /// None = run every cycle; Some(n) = run every nth cycle
    cycle_interval: Option<u64>,
}

impl ScheduledTask {
    /// Whether this task should run on the given cycle.
    fn should_run(&self, cycle: u64) -> bool {
        match self.cycle_interval {
            None => true,
            Some(0) => true,
            Some(interval) => cycle.is_multiple_of(interval),
        }
    }
}

/// Static task schedule. Order within the same priority is preserved.
fn task_schedule() -> Vec<ScheduledTask> {
    vec![
        // Critical: always run
        ScheduledTask {
            task: BackgroundTask::StaleSessions,
            priority: TaskPriority::Critical,
            cycle_interval: None,
        },
        ScheduledTask {
            task: BackgroundTask::MemoryEmbeddings,
            priority: TaskPriority::Critical,
            cycle_interval: None,
        },
        // Normal: standard cadence
        ScheduledTask {
            task: BackgroundTask::Summaries,
            priority: TaskPriority::Normal,
            cycle_interval: None,
        },
        ScheduledTask {
            task: BackgroundTask::Briefings,
            priority: TaskPriority::Normal,
            cycle_interval: None,
        },
        ScheduledTask {
            task: BackgroundTask::HealthFastScans,
            priority: TaskPriority::Normal,
            cycle_interval: None,
        },
        ScheduledTask {
            task: BackgroundTask::HealthModuleAnalysis,
            priority: TaskPriority::Normal,
            cycle_interval: None,
        },
        ScheduledTask {
            task: BackgroundTask::HealthLlmComplexity,
            priority: TaskPriority::Low,
            cycle_interval: Some(HEALTH_LLM_CYCLE_INTERVAL),
        },
        ScheduledTask {
            task: BackgroundTask::HealthLlmErrorQuality,
            priority: TaskPriority::Low,
            cycle_interval: Some(HEALTH_LLM_CYCLE_INTERVAL),
        },
        ScheduledTask {
            task: BackgroundTask::ProactiveItems,
            priority: TaskPriority::Normal,
            cycle_interval: None,
        },
        ScheduledTask {
            task: BackgroundTask::EntityBackfills,
            priority: TaskPriority::Normal,
            cycle_interval: None,
        },
        ScheduledTask {
            task: BackgroundTask::TeamMonitor,
            priority: TaskPriority::Normal,
            cycle_interval: Some(TEAM_MONITOR_CYCLE_INTERVAL),
        },
        // Low: deferrable under load
        ScheduledTask {
            task: BackgroundTask::DocumentationTasks,
            priority: TaskPriority::Low,
            cycle_interval: Some(DOCUMENTATION_CYCLE_INTERVAL),
        },
        ScheduledTask {
            task: BackgroundTask::PonderingInsights,
            priority: TaskPriority::Low,
            cycle_interval: Some(PONDERING_CYCLE_INTERVAL),
        },
        ScheduledTask {
            task: BackgroundTask::InsightCleanup,
            priority: TaskPriority::Low,
            cycle_interval: Some(PONDERING_CYCLE_INTERVAL),
        },
        ScheduledTask {
            task: BackgroundTask::ProactiveCleanup,
            priority: TaskPriority::Low,
            cycle_interval: Some(PONDERING_CYCLE_INTERVAL),
        },
        ScheduledTask {
            task: BackgroundTask::DiffOutcomes,
            priority: TaskPriority::Low,
            cycle_interval: Some(OUTCOME_SCAN_CYCLE_INTERVAL),
        },
        ScheduledTask {
            task: BackgroundTask::DataRetention,
            priority: TaskPriority::Low,
            cycle_interval: Some(DATA_RETENTION_CYCLE_INTERVAL),
        },
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

        // Write heartbeat so the status line can detect if the background loop is alive.
        // This runs every cycle (~60s idle, ~10s active) regardless of what tasks run.
        let pool = self.pool.clone();
        pool.try_interact("heartbeat write", move |conn| {
            crate::db::set_server_state_sync(
                conn,
                "last_bg_heartbeat",
                &chrono::Utc::now().to_rfc3339(),
            )?;
            Ok(())
        })
        .await;

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
            // Check for shutdown between tasks to avoid running the full schedule
            // when the server is stopping. Cuts worst-case shutdown from ~28 min to ~2 min.
            if *self.shutdown.borrow() {
                tracing::info!("Slow lane: shutdown requested, breaking out of task loop");
                break;
            }

            // Skip tasks not due this cycle
            if !task.should_run(self.cycle_count) {
                continue;
            }

            // Skip low-priority tasks when under load
            if skip_low && task.priority == TaskPriority::Low {
                tracing::debug!("Slow lane: skipping low-priority task '{}'", task.task);
                continue;
            }

            processed += self.dispatch_task(task.task, client).await;
        }

        processed
    }

    /// Dispatch a task to its implementation.
    /// The exhaustive match ensures new `BackgroundTask` variants cause a compile error
    /// until their dispatch logic is added here.
    async fn dispatch_task(
        &self,
        task: BackgroundTask,
        client: Option<&Arc<dyn LlmClient>>,
    ) -> usize {
        let name = task.to_string();
        match task {
            BackgroundTask::StaleSessions => {
                Self::run_task(
                    &name,
                    session_summaries::process_stale_sessions(&self.pool, client),
                )
                .await
            }
            BackgroundTask::Summaries => {
                Self::run_task(
                    &name,
                    summaries::process_queue(&self.code_pool, &self.pool, client),
                )
                .await
            }
            BackgroundTask::Briefings => {
                Self::run_task(&name, briefings::process_briefings(&self.pool, client)).await
            }
            BackgroundTask::DocumentationTasks => {
                Self::run_task(
                    &name,
                    documentation::process_documentation(&self.pool, &self.code_pool, client),
                )
                .await
            }
            BackgroundTask::HealthFastScans => {
                Self::run_task(
                    &name,
                    code_health::process_health_fast_scans(&self.pool, &self.code_pool),
                )
                .await
            }
            BackgroundTask::HealthLlmComplexity => {
                Self::run_task(
                    &name,
                    code_health::process_health_llm_complexity(&self.pool, &self.code_pool, client),
                )
                .await
            }
            BackgroundTask::HealthLlmErrorQuality => {
                Self::run_task(
                    &name,
                    code_health::process_health_llm_error_quality(
                        &self.pool,
                        &self.code_pool,
                        client,
                    ),
                )
                .await
            }
            BackgroundTask::HealthModuleAnalysis => {
                Self::run_task(
                    &name,
                    code_health::process_health_module_analysis(&self.pool, &self.code_pool),
                )
                .await
            }
            BackgroundTask::PonderingInsights => {
                Self::run_task(&name, pondering::process_pondering(&self.pool, client)).await
            }
            BackgroundTask::InsightCleanup => {
                Self::run_task(&name, pondering::cleanup_stale_insights(&self.pool)).await
            }
            BackgroundTask::ProactiveCleanup => {
                Self::run_task(
                    &name,
                    crate::proactive::background::cleanup_expired_suggestions(&self.pool),
                )
                .await
            }
            BackgroundTask::DiffOutcomes => {
                Self::run_task(
                    &name,
                    outcome_scanner::process_outcome_scanning(&self.pool, &self.code_pool),
                )
                .await
            }
            BackgroundTask::TeamMonitor => {
                Self::run_task(&name, team_monitor::process_team_monitor(&self.pool)).await
            }
            BackgroundTask::ProactiveItems => {
                Self::run_task(
                    &name,
                    crate::proactive::background::process_proactive(
                        &self.pool,
                        client,
                        self.cycle_count,
                    ),
                )
                .await
            }
            BackgroundTask::EntityBackfills => {
                Self::run_task(
                    &name,
                    entity_extraction::process_entity_backfill(&self.pool),
                )
                .await
            }
            BackgroundTask::MemoryEmbeddings => {
                if let Some(ref emb) = self.embeddings {
                    Self::run_task(
                        &name,
                        memory_embeddings::process_memory_embeddings(&self.pool, emb),
                    )
                    .await
                } else {
                    0
                }
            }
            BackgroundTask::DataRetention => {
                let pool = self.pool.clone();
                Self::run_task(&name, async move {
                    pool.run(crate::db::retention::run_data_retention_sync)
                        .await
                })
                .await
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

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    // ═══════════════════════════════════════
    // ScheduledTask::should_run
    // ═══════════════════════════════════════

    #[test]
    fn should_run_none_always_runs() {
        let task = ScheduledTask {
            task: BackgroundTask::StaleSessions,
            priority: TaskPriority::Critical,
            cycle_interval: None,
        };
        for cycle in 0..10 {
            assert!(
                task.should_run(cycle),
                "should_run(None) must be true for cycle {cycle}"
            );
        }
    }

    #[test]
    fn should_run_zero_always_runs() {
        let task = ScheduledTask {
            task: BackgroundTask::Summaries,
            priority: TaskPriority::Normal,
            cycle_interval: Some(0),
        };
        for cycle in 0..10 {
            assert!(
                task.should_run(cycle),
                "should_run(Some(0)) must be true for cycle {cycle}"
            );
        }
    }

    #[test]
    fn should_run_interval_3() {
        let task = ScheduledTask {
            task: BackgroundTask::DocumentationTasks,
            priority: TaskPriority::Low,
            cycle_interval: Some(3),
        };
        // 0 is a multiple of 3 (0 % 3 == 0)
        assert!(task.should_run(0));
        assert!(!task.should_run(1));
        assert!(!task.should_run(2));
        assert!(task.should_run(3));
        assert!(!task.should_run(4));
        assert!(!task.should_run(5));
        assert!(task.should_run(6));
        assert!(task.should_run(9));
        assert!(task.should_run(30));
    }

    #[test]
    fn should_run_interval_10() {
        let task = ScheduledTask {
            task: BackgroundTask::PonderingInsights,
            priority: TaskPriority::Low,
            cycle_interval: Some(10),
        };
        assert!(task.should_run(0));
        assert!(!task.should_run(1));
        assert!(!task.should_run(9));
        assert!(task.should_run(10));
        assert!(task.should_run(20));
        assert!(!task.should_run(15));
    }

    // ═══════════════════════════════════════
    // skip_low_priority
    // ═══════════════════════════════════════

    #[tokio::test]
    async fn skip_low_priority_under_threshold() {
        let worker = make_test_worker(Duration::from_secs(59)).await;
        assert!(!worker.skip_low_priority());
    }

    #[tokio::test]
    async fn skip_low_priority_at_threshold() {
        let worker = make_test_worker(Duration::from_secs(60)).await;
        assert!(worker.skip_low_priority());
    }

    #[tokio::test]
    async fn skip_low_priority_over_threshold() {
        let worker = make_test_worker(Duration::from_secs(120)).await;
        assert!(worker.skip_low_priority());
    }

    #[tokio::test]
    async fn skip_low_priority_zero_duration() {
        let worker = make_test_worker(Duration::ZERO).await;
        assert!(!worker.skip_low_priority());
    }

    // ═══════════════════════════════════════
    // task_schedule completeness
    // ═══════════════════════════════════════

    #[test]
    fn task_schedule_contains_all_variants() {
        let schedule = task_schedule();
        // Verify we have all BackgroundTask variants
        let names: Vec<String> = schedule.iter().map(|s| s.task.to_string()).collect();

        assert!(names.contains(&"stale sessions".to_string()));
        assert!(names.contains(&"memory embeddings".to_string()));
        assert!(names.contains(&"summaries".to_string()));
        assert!(names.contains(&"briefings".to_string()));
        assert!(names.contains(&"health: fast scans".to_string()));
        assert!(names.contains(&"health: LLM complexity".to_string()));
        assert!(names.contains(&"health: LLM error quality".to_string()));
        assert!(names.contains(&"health: module analysis".to_string()));
        assert!(names.contains(&"proactive items".to_string()));
        assert!(names.contains(&"entity backfills".to_string()));
        assert!(names.contains(&"team monitor".to_string()));
        assert!(names.contains(&"documentation tasks".to_string()));
        assert!(names.contains(&"pondering insights".to_string()));
        assert!(names.contains(&"insight cleanup".to_string()));
        assert!(names.contains(&"proactive cleanup".to_string()));
        assert!(names.contains(&"diff outcomes".to_string()));
        assert!(names.contains(&"data retention".to_string()));
    }

    #[test]
    fn task_schedule_critical_tasks_come_first() {
        let schedule = task_schedule();
        // All Critical tasks should appear before any Normal or Low
        let mut seen_non_critical = false;
        for task in &schedule {
            if task.priority != TaskPriority::Critical {
                seen_non_critical = true;
            }
            if seen_non_critical {
                assert_ne!(
                    task.priority,
                    TaskPriority::Critical,
                    "Critical task '{}' appears after non-critical tasks",
                    task.task
                );
            }
        }
    }

    #[test]
    fn task_schedule_has_no_duplicates() {
        let schedule = task_schedule();
        let names: Vec<String> = schedule.iter().map(|s| s.task.to_string()).collect();
        let unique: std::collections::HashSet<&String> = names.iter().collect();
        assert_eq!(
            names.len(),
            unique.len(),
            "task_schedule contains duplicates"
        );
    }

    // ═══════════════════════════════════════
    // Helpers
    // ═══════════════════════════════════════

    /// Build a minimal SlowLaneWorker with a given last_cycle_duration for skip_low_priority tests.
    async fn make_test_worker(last_cycle_duration: Duration) -> SlowLaneWorker {
        let (_tx, rx) = watch::channel(false);
        let pool = Arc::new(
            crate::db::pool::DatabasePool::open_in_memory()
                .await
                .unwrap(),
        );
        let code_pool = Arc::new(
            crate::db::pool::DatabasePool::open_in_memory()
                .await
                .unwrap(),
        );
        SlowLaneWorker {
            pool,
            code_pool,
            embeddings: None,
            llm_factory: Arc::new(crate::llm::ProviderFactory::new()),
            shutdown: rx,
            cycle_count: 0,
            last_cycle_duration,
        }
    }
}
