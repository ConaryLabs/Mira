// src/tasks/metrics.rs

//! Task metrics tracking

use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tracing::info;

pub struct TaskMetrics {
    processed: RwLock<HashMap<String, AtomicUsize>>,
    errors: RwLock<HashMap<String, AtomicUsize>>,
    durations: RwLock<HashMap<String, Vec<Duration>>>,
}

impl TaskMetrics {
    pub fn new() -> Self {
        Self {
            processed: RwLock::new(HashMap::new()),
            errors: RwLock::new(HashMap::new()),
            durations: RwLock::new(HashMap::new()),
        }
    }

    pub fn add_processed_items(&self, task: &str, count: usize) {
        let mut map = self.processed.write();
        map.entry(task.to_string())
            .or_insert_with(|| AtomicUsize::new(0))
            .fetch_add(count, Ordering::Relaxed);
    }

    pub fn record_error(&self, task: &str) {
        let mut map = self.errors.write();
        map.entry(task.to_string())
            .or_insert_with(|| AtomicUsize::new(0))
            .fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_task_duration(&self, task: &str, duration: Duration) {
        let mut map = self.durations.write();
        map.entry(task.to_string())
            .or_insert_with(Vec::new)
            .push(duration);
    }

    pub fn report(&self) {
        let processed = self.processed.read();
        let errors = self.errors.read();

        let mut has_activity = false;

        for (task, count) in processed.iter() {
            let processed_count = count.load(Ordering::Relaxed);
            let error_count = errors
                .get(task)
                .map(|c| c.load(Ordering::Relaxed))
                .unwrap_or(0);

            // Only log tasks that have actually done work since last report
            if processed_count > 0 || error_count > 0 {
                info!(
                    "Task '{}': processed={}, errors={}",
                    task, processed_count, error_count
                );
                has_activity = true;
            }
        }

        if !has_activity {
            info!("All background tasks idle - no activity in past hour");
        }

        // Reset counters after reporting to show incremental progress
        self.reset_counters();
    }

    /// Reset all counters after reporting to show incremental progress
    fn reset_counters(&self) {
        let processed = self.processed.read();
        let errors = self.errors.read();

        for (_, count) in processed.iter() {
            count.store(0, Ordering::Relaxed);
        }

        for (_, count) in errors.iter() {
            count.store(0, Ordering::Relaxed);
        }

        // Clear duration history to prevent memory growth
        let mut durations = self.durations.write();
        durations.clear();
    }
}
