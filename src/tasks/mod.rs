// src/tasks/mod.rs

//! Background task management for async operations.
//! Handles analysis, decay, cleanup, and other periodic tasks.

use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinHandle;
use tokio::time;
use tracing::{debug, error, info};
use sqlx::Row;

use crate::memory::features::decay;
use crate::memory::features::message_pipeline::MessagePipeline;
use crate::llm::router::TaskType;  // NEW: Import TaskType
use crate::state::AppState;

pub mod config;
pub mod metrics;
pub mod backfill;

use config::TaskConfig;
use metrics::TaskMetrics;
use backfill::BackfillTask;

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

        // Start metrics reporter
        let handle = self.spawn_metrics_reporter();
        self.handles.push(handle);

        info!("Started {} background tasks", self.handles.len());
    }

    /// Run one-time embedding backfill task
    /// This processes all messages that were "queued" before the embedding processor was implemented
    async fn run_backfill(&self) {
        info!("Running one-time embedding backfill check");
        
        let backfill = BackfillTask::new(self.app_state.clone());
        
        match backfill.run().await {
            Ok(()) => {
                info!("Embedding backfill completed successfully");
            }
            Err(e) => {
                error!("Embedding backfill failed: {}", e);
                // Don't panic - this is non-critical, new messages will still work
            }
        }
    }

    /// Spawns the analysis processor task
    /// Processes pending messages through the unified MessagePipeline
    fn spawn_analysis_processor(&self) -> JoinHandle<()> {
        let app_state = self.app_state.clone();
        let interval = self.config.analysis_interval;
        let metrics = self.metrics.clone();

        tokio::spawn(async move {
            info!("Analysis processor started (interval: {:?})", interval);
            
            // NEW: MessagePipeline uses GPT-5 via router for message analysis
            // GPT-5 is better at understanding sentiment, intent, and context
            let gpt5_provider = app_state.llm_router.route(TaskType::Chat);
            
            let message_pipeline = MessagePipeline::new(gpt5_provider);

            let mut interval_timer = time::interval(interval);
            interval_timer.set_missed_tick_behavior(time::MissedTickBehavior::Skip);

            loop {
                interval_timer.tick().await;
                
                // Process all active sessions
                match get_active_sessions(&app_state).await {
                    Ok(sessions) => {
                        let start = std::time::Instant::now();
                        let mut total_processed = 0;

                        for session_id in sessions {
                            match message_pipeline.process_pending_messages(&session_id).await {
                                Ok(count) => {
                                    if count > 0 {
                                        info!("Analyzed {} messages for session {}", count, session_id);
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
                            info!("Analysis batch complete: {} messages in {:?}", total_processed, duration);
                        }
                    }
                    Err(e) => {
                        error!("Failed to get active sessions: {}", e);
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
        
        info!(
            "Decay scheduler started (interval: {} hours)", 
            interval.as_secs() / 3600
        );
        
        tokio::spawn(async move {
            let mut interval_timer = time::interval(interval);
            interval_timer.set_missed_tick_behavior(time::MissedTickBehavior::Skip);
            
            loop {
                interval_timer.tick().await;
                
                let start = std::time::Instant::now();
                
                match decay::run_decay_cycle(app_state.clone()).await {
                    Ok(()) => {
                        let duration = start.elapsed();
                        metrics.record_task_duration("decay", duration);
                        metrics.add_processed_items("decay", 1); // 1 cycle completed
                    }
                    Err(e) => {
                        error!("Decay cycle failed: {:#}", e);
                        metrics.record_error("decay");
                    }
                }
            }
        })
    }

    /// Spawns the session cleanup task
    fn spawn_session_cleanup(&self) -> JoinHandle<()> {
        let memory_service = self.app_state.memory_service.clone();
        let interval = self.config.cleanup_interval;
        let max_age_hours = self.config.session_max_age_hours;
        let metrics = self.metrics.clone();

        tokio::spawn(async move {
            info!("Session cleanup started (interval: {:?}, max age: {}h)", interval, max_age_hours);
            
            let mut interval_timer = time::interval(interval);
            interval_timer.set_missed_tick_behavior(time::MissedTickBehavior::Skip);

            loop {
                interval_timer.tick().await;
                
                match memory_service.cleanup_inactive_sessions(max_age_hours).await {
                    Ok(count) => {
                        if count > 0 {
                            info!("Cleaned up {} inactive sessions", count);
                            metrics.add_processed_items("cleanup", count);
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
    /// Identifies sessions that may need summaries and delegates to SummarizationEngine
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
                
                // Get all sessions with their message counts
                // SummarizationEngine decides what needs summarizing
                match check_summary_candidates(&app_state).await {
                    Ok(candidates) => {
                        for (session_id, message_count) in candidates {
                            let summarization_engine = &app_state.memory_service.summarization_engine;
                            
                            // Engine handles all threshold logic for rolling/session summaries
                            match summarization_engine
                                .check_and_process_summaries(&session_id, message_count)
                                .await 
                            {
                                Ok(Some(summary_id)) => {
                                    info!("Created summary {} for session {}", summary_id, session_id);
                                    metrics.add_processed_items("summary", 1);
                                }
                                Ok(None) => {
                                    // No summary needed at this message count
                                    debug!("No summary needed for session {} at {} messages", 
                                        session_id, message_count);
                                }
                                Err(e) => {
                                    error!("Summary processing failed for session {}: {}", session_id, e);
                                    metrics.record_error("summary");
                                }
                            }
                        }
                    }
                    Err(e) => {
                        error!("Failed to check summary candidates: {}", e);
                    }
                }
            }
        })
    }

    /// Spawns the metrics reporter task
    fn spawn_metrics_reporter(&self) -> JoinHandle<()> {
        let metrics = self.metrics.clone();
        let interval = Duration::from_secs(3600); // 1 hour

        tokio::spawn(async move {
            let mut interval_timer = time::interval(interval);
            interval_timer.set_missed_tick_behavior(time::MissedTickBehavior::Skip);

            loop {
                interval_timer.tick().await;
                metrics.report();
            }
        })
    }

    /// Gracefully shuts down all tasks
    pub async fn shutdown(self) {
        info!("Shutting down {} background tasks", self.handles.len());
        
        for handle in self.handles {
            handle.abort();
        }
        
        info!("All background tasks terminated");
    }
}

/// Get active sessions from the database (configurable limit)
async fn get_active_sessions(app_state: &Arc<AppState>) -> anyhow::Result<Vec<String>> {
    let pool = &app_state.sqlite_store.pool;
    let limit = TaskConfig::from_env().active_session_limit;
    
    let rows = sqlx::query(&format!(
        r#"
        SELECT DISTINCT session_id 
        FROM memory_entries 
        WHERE timestamp > datetime('now', '-24 hours')
        ORDER BY timestamp DESC
        LIMIT {}
        "#,
        limit
    ))
    .fetch_all(pool)
    .await?;
    
    Ok(rows.iter()
        .filter_map(|row| row.try_get::<String, _>("session_id").ok())
        .collect())
}

/// Check which sessions might need summarization
async fn check_summary_candidates(app_state: &Arc<AppState>) -> anyhow::Result<Vec<(String, usize)>> {
    let pool = &app_state.sqlite_store.pool;
    
    let rows = sqlx::query(
        r#"
        SELECT session_id, COUNT(*) as count
        FROM memory_entries
        WHERE timestamp > datetime('now', '-7 days')
        GROUP BY session_id
        HAVING COUNT(*) >= 10
        "#
    )
    .fetch_all(pool)
    .await?;
    
    Ok(rows.iter()
        .filter_map(|row| {
            let session_id: String = row.try_get("session_id").ok()?;
            let count: i64 = row.try_get("count").ok()?;
            Some((session_id, count as usize))
        })
        .collect())
}
