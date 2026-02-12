// crates/mira-server/src/background/slow_lane.rs
// Slow lane worker for LLM-dependent tasks (summaries, pondering, code health)
//
// Tasks are assigned priority levels and executed in priority order.
// When the previous cycle ran long, low-priority tasks are skipped.

use crate::db::pool::DatabasePool;
use crate::embeddings::EmbeddingClient;
use crate::llm::{LlmClient, ProviderFactory};
use std::collections::HashMap;
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

/// Circuit breaker: max consecutive failures before suppressing warnings
const CIRCUIT_BREAKER_THRESHOLD: u32 = 3;
/// Circuit breaker: backoff multiplier for warning suppression
/// Warning is logged every 2^failures cycles (capped at 2^6 = 64)
const CIRCUIT_BREAKER_MAX_EXPONENT: u32 = 6;

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

/// Tracks failure state for a single task type
#[derive(Debug, Clone, Default)]
struct TaskFailureState {
    /// Consecutive failure count
    count: u32,
    /// Last cycle where we logged a warning
    last_warn_cycle: u64,
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
    /// Tracks failure counts per task for circuit breaker pattern
    failure_states: HashMap<String, TaskFailureState>,
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
            failure_states: HashMap::new(),
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

        if let Some(ref c) = client {
            tracing::debug!(provider = %c.provider_type(), "Slow lane: using LLM provider");
        } else {
            tracing::info!("Slow lane: no LLM provider available, using fallbacks");
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
        &mut self,
        task: BackgroundTask,
        client: Option<&Arc<dyn LlmClient>>,
    ) -> usize {
        let name = task.to_string();

        // Clone pools and other needed data upfront to avoid borrow issues with &mut self
        let pool = self.pool.clone();
        let code_pool = self.code_pool.clone();
        let cycle_count = self.cycle_count;

        match task {
            BackgroundTask::StaleSessions => {
                self.run_task(
                    &name,
                    session_summaries::process_stale_sessions(&pool, client),
                )
                .await
            }
            BackgroundTask::Summaries => {
                self.run_task(&name, summaries::process_queue(&code_pool, &pool, client))
                    .await
            }
            BackgroundTask::Briefings => {
                self.run_task(&name, briefings::process_briefings(&pool, client))
                    .await
            }
            BackgroundTask::DocumentationTasks => {
                self.run_task(
                    &name,
                    documentation::process_documentation(&pool, &code_pool, client),
                )
                .await
            }
            BackgroundTask::HealthFastScans => {
                self.run_task(
                    &name,
                    code_health::process_health_fast_scans(&pool, &code_pool),
                )
                .await
            }
            BackgroundTask::HealthLlmComplexity => {
                self.run_task(
                    &name,
                    code_health::process_health_llm_complexity(&pool, &code_pool, client),
                )
                .await
            }
            BackgroundTask::HealthLlmErrorQuality => {
                self.run_task(
                    &name,
                    code_health::process_health_llm_error_quality(&pool, &code_pool, client),
                )
                .await
            }
            BackgroundTask::HealthModuleAnalysis => {
                self.run_task(
                    &name,
                    code_health::process_health_module_analysis(&pool, &code_pool),
                )
                .await
            }
            BackgroundTask::PonderingInsights => {
                self.run_task(&name, pondering::process_pondering(&pool, client))
                    .await
            }
            BackgroundTask::InsightCleanup => {
                self.run_task(&name, pondering::cleanup_stale_insights(&pool))
                    .await
            }
            BackgroundTask::ProactiveCleanup => {
                self.run_task(
                    &name,
                    crate::proactive::background::cleanup_expired_suggestions(&pool),
                )
                .await
            }
            BackgroundTask::DiffOutcomes => {
                self.run_task(
                    &name,
                    outcome_scanner::process_outcome_scanning(&pool, &code_pool),
                )
                .await
            }
            BackgroundTask::TeamMonitor => {
                self.run_task(&name, team_monitor::process_team_monitor(&pool))
                    .await
            }
            BackgroundTask::ProactiveItems => {
                self.run_task(
                    &name,
                    crate::proactive::background::process_proactive(&pool, client, cycle_count),
                )
                .await
            }
            BackgroundTask::EntityBackfills => {
                self.run_task(&name, entity_extraction::process_entity_backfill(&pool))
                    .await
            }
            BackgroundTask::MemoryEmbeddings => {
                if let Some(ref emb) = self.embeddings {
                    let emb = emb.clone();
                    self.run_task(
                        &name,
                        memory_embeddings::process_memory_embeddings(&pool, &emb),
                    )
                    .await
                } else {
                    0
                }
            }
            BackgroundTask::DataRetention => {
                self.run_task(&name, async move {
                    let retention_count = pool
                        .run(crate::db::retention::run_data_retention_sync)
                        .await?;
                    // Also clean up expired system observations (TTL-based)
                    let obs_count = pool
                        .run(crate::db::cleanup_expired_observations_sync)
                        .await?;
                    Ok(retention_count + obs_count)
                })
                .await
            }
        }
    }

    /// Run a background task with a timeout. Errors and timeouts are caught and
    /// logged so that one failing subsystem cannot starve others.
    /// Implements circuit breaker pattern to reduce log spam from repeatedly failing tasks.
    async fn run_task(
        &mut self,
        name: &str,
        fut: impl std::future::Future<Output = Result<usize, String>>,
    ) -> usize {
        let result = timeout(Duration::from_secs(TASK_TIMEOUT_SECS), fut).await;

        match result {
            Ok(Ok(count)) => {
                // Reset failure count on success
                if self.failure_states.contains_key(name) {
                    self.failure_states.remove(name);
                }
                if count > 0 {
                    tracing::info!("Slow lane: processed {} {}", count, name);
                }
                count
            }
            Ok(Err(e)) => {
                self.handle_task_failure(name, &format!("failed: {e}"))
                    .await;
                0
            }
            Err(_) => {
                self.handle_task_failure(name, &format!("timed out after {TASK_TIMEOUT_SECS}s"))
                    .await;
                0
            }
        }
    }

    /// Handle task failure with circuit breaker pattern.
    /// Tracks consecutive failures and suppresses warnings using exponential backoff.
    async fn handle_task_failure(&mut self, name: &str, reason: &str) {
        let state = self.failure_states.entry(name.to_string()).or_default();

        state.count += 1;

        // Determine if we should log a warning based on circuit breaker state
        let should_warn = if state.count <= CIRCUIT_BREAKER_THRESHOLD {
            // Always warn for first few failures
            true
        } else {
            // Use exponential backoff: warn every 2^(count - threshold) cycles
            let exponent =
                (state.count - CIRCUIT_BREAKER_THRESHOLD).min(CIRCUIT_BREAKER_MAX_EXPONENT);
            let interval = 1u64 << exponent;
            self.cycle_count.saturating_sub(state.last_warn_cycle) >= interval
        };

        if should_warn {
            if state.count > CIRCUIT_BREAKER_THRESHOLD {
                tracing::warn!(
                    "Slow lane task '{}' {} (failure #{}, suppressing future warnings for {} cycles)",
                    name,
                    reason,
                    state.count,
                    1u64 << (state.count - CIRCUIT_BREAKER_THRESHOLD)
                        .min(CIRCUIT_BREAKER_MAX_EXPONENT)
                );
            } else {
                tracing::warn!(
                    "Slow lane task '{}' {} (failure #{}/{})",
                    name,
                    reason,
                    state.count,
                    CIRCUIT_BREAKER_THRESHOLD
                );
            }
            state.last_warn_cycle = self.cycle_count;
        } else {
            // Log at debug level so the failure is still traceable
            tracing::debug!(
                "Slow lane task '{}' {} (failure #{}, suppressed)",
                name,
                reason,
                state.count
            );
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
    // Circuit breaker tests
    // ═══════════════════════════════════════

    #[test]
    fn task_failure_state_tracks_consecutive_failures() {
        let mut state = TaskFailureState::default();
        assert_eq!(state.count, 0);

        state.count += 1;
        assert_eq!(state.count, 1);

        state.count += 1;
        assert_eq!(state.count, 2);
    }

    #[test]
    fn task_failure_state_resets() {
        let mut state = TaskFailureState {
            count: 5,
            last_warn_cycle: 100,
        };

        state.count = 0;
        state.last_warn_cycle = 0;

        assert_eq!(state.count, 0);
        assert_eq!(state.last_warn_cycle, 0);
    }

    #[test]
    fn circuit_breaker_warn_interval_calculation() {
        // Test the exponential backoff logic
        let threshold = 3u32;
        let max_exponent = 6u32;

        // For failures 1-3: always warn (interval doesn't matter)
        for count in 1..=threshold {
            assert!(count <= threshold, "Should warn for count <= threshold");
        }

        // For failures 4+: warn every 2^(count - threshold) cycles
        let count = 4u32;
        let exponent = (count - threshold).min(max_exponent);
        let interval = 1u64 << exponent;
        assert_eq!(interval, 2, "Failure 4 should warn every 2 cycles");

        let count = 5u32;
        let exponent = (count - threshold).min(max_exponent);
        let interval = 1u64 << exponent;
        assert_eq!(interval, 4, "Failure 5 should warn every 4 cycles");

        let count = 9u32;
        let exponent = (count - threshold).min(max_exponent);
        let interval = 1u64 << exponent;
        assert_eq!(interval, 64, "Failure 9+ should cap at 64 cycles");

        let count = 20u32;
        let exponent = (count - threshold).min(max_exponent);
        let interval = 1u64 << exponent;
        assert_eq!(interval, 64, "Failure 20 should still cap at 64 cycles");
    }

    #[test]
    fn circuit_breaker_should_warn_logic() {
        // Simulate the logic from handle_task_failure
        let threshold = 3u32;
        let max_exponent = 6u32;

        // Test cases: (failure_count, cycles_since_last_warn, expected_should_warn)
        let test_cases = vec![
            (1, 0, true),    // First failure: always warn
            (2, 0, true),    // Second failure: always warn
            (3, 0, true),    // Third failure: always warn (at threshold)
            (4, 0, false),   // Fourth failure: only 0 cycles since last warn, interval is 2
            (4, 1, false),   // Fourth failure: only 1 cycle since last warn, need 2
            (4, 2, true),    // Fourth failure: 2 cycles passed, should warn
            (5, 0, false),   // Fifth failure: interval is 4
            (5, 3, false),   // Fifth failure: 3 cycles passed, need 4
            (5, 4, true),    // Fifth failure: 4 cycles passed, should warn
            (10, 0, false),  // Tenth failure: interval is 64
            (10, 63, false), // Tenth failure: 63 cycles passed, need 64
            (10, 64, true),  // Tenth failure: 64 cycles passed, should warn
            (10, 100, true), // Tenth failure: way more cycles passed, should warn
        ];

        for (failure_count, cycles_since_last_warn, expected_warn) in test_cases {
            let should_warn = if failure_count <= threshold {
                true
            } else {
                let exponent = (failure_count - threshold).min(max_exponent);
                let interval = 1u64 << exponent;
                cycles_since_last_warn >= interval
            };

            assert_eq!(
                should_warn, expected_warn,
                "Failure count={}, cycles_since_last_warn={}, expected_warn={}",
                failure_count, cycles_since_last_warn, expected_warn
            );
        }
    }

    #[tokio::test]
    async fn run_task_failures_increment_and_success_resets_state() {
        let mut worker = make_test_worker(Duration::ZERO).await;
        let name = "test-task";

        worker.cycle_count = 1;
        let processed = worker
            .run_task(name, async { Err::<usize, String>("boom".to_string()) })
            .await;
        assert_eq!(processed, 0);
        assert_eq!(worker.failure_states.get(name).map(|s| s.count), Some(1));

        worker.cycle_count = 2;
        let processed = worker
            .run_task(name, async { Err::<usize, String>("boom".to_string()) })
            .await;
        assert_eq!(processed, 0);
        assert_eq!(worker.failure_states.get(name).map(|s| s.count), Some(2));

        worker.cycle_count = 3;
        let processed = worker
            .run_task(name, async { Ok::<usize, String>(1) })
            .await;
        assert_eq!(processed, 1);
        assert!(
            !worker.failure_states.contains_key(name),
            "failure state should reset after success"
        );
    }

    #[tokio::test]
    async fn handle_task_failure_updates_last_warn_cycle_only_when_due() {
        let mut worker = make_test_worker(Duration::ZERO).await;
        let name = "backoff-task";

        // Simulate previous threshold-level failures that last warned at cycle 10.
        worker.failure_states.insert(
            name.to_string(),
            TaskFailureState {
                count: CIRCUIT_BREAKER_THRESHOLD,
                last_warn_cycle: 10,
            },
        );
        // Next failure becomes threshold+1 (interval = 2). At cycle 11, still suppressed.
        worker.cycle_count = 11;
        worker.handle_task_failure(name, "failed").await;
        let state = worker.failure_states.get(name).expect("state should exist");
        assert_eq!(state.count, CIRCUIT_BREAKER_THRESHOLD + 1);
        assert_eq!(
            state.last_warn_cycle, 10,
            "warning should be suppressed before interval elapses"
        );

        // Reset to the same starting state and rerun when interval has elapsed.
        worker.failure_states.insert(
            name.to_string(),
            TaskFailureState {
                count: CIRCUIT_BREAKER_THRESHOLD,
                last_warn_cycle: 10,
            },
        );
        worker.cycle_count = 12;
        worker.handle_task_failure(name, "failed").await;
        let state = worker.failure_states.get(name).expect("state should exist");
        assert_eq!(state.count, CIRCUIT_BREAKER_THRESHOLD + 1);
        assert_eq!(
            state.last_warn_cycle, 12,
            "warning should update once backoff interval is reached"
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
            failure_states: HashMap::new(),
        }
    }
}
