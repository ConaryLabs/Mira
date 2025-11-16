// src/operations/tasks/mod.rs
// Task tracking for operations

pub mod store;
pub mod types;

pub use store::TaskStore;
pub use types::{OperationTask, TaskStatus};

use crate::operations::engine::events::OperationEngineEvent;
use anyhow::Result;
use sqlx::SqlitePool;
use std::sync::Arc;
use tokio::sync::mpsc;

/// Manager for creating and tracking operation tasks
pub struct TaskManager {
    store: TaskStore,
}

impl TaskManager {
    pub fn new(db: Arc<SqlitePool>) -> Self {
        Self {
            store: TaskStore::new(db),
        }
    }

    /// Create a new task and emit TaskCreated event
    pub async fn create_task(
        &self,
        operation_id: &str,
        sequence: i32,
        description: String,
        active_form: String,
        event_tx: &mpsc::Sender<OperationEngineEvent>,
    ) -> Result<String> {
        let task = OperationTask::new(
            operation_id.to_string(),
            sequence,
            description.clone(),
            active_form.clone(),
        );

        self.store.create(&task).await?;

        // Emit TaskCreated event
        let _ = event_tx
            .send(OperationEngineEvent::TaskCreated {
                operation_id: operation_id.to_string(),
                task_id: task.id.clone(),
                sequence,
                description,
                active_form,
            })
            .await;

        Ok(task.id)
    }

    /// Start a task (set status to in_progress) and emit TaskStarted event
    pub async fn start_task(
        &self,
        operation_id: &str,
        task_id: &str,
        event_tx: &mpsc::Sender<OperationEngineEvent>,
    ) -> Result<()> {
        self.store.start(task_id).await?;

        // Emit TaskStarted event
        let _ = event_tx
            .send(OperationEngineEvent::TaskStarted {
                operation_id: operation_id.to_string(),
                task_id: task_id.to_string(),
            })
            .await;

        Ok(())
    }

    /// Complete a task (set status to completed) and emit TaskCompleted event
    pub async fn complete_task(
        &self,
        operation_id: &str,
        task_id: &str,
        event_tx: &mpsc::Sender<OperationEngineEvent>,
    ) -> Result<()> {
        self.store.complete(task_id).await?;

        // Emit TaskCompleted event
        let _ = event_tx
            .send(OperationEngineEvent::TaskCompleted {
                operation_id: operation_id.to_string(),
                task_id: task_id.to_string(),
            })
            .await;

        Ok(())
    }

    /// Fail a task (set status to failed) and emit TaskFailed event
    pub async fn fail_task(
        &self,
        operation_id: &str,
        task_id: &str,
        error: String,
        event_tx: &mpsc::Sender<OperationEngineEvent>,
    ) -> Result<()> {
        self.store.fail(task_id, &error).await?;

        // Emit TaskFailed event
        let _ = event_tx
            .send(OperationEngineEvent::TaskFailed {
                operation_id: operation_id.to_string(),
                task_id: task_id.to_string(),
                error,
            })
            .await;

        Ok(())
    }

    /// Get all tasks for an operation
    pub async fn get_tasks(&self, operation_id: &str) -> Result<Vec<OperationTask>> {
        self.store.get_by_operation(operation_id).await
    }

    /// Get a single task by ID
    pub async fn get_task(&self, task_id: &str) -> Result<Option<OperationTask>> {
        self.store.get(task_id).await
    }

    /// Delete a task
    pub async fn delete_task(&self, task_id: &str) -> Result<()> {
        self.store.delete(task_id).await
    }

    /// Update task status
    pub async fn update_status(&self, task_id: &str, status: TaskStatus) -> Result<()> {
        self.store.update_status(task_id, status).await
    }

    /// Get progress summary for an operation
    pub async fn get_progress(&self, operation_id: &str) -> Result<TaskProgress> {
        let tasks = self.store.get_by_operation(operation_id).await?;
        let total = tasks.len();
        let completed = tasks
            .iter()
            .filter(|t| t.status == TaskStatus::Completed)
            .count();
        let failed = tasks
            .iter()
            .filter(|t| t.status == TaskStatus::Failed)
            .count();
        let in_progress = tasks
            .iter()
            .filter(|t| t.status == TaskStatus::InProgress)
            .count();
        let pending = tasks
            .iter()
            .filter(|t| t.status == TaskStatus::Pending)
            .count();

        Ok(TaskProgress {
            total,
            completed,
            failed,
            in_progress,
            pending,
        })
    }
}

/// Progress summary for tasks
#[derive(Debug, Clone, serde::Serialize)]
pub struct TaskProgress {
    pub total: usize,
    pub completed: usize,
    pub failed: usize,
    pub in_progress: usize,
    pub pending: usize,
}
