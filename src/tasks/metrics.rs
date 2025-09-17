// src/tasks/metrics.rs

//! Task metrics tracking

use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use std::collections::HashMap;
use parking_lot::RwLock;
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
        
        for (task, count) in processed.iter() {
            let processed = count.load(Ordering::Relaxed);
            let error_count = errors
                .get(task)
                .map(|c| c.load(Ordering::Relaxed))
                .unwrap_or(0);
            
            info!(
                "Task '{}': processed={}, errors={}", 
                task, processed, error_count
            );
        }
    }
}
