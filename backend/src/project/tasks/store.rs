// backend/src/project/tasks/store.rs
// Database operations for project tasks

use super::types::{ProjectTask, ProjectTaskStatus, TaskContext, TaskSession};
use anyhow::Result;
use chrono::Utc;
use sqlx::{Row, SqlitePool};

/// Database store for project tasks
pub struct ProjectTaskStore {
    pool: SqlitePool,
}

impl ProjectTaskStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    // =========================================================================
    // Task CRUD
    // =========================================================================

    /// Create a new task
    pub async fn create(&self, task: &ProjectTask) -> Result<i64> {
        let tags_json = serde_json::to_string(&task.tags)?;

        let id = sqlx::query(
            "INSERT INTO project_tasks
            (project_id, parent_task_id, user_id, title, description, status, priority,
             complexity_estimate, time_estimate_minutes, tags, created_at, updated_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&task.project_id)
        .bind(task.parent_task_id)
        .bind(&task.user_id)
        .bind(&task.title)
        .bind(&task.description)
        .bind(task.status.as_str())
        .bind(task.priority)
        .bind(task.complexity_estimate)
        .bind(task.time_estimate_minutes)
        .bind(&tags_json)
        .bind(task.created_at)
        .bind(task.updated_at)
        .execute(&self.pool)
        .await?
        .last_insert_rowid();

        Ok(id)
    }

    /// Get task by ID
    pub async fn get(&self, task_id: i64) -> Result<Option<ProjectTask>> {
        let row = sqlx::query(
            "SELECT id, project_id, parent_task_id, user_id, title, description, status,
             priority, complexity_estimate, time_estimate_minutes, tags,
             created_at, updated_at, started_at, completed_at
             FROM project_tasks WHERE id = ?",
        )
        .bind(task_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| self.row_to_task(&r)))
    }

    /// List all tasks for a project
    pub async fn list_by_project(&self, project_id: &str) -> Result<Vec<ProjectTask>> {
        let rows = sqlx::query(
            "SELECT id, project_id, parent_task_id, user_id, title, description, status,
             priority, complexity_estimate, time_estimate_minutes, tags,
             created_at, updated_at, started_at, completed_at
             FROM project_tasks WHERE project_id = ? ORDER BY priority DESC, created_at DESC",
        )
        .bind(project_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.iter().map(|r| self.row_to_task(r)).collect())
    }

    /// List incomplete tasks (pending or in_progress) for a project
    pub async fn list_incomplete(&self, project_id: &str) -> Result<Vec<ProjectTask>> {
        let rows = sqlx::query(
            "SELECT id, project_id, parent_task_id, user_id, title, description, status,
             priority, complexity_estimate, time_estimate_minutes, tags,
             created_at, updated_at, started_at, completed_at
             FROM project_tasks
             WHERE project_id = ? AND status IN ('pending', 'in_progress')
             ORDER BY priority DESC, created_at DESC",
        )
        .bind(project_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.iter().map(|r| self.row_to_task(r)).collect())
    }

    /// Update a task
    pub async fn update(&self, task: &ProjectTask) -> Result<()> {
        let tags_json = serde_json::to_string(&task.tags)?;
        let now = Utc::now().timestamp();

        sqlx::query(
            "UPDATE project_tasks
             SET title = ?, description = ?, status = ?, priority = ?,
                 complexity_estimate = ?, time_estimate_minutes = ?, tags = ?,
                 updated_at = ?, started_at = ?, completed_at = ?
             WHERE id = ?",
        )
        .bind(&task.title)
        .bind(&task.description)
        .bind(task.status.as_str())
        .bind(task.priority)
        .bind(task.complexity_estimate)
        .bind(task.time_estimate_minutes)
        .bind(&tags_json)
        .bind(now)
        .bind(task.started_at)
        .bind(task.completed_at)
        .bind(task.id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Delete a task
    pub async fn delete(&self, task_id: i64) -> Result<bool> {
        let result = sqlx::query("DELETE FROM project_tasks WHERE id = ?")
            .bind(task_id)
            .execute(&self.pool)
            .await?;

        Ok(result.rows_affected() > 0)
    }

    // =========================================================================
    // Status Transitions
    // =========================================================================

    /// Start a task (set status to in_progress)
    pub async fn start_task(&self, task_id: i64) -> Result<()> {
        let now = Utc::now().timestamp();
        sqlx::query(
            "UPDATE project_tasks
             SET status = ?, started_at = COALESCE(started_at, ?), updated_at = ?
             WHERE id = ?",
        )
        .bind(ProjectTaskStatus::InProgress.as_str())
        .bind(now)
        .bind(now)
        .bind(task_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Complete a task
    pub async fn complete_task(&self, task_id: i64) -> Result<()> {
        let now = Utc::now().timestamp();
        sqlx::query(
            "UPDATE project_tasks
             SET status = ?, completed_at = ?, updated_at = ?
             WHERE id = ?",
        )
        .bind(ProjectTaskStatus::Completed.as_str())
        .bind(now)
        .bind(now)
        .bind(task_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Block a task
    pub async fn block_task(&self, task_id: i64) -> Result<()> {
        let now = Utc::now().timestamp();
        sqlx::query("UPDATE project_tasks SET status = ?, updated_at = ? WHERE id = ?")
            .bind(ProjectTaskStatus::Blocked.as_str())
            .bind(now)
            .bind(task_id)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    // =========================================================================
    // Task Sessions
    // =========================================================================

    /// Create a work session for a task
    pub async fn create_session(
        &self,
        task_id: i64,
        session_id: &str,
        user_id: Option<&str>,
    ) -> Result<i64> {
        let now = Utc::now().timestamp();
        let id = sqlx::query(
            "INSERT INTO task_sessions (task_id, session_id, user_id, started_at, files_modified, commits)
             VALUES (?, ?, ?, ?, '[]', '[]')",
        )
        .bind(task_id)
        .bind(session_id)
        .bind(user_id)
        .bind(now)
        .execute(&self.pool)
        .await?
        .last_insert_rowid();

        Ok(id)
    }

    /// End a work session
    pub async fn end_session(&self, session_id: i64, progress_notes: Option<&str>) -> Result<()> {
        let now = Utc::now().timestamp();
        sqlx::query("UPDATE task_sessions SET ended_at = ?, progress_notes = ? WHERE id = ?")
            .bind(now)
            .bind(progress_notes)
            .bind(session_id)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    /// Get active session for a task (one without ended_at)
    pub async fn get_active_session(&self, task_id: i64) -> Result<Option<TaskSession>> {
        let row = sqlx::query(
            "SELECT id, task_id, session_id, user_id, started_at, ended_at,
             progress_notes, files_modified, commits
             FROM task_sessions WHERE task_id = ? AND ended_at IS NULL
             ORDER BY started_at DESC LIMIT 1",
        )
        .bind(task_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| self.row_to_session(&r)))
    }

    /// Get active session by conversation session_id
    pub async fn get_session_by_conversation(
        &self,
        session_id: &str,
    ) -> Result<Option<TaskSession>> {
        let row = sqlx::query(
            "SELECT id, task_id, session_id, user_id, started_at, ended_at,
             progress_notes, files_modified, commits
             FROM task_sessions WHERE session_id = ? AND ended_at IS NULL
             ORDER BY started_at DESC LIMIT 1",
        )
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| self.row_to_session(&r)))
    }

    /// Add file to session's files_modified list
    pub async fn add_session_file(&self, session_id: i64, file_path: &str) -> Result<()> {
        // Get current files
        let row = sqlx::query("SELECT files_modified FROM task_sessions WHERE id = ?")
            .bind(session_id)
            .fetch_optional(&self.pool)
            .await?;

        if let Some(row) = row {
            let files_json: String = row.get("files_modified");
            let mut files: Vec<String> = serde_json::from_str(&files_json).unwrap_or_default();

            if !files.contains(&file_path.to_string()) {
                files.push(file_path.to_string());
                let new_json = serde_json::to_string(&files)?;
                sqlx::query("UPDATE task_sessions SET files_modified = ? WHERE id = ?")
                    .bind(&new_json)
                    .bind(session_id)
                    .execute(&self.pool)
                    .await?;
            }
        }

        Ok(())
    }

    /// Add commit to session's commits list
    pub async fn add_session_commit(&self, session_id: i64, commit_sha: &str) -> Result<()> {
        // Get current commits
        let row = sqlx::query("SELECT commits FROM task_sessions WHERE id = ?")
            .bind(session_id)
            .fetch_optional(&self.pool)
            .await?;

        if let Some(row) = row {
            let commits_json: String = row.get("commits");
            let mut commits: Vec<String> = serde_json::from_str(&commits_json).unwrap_or_default();

            if !commits.contains(&commit_sha.to_string()) {
                commits.push(commit_sha.to_string());
                let new_json = serde_json::to_string(&commits)?;
                sqlx::query("UPDATE task_sessions SET commits = ? WHERE id = ?")
                    .bind(&new_json)
                    .bind(session_id)
                    .execute(&self.pool)
                    .await?;
            }
        }

        Ok(())
    }

    /// List sessions for a task
    pub async fn list_sessions(&self, task_id: i64) -> Result<Vec<TaskSession>> {
        let rows = sqlx::query(
            "SELECT id, task_id, session_id, user_id, started_at, ended_at,
             progress_notes, files_modified, commits
             FROM task_sessions WHERE task_id = ? ORDER BY started_at DESC",
        )
        .bind(task_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.iter().map(|r| self.row_to_session(r)).collect())
    }

    // =========================================================================
    // Task Context
    // =========================================================================

    /// Add context to a task
    pub async fn add_context(
        &self,
        task_id: i64,
        context_type: &str,
        context_data: &str,
    ) -> Result<i64> {
        let now = Utc::now().timestamp();
        let id = sqlx::query(
            "INSERT INTO task_context (task_id, context_type, context_data, created_at)
             VALUES (?, ?, ?, ?)",
        )
        .bind(task_id)
        .bind(context_type)
        .bind(context_data)
        .bind(now)
        .execute(&self.pool)
        .await?
        .last_insert_rowid();

        Ok(id)
    }

    /// Get all context for a task
    pub async fn get_context(&self, task_id: i64) -> Result<Vec<TaskContext>> {
        let rows = sqlx::query(
            "SELECT id, task_id, context_type, context_data, created_at
             FROM task_context WHERE task_id = ? ORDER BY created_at DESC",
        )
        .bind(task_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .iter()
            .map(|r| TaskContext {
                id: r.get("id"),
                task_id: r.get("task_id"),
                context_type: r.get("context_type"),
                context_data: r.get("context_data"),
                created_at: r.get("created_at"),
            })
            .collect())
    }

    /// Get context by type for a task
    pub async fn get_context_by_type(
        &self,
        task_id: i64,
        context_type: &str,
    ) -> Result<Vec<TaskContext>> {
        let rows = sqlx::query(
            "SELECT id, task_id, context_type, context_data, created_at
             FROM task_context WHERE task_id = ? AND context_type = ?
             ORDER BY created_at DESC",
        )
        .bind(task_id)
        .bind(context_type)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .iter()
            .map(|r| TaskContext {
                id: r.get("id"),
                task_id: r.get("task_id"),
                context_type: r.get("context_type"),
                context_data: r.get("context_data"),
                created_at: r.get("created_at"),
            })
            .collect())
    }

    // =========================================================================
    // Helpers
    // =========================================================================

    fn row_to_task(&self, row: &sqlx::sqlite::SqliteRow) -> ProjectTask {
        let status_str: String = row.get("status");
        let tags_json: Option<String> = row.get("tags");
        let tags: Vec<String> = tags_json
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();

        ProjectTask {
            id: row.get("id"),
            project_id: row.get("project_id"),
            parent_task_id: row.get("parent_task_id"),
            user_id: row.get("user_id"),
            title: row.get("title"),
            description: row.get("description"),
            status: ProjectTaskStatus::from_str(&status_str),
            priority: row.get("priority"),
            complexity_estimate: row.get("complexity_estimate"),
            time_estimate_minutes: row.get("time_estimate_minutes"),
            tags,
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
            started_at: row.get("started_at"),
            completed_at: row.get("completed_at"),
        }
    }

    fn row_to_session(&self, row: &sqlx::sqlite::SqliteRow) -> TaskSession {
        let files_json: Option<String> = row.get("files_modified");
        let commits_json: Option<String> = row.get("commits");

        TaskSession {
            id: row.get("id"),
            task_id: row.get("task_id"),
            session_id: row.get("session_id"),
            user_id: row.get("user_id"),
            started_at: row.get("started_at"),
            ended_at: row.get("ended_at"),
            progress_notes: row.get("progress_notes"),
            files_modified: files_json
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default(),
            commits: commits_json
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default(),
        }
    }
}
