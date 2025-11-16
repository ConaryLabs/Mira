// src/operations/tasks/store.rs
// Database operations for operation tasks

use super::types::{OperationTask, TaskStatus};
use anyhow::Result;
use sqlx::{Row, SqlitePool};
use std::sync::Arc;

/// Database store for operation tasks
pub struct TaskStore {
    db: Arc<SqlitePool>,
}

impl TaskStore {
    pub fn new(db: Arc<SqlitePool>) -> Self {
        Self { db }
    }

    /// Create a new task
    pub async fn create(&self, task: &OperationTask) -> Result<()> {
        sqlx::query(
            "INSERT INTO operation_tasks
            (id, operation_id, sequence, description, active_form, status, created_at)
            VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&task.id)
        .bind(&task.operation_id)
        .bind(task.sequence)
        .bind(&task.description)
        .bind(&task.active_form)
        .bind(task.status.as_str())
        .bind(task.created_at)
        .execute(&*self.db)
        .await?;

        Ok(())
    }

    /// Get task by ID
    pub async fn get(&self, task_id: &str) -> Result<Option<OperationTask>> {
        let row = sqlx::query(
            "SELECT id, operation_id, sequence, description, active_form, status,
             created_at, started_at, completed_at, error_message
             FROM operation_tasks WHERE id = ?",
        )
        .bind(task_id)
        .fetch_optional(&*self.db)
        .await?;

        Ok(row.map(|r| self.row_to_task(&r)))
    }

    /// Get all tasks for an operation, ordered by sequence
    pub async fn get_by_operation(&self, operation_id: &str) -> Result<Vec<OperationTask>> {
        let rows = sqlx::query(
            "SELECT id, operation_id, sequence, description, active_form, status,
             created_at, started_at, completed_at, error_message
             FROM operation_tasks WHERE operation_id = ? ORDER BY sequence ASC",
        )
        .bind(operation_id)
        .fetch_all(&*self.db)
        .await?;

        Ok(rows.iter().map(|r| self.row_to_task(r)).collect())
    }

    /// Update task status to in_progress
    pub async fn start(&self, task_id: &str) -> Result<()> {
        let now = chrono::Utc::now().timestamp();
        sqlx::query(
            "UPDATE operation_tasks
             SET status = ?, started_at = ?
             WHERE id = ?",
        )
        .bind(TaskStatus::InProgress.as_str())
        .bind(now)
        .bind(task_id)
        .execute(&*self.db)
        .await?;

        Ok(())
    }

    /// Update task status to completed
    pub async fn complete(&self, task_id: &str) -> Result<()> {
        let now = chrono::Utc::now().timestamp();
        sqlx::query(
            "UPDATE operation_tasks
             SET status = ?, completed_at = ?
             WHERE id = ?",
        )
        .bind(TaskStatus::Completed.as_str())
        .bind(now)
        .bind(task_id)
        .execute(&*self.db)
        .await?;

        Ok(())
    }

    /// Update task status to failed with error message
    pub async fn fail(&self, task_id: &str, error: &str) -> Result<()> {
        let now = chrono::Utc::now().timestamp();
        sqlx::query(
            "UPDATE operation_tasks
             SET status = ?, completed_at = ?, error_message = ?
             WHERE id = ?",
        )
        .bind(TaskStatus::Failed.as_str())
        .bind(now)
        .bind(error)
        .bind(task_id)
        .execute(&*self.db)
        .await?;

        Ok(())
    }

    /// Update task status
    pub async fn update_status(&self, task_id: &str, status: TaskStatus) -> Result<()> {
        sqlx::query("UPDATE operation_tasks SET status = ? WHERE id = ?")
            .bind(status.as_str())
            .bind(task_id)
            .execute(&*self.db)
            .await?;

        Ok(())
    }

    /// Delete a task
    pub async fn delete(&self, task_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM operation_tasks WHERE id = ?")
            .bind(task_id)
            .execute(&*self.db)
            .await?;

        Ok(())
    }

    /// Delete all tasks for an operation
    pub async fn delete_by_operation(&self, operation_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM operation_tasks WHERE operation_id = ?")
            .bind(operation_id)
            .execute(&*self.db)
            .await?;

        Ok(())
    }

    /// Get count of tasks by status for an operation
    pub async fn count_by_status(
        &self,
        operation_id: &str,
        status: TaskStatus,
    ) -> Result<i64> {
        let row = sqlx::query(
            "SELECT COUNT(*) as count FROM operation_tasks
             WHERE operation_id = ? AND status = ?",
        )
        .bind(operation_id)
        .bind(status.as_str())
        .fetch_one(&*self.db)
        .await?;

        Ok(row.get("count"))
    }

    /// Helper to convert row to OperationTask
    fn row_to_task(&self, row: &sqlx::sqlite::SqliteRow) -> OperationTask {
        let status_str: String = row.get("status");
        let status = match status_str.as_str() {
            "pending" => TaskStatus::Pending,
            "in_progress" => TaskStatus::InProgress,
            "completed" => TaskStatus::Completed,
            "failed" => TaskStatus::Failed,
            _ => TaskStatus::Pending,
        };

        OperationTask {
            id: row.get("id"),
            operation_id: row.get("operation_id"),
            sequence: row.get("sequence"),
            description: row.get("description"),
            active_form: row.get("active_form"),
            status,
            created_at: row.get("created_at"),
            started_at: row.get("started_at"),
            completed_at: row.get("completed_at"),
            error_message: row.get("error_message"),
        }
    }
}
