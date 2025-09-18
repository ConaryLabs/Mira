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
use crate::memory::features::message_pipeline::MessagePipeline;  // CHANGED from message_analyzer
use crate::state::AppState;

pub mod config;
pub mod metrics;

use config::TaskConfig;
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

    /// Spawns the analysis processor task
    fn spawn_analysis_processor(&self) -> JoinHandle<()> {
        let app_state = self.app_state.clone();
        let interval = self.config.analysis_interval;
        let batch_size = self.config.analysis_batch_size;
        let metrics = self.metrics.clone();

        tokio::spawn(async move {
            info!("Analysis processor started (interval: {:?}, batch: {})", interval, batch_size);
            
            // CHANGED: Use unified MessagePipeline instead of AnalysisService
            let message_pipeline = MessagePipeline::new(
                app_state.llm_client.clone(),
            );

            let mut interval_timer = time::interval(interval);
            interval_timer.set_missed_tick_behavior(time::MissedTickBehavior::Skip);

            loop {
                interval_timer.tick().await;
                
                // Get all active sessions
                match get_active_sessions(&app_state).await {
                    Ok(sessions) => {
                        let start = std::time::Instant::now();
                        let mut total_processed = 0;

                        for session_id in sessions {
                            // CHANGED: Use message_pipeline instead of analysis_service
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
        
        // Wrap the decay scheduler to add metrics tracking
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
                
                // Check for sessions needing summaries
                match check_summary_candidates(&app_state).await {
                    Ok(candidates) => {
                        for (session_id, message_count) in candidates {
                            // ACCESS THE SUMMARIZATION ENGINE DIRECTLY
                            let summarization_engine = &app_state.memory_service.summarization_engine;
                            
                            // Use the engine's check_and_process_summaries method
                            // This handles all the logic for determining what summaries to create
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
        let interval = Duration::from_secs(60);

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

/// Helper to get active sessions from the database
async fn get_active_sessions(app_state: &Arc<AppState>) -> anyhow::Result<Vec<String>> {
    let pool = &app_state.sqlite_store.pool;
    
    let rows = sqlx::query(
        r#"
        SELECT DISTINCT session_id 
        FROM memory_entries 
        WHERE timestamp > datetime('now', '-24 hours')
        ORDER BY timestamp DESC
        LIMIT 100
        "#
    )
    .fetch_all(pool)
    .await?;
    
    let sessions: Vec<String> = rows
        .iter()
        .map(|row| row.get("session_id"))
        .collect();
    
    Ok(sessions)
}

/// Helper to find sessions needing summaries  
async fn check_summary_candidates(app_state: &Arc<AppState>) -> anyhow::Result<Vec<(String, usize)>> {
    let pool = &app_state.sqlite_store.pool;
    
    // Get all sessions with their message counts
    // Let the summarization engine decide what to do with them
    let rows = sqlx::query(
        r#"
        SELECT session_id, COUNT(*) as message_count
        FROM memory_entries
        WHERE role IN ('user', 'assistant')
        GROUP BY session_id
        HAVING message_count >= 10  -- Minimum threshold for any summary
        ORDER BY message_count DESC
        "#
    )
    .fetch_all(pool)
    .await?;
    
    let candidates: Vec<(String, usize)> = rows
        .iter()
        .map(|row| {
            let session_id: String = row.get("session_id");
            let count: i64 = row.get("message_count");
            (session_id, count as usize)
        })
        .collect();
    
    Ok(candidates)
}
