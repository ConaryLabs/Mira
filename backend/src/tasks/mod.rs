// src/tasks/mod.rs

//! Background task management for async operations.
//! Handles analysis, decay, cleanup, and other periodic tasks.

use sqlx::Row;
use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinHandle;
use tokio::time;
use tracing::{error, info, warn};

use crate::memory::features::decay;
use crate::memory::features::message_pipeline::MessagePipeline;
use crate::state::AppState;

pub mod backfill;
pub mod code_sync;
pub mod config;
pub mod embedding_cleanup;
pub mod metrics;

use backfill::BackfillTask;
use code_sync::CodeSyncTask;
use config::TaskConfig;
use embedding_cleanup::EmbeddingCleanupTask;
use metrics::TaskMetrics;

/// Manages all background tasks for the memory system
pub struct TaskManager {
    app_state: Arc<AppState>,
    config: TaskConfig,
    metrics: Arc<TaskMetrics>,
    handles: Vec<JoinHandle<()>>,
}

impl TaskManager {
    /// Creates a new task manager
    pub fn new(app_state: Arc<AppState>) -> Self {
        Self {
            app_state,
            config: TaskConfig::from_env(),
            metrics: Arc::new(TaskMetrics::new()),
            handles: Vec::new(),
        }
    }

    /// Starts all background tasks
    pub async fn start(&mut self) {
        info!("Starting background task manager");

        // Run one-time embedding backfill on startup
        self.run_backfill().await;

        // Start analysis processor
        if self.config.analysis_enabled {
            let handle = self.spawn_analysis_processor();
            self.handles.push(handle);
        }

        // Start decay scheduler
        if self.config.decay_enabled {
            let handle = self.spawn_decay_scheduler();
            self.handles.push(handle);
        }

        // Start session cleanup
        if self.config.cleanup_enabled {
            let handle = self.spawn_session_cleanup();
            self.handles.push(handle);
        }

        // Start summary processor
        if self.config.summary_processor_enabled {
            let handle = self.spawn_summary_processor();
            self.handles.push(handle);
        }

        // Start code sync task (Layer 2: Background sync every 5min)
        if self.config.code_sync_enabled {
            let handle = self.spawn_code_sync();
            self.handles.push(handle);
        }

        // Start embedding cleanup task (weekly orphan removal)
        if self.config.embedding_cleanup_enabled {
            let handle = self.spawn_embedding_cleanup();
            self.handles.push(handle);
        }

        // Start metrics reporter
        let handle = self.spawn_metrics_reporter();
        self.handles.push(handle);

        info!("Started {} background tasks", self.handles.len());
    }

    /// Run one-time embedding backfill task
    async fn run_backfill(&self) {
        info!("Running one-time embedding backfill check");

        let backfill = BackfillTask::new(self.app_state.clone());

        match backfill.run().await {
            Ok(()) => {
                info!("Embedding backfill completed successfully");
            }
            Err(e) => {
                error!("Embedding backfill failed: {}", e);
            }
        }
    }

    /// Spawns the embedding cleanup task
    fn spawn_embedding_cleanup(&self) -> JoinHandle<()> {
        let pool = Arc::new(self.app_state.sqlite_pool.clone());
        let multi_store = self.app_state.memory_service.get_multi_store();
        let interval = self.config.embedding_cleanup_interval;
        let metrics = self.metrics.clone();

        tokio::spawn(async move {
            info!("Embedding cleanup task started (interval: {:?})", interval);

            let cleanup = EmbeddingCleanupTask::new(pool, multi_store);
            let mut interval_timer = time::interval(interval);
            interval_timer.set_missed_tick_behavior(time::MissedTickBehavior::Skip);

            loop {
                interval_timer.tick().await;

                let start = std::time::Instant::now();

                match cleanup.run(false).await {
                    // false = actually delete orphans
                    Ok(report) => {
                        let duration = start.elapsed();
                        metrics.record_task_duration("embedding_cleanup", duration);

                        info!(
                            "Embedding cleanup complete: checked {}, found {} orphans, deleted {}",
                            report.total_checked, report.orphans_found, report.orphans_deleted
                        );

                        if !report.errors.is_empty() {
                            warn!(
                                "Cleanup had {} errors: {:?}",
                                report.errors.len(),
                                report.errors
                            );
                        }

                        metrics.add_processed_items("embedding_cleanup", report.orphans_deleted);
                    }
                    Err(e) => {
                        error!("Embedding cleanup failed: {}", e);
                        metrics.record_error("embedding_cleanup");
                    }
                }
            }
        })
    }

    /// Spawns the code sync task (Layer 2: Background safety net)
    fn spawn_code_sync(&self) -> JoinHandle<()> {
        let pool = self.app_state.sqlite_pool.clone();
        let code_intelligence = self.app_state.code_intelligence.clone();
        let interval = self.config.code_sync_interval;
        let metrics = self.metrics.clone();

        tokio::spawn(async move {
            info!("Code sync task started (interval: {:?})", interval);

            let sync_task = CodeSyncTask::new(pool, code_intelligence);
            let mut interval_timer = time::interval(interval);
            interval_timer.set_missed_tick_behavior(time::MissedTickBehavior::Skip);

            loop {
                interval_timer.tick().await;

                let start = std::time::Instant::now();

                match sync_task.run().await {
                    Ok(()) => {
                        let duration = start.elapsed();
                        metrics.record_task_duration("code_sync", duration);
                        metrics.add_processed_items("code_sync", 1);
                    }
                    Err(e) => {
                        error!("Code sync failed: {}", e);
                        metrics.record_error("code_sync");
                    }
                }
            }
        })
    }

    /// Spawns the analysis processor task
    fn spawn_analysis_processor(&self) -> JoinHandle<()> {
        let app_state = self.app_state.clone();
        let interval = self.config.analysis_interval;
        let metrics = self.metrics.clone();

        tokio::spawn(async move {
            info!("Analysis processor started (interval: {:?})", interval);

            // Use DeepSeek provider for message analysis
            let deepseek_provider = app_state.deepseek_provider.clone();
            let message_pipeline = MessagePipeline::new(deepseek_provider);

            let mut interval_timer = time::interval(interval);
            interval_timer.set_missed_tick_behavior(time::MissedTickBehavior::Skip);

            loop {
                interval_timer.tick().await;

                match get_active_sessions(&app_state).await {
                    Ok(sessions) => {
                        let start = std::time::Instant::now();
                        let mut total_processed = 0;

                        for session_id in sessions {
                            match message_pipeline.process_pending_messages(&session_id).await {
                                Ok(count) => {
                                    if count > 0 {
                                        info!(
                                            "Analyzed {} messages for session {}",
                                            count, session_id
                                        );
                                        total_processed += count;
                                    }
                                }
                                Err(e) => {
                                    error!("Analysis failed for session {}: {}", session_id, e);
                                    metrics.record_error("analysis");
                                }
                            }
                        }

                        if total_processed > 0 {
                            let duration = start.elapsed();
                            metrics.record_task_duration("analysis", duration);
                            metrics.add_processed_items("analysis", total_processed);
                            info!(
                                "Analysis batch complete: {} messages in {:?}",
                                total_processed, duration
                            );
                        }
                    }
                    Err(e) => {
                        error!("Failed to get active sessions: {}", e);
                        metrics.record_error("analysis");
                    }
                }
            }
        })
    }

    /// Spawns the decay scheduler task
    fn spawn_decay_scheduler(&self) -> JoinHandle<()> {
        let app_state = self.app_state.clone();
        let interval = self.config.decay_interval;
        let metrics = self.metrics.clone();

        tokio::spawn(async move {
            info!("Decay scheduler started (interval: {:?})", interval);

            let mut interval_timer = time::interval(interval);
            interval_timer.set_missed_tick_behavior(time::MissedTickBehavior::Skip);

            loop {
                interval_timer.tick().await;

                let start = std::time::Instant::now();

                match decay::run_decay_cycle(app_state.clone()).await {
                    Ok(()) => {
                        let duration = start.elapsed();
                        metrics.record_task_duration("decay", duration);
                        info!("Decay cycle completed in {:?}", duration);
                    }
                    Err(e) => {
                        error!("Decay scheduler failed: {}", e);
                        metrics.record_error("decay");
                    }
                }
            }
        })
    }

    /// Spawns the session cleanup task
    fn spawn_session_cleanup(&self) -> JoinHandle<()> {
        let pool = self.app_state.sqlite_pool.clone();
        let interval = self.config.cleanup_interval;
        let max_age = self.config.session_max_age_hours;
        let metrics = self.metrics.clone();

        tokio::spawn(async move {
            info!(
                "Session cleanup started (interval: {:?}, max_age: {}h)",
                interval, max_age
            );

            let mut interval_timer = time::interval(interval);
            interval_timer.set_missed_tick_behavior(time::MissedTickBehavior::Skip);

            loop {
                interval_timer.tick().await;

                match cleanup_old_sessions(&pool, max_age).await {
                    Ok(count) => {
                        if count > 0 {
                            info!("Cleaned up {} inactive sessions", count);
                            metrics.add_processed_items("cleanup", count as usize);
                        }
                    }
                    Err(e) => {
                        error!("Session cleanup failed: {}", e);
                        metrics.record_error("cleanup");
                    }
                }
            }
        })
    }

    /// Spawns the summary processor task
    fn spawn_summary_processor(&self) -> JoinHandle<()> {
        let app_state = self.app_state.clone();
        let interval = self.config.summary_check_interval;
        let metrics = self.metrics.clone();

        tokio::spawn(async move {
            info!("Summary processor started (interval: {:?})", interval);

            let mut interval_timer = time::interval(interval);
            interval_timer.set_missed_tick_behavior(time::MissedTickBehavior::Skip);

            loop {
                interval_timer.tick().await;

                match check_summary_candidates(&app_state).await {
                    Ok(candidates) => {
                        for (session_id, message_count) in candidates {
                            let summarization_engine =
                                &app_state.memory_service.summarization_engine;

                            match summarization_engine
                                .check_and_process_summaries(&session_id, message_count as usize)
                                .await
                            {
                                Ok(Some(summary_id)) => {
                                    info!(
                                        "Created summary {} for session {}",
                                        summary_id, session_id
                                    );
                                    metrics.add_processed_items("summary", 1);
                                }
                                Ok(None) => {}
                                Err(e) => {
                                    error!("Summary processing failed for {}: {}", session_id, e);
                                    metrics.record_error("summary");
                                }
                            }
                        }
                    }
                    Err(e) => {
                        error!("Failed to check summary candidates: {}", e);
                        metrics.record_error("summary");
                    }
                }
            }
        })
    }

    /// Spawns the metrics reporter task
    fn spawn_metrics_reporter(&self) -> JoinHandle<()> {
        let metrics = self.metrics.clone();

        tokio::spawn(async move {
            let mut interval = time::interval(Duration::from_secs(300)); // 5 minutes
            interval.set_missed_tick_behavior(time::MissedTickBehavior::Skip);

            loop {
                interval.tick().await;
                metrics.report();
            }
        })
    }

    /// Gracefully shutdown all background tasks
    pub async fn shutdown(self) {
        info!("Shutting down background tasks");

        for handle in self.handles {
            handle.abort();
        }

        info!("All background tasks stopped");
    }
}

/// Get active sessions for processing
async fn get_active_sessions(app_state: &AppState) -> anyhow::Result<Vec<String>> {
    let sessions = sqlx::query(
        "SELECT DISTINCT session_id FROM memory_entries 
         WHERE timestamp > (strftime('%s', 'now') - 86400)
         LIMIT 100",
    )
    .fetch_all(&app_state.sqlite_pool)
    .await?;

    Ok(sessions
        .iter()
        .filter_map(|row| row.try_get::<String, _>("session_id").ok())
        .collect())
}

/// Check which sessions might need summaries
async fn check_summary_candidates(app_state: &AppState) -> anyhow::Result<Vec<(String, i64)>> {
    let candidates = sqlx::query_as::<_, (String, i64)>(
        "SELECT session_id, COUNT(*) as count 
         FROM memory_entries 
         WHERE timestamp > (strftime('%s', 'now') - 604800)
         GROUP BY session_id
         HAVING count >= 10",
    )
    .fetch_all(&app_state.sqlite_pool)
    .await?;

    Ok(candidates)
}

/// Cleanup old sessions
async fn cleanup_old_sessions(pool: &sqlx::SqlitePool, max_age_hours: i64) -> anyhow::Result<u64> {
    let result = sqlx::query(
        "DELETE FROM memory_entries 
         WHERE timestamp < (strftime('%s', 'now') - ?1)",
    )
    .bind(max_age_hours * 3600)
    .execute(pool)
    .await?;

    Ok(result.rows_affected())
}
