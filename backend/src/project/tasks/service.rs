// backend/src/project/tasks/service.rs
// Business logic for project tasks

use super::store::ProjectTaskStore;
use super::types::{
    context_types, NewProjectTask, ProjectTask, ProjectTaskStatus, TaskContext, TaskPriority,
    TaskSession,
};
use anyhow::Result;
use chrono::Utc;
use sqlx::SqlitePool;
use tracing::info;

/// Service for managing persistent project tasks
pub struct ProjectTaskService {
    store: ProjectTaskStore,
}

impl ProjectTaskService {
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            store: ProjectTaskStore::new(pool),
        }
    }

    // =========================================================================
    // Task Lifecycle
    // =========================================================================

    /// Create a new task
    pub async fn create_task(&self, input: NewProjectTask) -> Result<ProjectTask> {
        let now = Utc::now().timestamp();

        let task = ProjectTask {
            id: 0, // Will be set by DB
            project_id: input.project_id,
            parent_task_id: input.parent_task_id,
            user_id: input.user_id,
            title: input.title,
            description: input.description,
            status: ProjectTaskStatus::Pending,
            priority: input.priority.to_int(),
            complexity_estimate: None,
            time_estimate_minutes: None,
            tags: input.tags,
            created_at: now,
            updated_at: now,
            started_at: None,
            completed_at: None,
        };

        let id = self.store.create(&task).await?;
        info!(task_id = id, "Created project task: {}", task.title);

        // Return task with correct ID
        Ok(ProjectTask { id, ..task })
    }

    /// Get a task by ID
    pub async fn get_task(&self, task_id: i64) -> Result<Option<ProjectTask>> {
        self.store.get(task_id).await
    }

    /// Start working on a task - creates a session and sets status to in_progress
    pub async fn start_task(
        &self,
        task_id: i64,
        session_id: &str,
        user_id: Option<&str>,
    ) -> Result<TaskSession> {
        // Update task status
        self.store.start_task(task_id).await?;

        // Create work session
        let session_row_id = self
            .store
            .create_session(task_id, session_id, user_id)
            .await?;

        info!(
            task_id = task_id,
            session_id = session_id,
            "Started work session on task"
        );

        // Return session
        Ok(TaskSession {
            id: session_row_id,
            task_id,
            session_id: session_id.to_string(),
            user_id: user_id.map(|s| s.to_string()),
            started_at: Utc::now().timestamp(),
            ended_at: None,
            progress_notes: None,
            files_modified: vec![],
            commits: vec![],
        })
    }

    /// Complete a task with optional summary
    pub async fn complete_task(&self, task_id: i64, summary: Option<&str>) -> Result<()> {
        // Complete the task
        self.store.complete_task(task_id).await?;

        // End any active session
        if let Some(session) = self.store.get_active_session(task_id).await? {
            self.store.end_session(session.id, summary).await?;
        }

        info!(task_id = task_id, "Completed task");
        Ok(())
    }

    /// Update progress notes on a task (via its active session)
    pub async fn update_progress(&self, task_id: i64, notes: &str) -> Result<()> {
        if let Some(session) = self.store.get_active_session(task_id).await? {
            self.store.end_session(session.id, Some(notes)).await?;
            // Re-open session (or we could just update the notes without ending)
        }

        // Also add as context note
        self.store
            .add_context(task_id, context_types::NOTE, notes)
            .await?;

        info!(task_id = task_id, "Updated task progress");
        Ok(())
    }

    /// Block a task
    pub async fn block_task(&self, task_id: i64, reason: Option<&str>) -> Result<()> {
        self.store.block_task(task_id).await?;

        if let Some(reason) = reason {
            self.store
                .add_context(
                    task_id,
                    context_types::NOTE,
                    &format!("Blocked: {}", reason),
                )
                .await?;
        }

        info!(task_id = task_id, "Blocked task");
        Ok(())
    }

    // =========================================================================
    // Artifact/Commit Linking
    // =========================================================================

    /// Link an artifact to a task
    pub async fn link_artifact(
        &self,
        task_id: i64,
        artifact_id: &str,
        file_path: &str,
    ) -> Result<()> {
        let data = serde_json::json!({
            "artifact_id": artifact_id,
            "file_path": file_path,
        });

        self.store
            .add_context(task_id, context_types::ARTIFACT, &data.to_string())
            .await?;

        // Also add file to active session
        if let Some(session) = self.store.get_active_session(task_id).await? {
            self.store.add_session_file(session.id, file_path).await?;
        }

        info!(
            task_id = task_id,
            artifact_id = artifact_id,
            "Linked artifact to task"
        );
        Ok(())
    }

    /// Link a commit to a task
    pub async fn link_commit(
        &self,
        task_id: i64,
        commit_sha: &str,
        message: &str,
    ) -> Result<()> {
        let data = serde_json::json!({
            "commit_sha": commit_sha,
            "message": message,
        });

        self.store
            .add_context(task_id, context_types::COMMIT, &data.to_string())
            .await?;

        // Also add commit to active session
        if let Some(session) = self.store.get_active_session(task_id).await? {
            self.store.add_session_commit(session.id, commit_sha).await?;
        }

        info!(
            task_id = task_id,
            commit_sha = commit_sha,
            "Linked commit to task"
        );
        Ok(())
    }

    // =========================================================================
    // Queries
    // =========================================================================

    /// Get all incomplete tasks for a project
    pub async fn get_incomplete_tasks(&self, project_id: &str) -> Result<Vec<ProjectTask>> {
        self.store.list_incomplete(project_id).await
    }

    /// Get all tasks for a project
    pub async fn get_all_tasks(&self, project_id: &str) -> Result<Vec<ProjectTask>> {
        self.store.list_by_project(project_id).await
    }

    /// Get task with all its context
    pub async fn get_task_with_context(
        &self,
        task_id: i64,
    ) -> Result<Option<(ProjectTask, Vec<TaskContext>)>> {
        let task = self.store.get(task_id).await?;
        if let Some(task) = task {
            let context = self.store.get_context(task_id).await?;
            Ok(Some((task, context)))
        } else {
            Ok(None)
        }
    }

    /// Get active task for a conversation session
    pub async fn get_active_task_for_session(
        &self,
        session_id: &str,
    ) -> Result<Option<ProjectTask>> {
        let session = self.store.get_session_by_conversation(session_id).await?;
        if let Some(session) = session {
            self.store.get(session.task_id).await
        } else {
            Ok(None)
        }
    }

    // =========================================================================
    // LLM Context Formatting
    // =========================================================================

    /// Format tasks for inclusion in system prompt
    pub async fn format_for_prompt(&self, project_id: &str) -> Result<Option<String>> {
        let tasks = self.store.list_incomplete(project_id).await?;

        if tasks.is_empty() {
            return Ok(None);
        }

        let mut output = String::new();
        output.push_str("Tasks for this project:\n\n");

        for (i, task) in tasks.iter().enumerate() {
            let status = match task.status {
                ProjectTaskStatus::InProgress => "[IN_PROGRESS]",
                ProjectTaskStatus::Blocked => "[BLOCKED]",
                _ => "[PENDING]",
            };

            let priority = match TaskPriority::from_int(task.priority) {
                TaskPriority::Critical => "critical",
                TaskPriority::High => "high",
                TaskPriority::Medium => "medium",
                TaskPriority::Low => "low",
            };

            output.push_str(&format!(
                "{}. {} \"{}\" ({} priority)\n",
                i + 1,
                status,
                task.title,
                priority
            ));

            if let Some(desc) = &task.description {
                output.push_str(&format!("   Description: {}\n", desc));
            }

            // Add timing info
            if let Some(started) = task.started_at {
                let elapsed = Utc::now().timestamp() - started;
                let hours = elapsed / 3600;
                let minutes = (elapsed % 3600) / 60;
                if hours > 0 {
                    output.push_str(&format!("   Started: {} hours ago\n", hours));
                } else {
                    output.push_str(&format!("   Started: {} minutes ago\n", minutes));
                }
            }

            // Get context (files, commits)
            if let Ok(context) = self.store.get_context(task.id).await {
                let files: Vec<_> = context
                    .iter()
                    .filter(|c| c.context_type == context_types::ARTIFACT)
                    .filter_map(|c| {
                        serde_json::from_str::<serde_json::Value>(&c.context_data)
                            .ok()
                            .and_then(|v| v.get("file_path")?.as_str().map(|s| s.to_string()))
                    })
                    .collect();

                if !files.is_empty() {
                    output.push_str(&format!("   Files: {}\n", files.join(", ")));
                }

                // Get latest progress note
                let notes: Vec<_> = context
                    .iter()
                    .filter(|c| c.context_type == context_types::NOTE)
                    .map(|c| c.context_data.clone())
                    .collect();

                if let Some(note) = notes.first() {
                    output.push_str(&format!("   Progress: {}\n", note));
                }
            }

            output.push('\n');
        }

        output.push_str(
            "Use manage_project_task tool to create/update/complete tasks as you work.\n",
        );

        Ok(Some(output))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn setup_test_db() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .connect("sqlite::memory:")
            .await
            .unwrap();

        // Create required tables
        sqlx::query(
            r#"
            CREATE TABLE project_tasks (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                project_id TEXT NOT NULL,
                parent_task_id INTEGER,
                user_id TEXT,
                title TEXT NOT NULL,
                description TEXT,
                status TEXT NOT NULL DEFAULT 'pending',
                priority INTEGER DEFAULT 0,
                complexity_estimate REAL,
                time_estimate_minutes INTEGER,
                tags TEXT,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                started_at INTEGER,
                completed_at INTEGER
            )
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            r#"
            CREATE TABLE task_sessions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                task_id INTEGER NOT NULL,
                session_id TEXT NOT NULL,
                user_id TEXT,
                started_at INTEGER NOT NULL,
                ended_at INTEGER,
                progress_notes TEXT,
                files_modified TEXT,
                commits TEXT
            )
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            r#"
            CREATE TABLE task_context (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                task_id INTEGER NOT NULL,
                context_type TEXT NOT NULL,
                context_data TEXT NOT NULL,
                created_at INTEGER NOT NULL
            )
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        pool
    }

    #[tokio::test]
    async fn test_create_and_get_task() {
        let pool = setup_test_db().await;
        let service = ProjectTaskService::new(pool);

        let input = NewProjectTask {
            project_id: "test-project".to_string(),
            title: "Implement feature X".to_string(),
            description: Some("Add the new feature".to_string()),
            priority: TaskPriority::High,
            tags: vec!["feature".to_string()],
            parent_task_id: None,
            user_id: None,
        };

        let task = service.create_task(input).await.unwrap();
        assert!(task.id > 0);
        assert_eq!(task.title, "Implement feature X");
        assert_eq!(task.status, ProjectTaskStatus::Pending);

        let fetched = service.get_task(task.id).await.unwrap();
        assert!(fetched.is_some());
        assert_eq!(fetched.unwrap().title, "Implement feature X");
    }

    #[tokio::test]
    async fn test_task_lifecycle() {
        let pool = setup_test_db().await;
        let service = ProjectTaskService::new(pool);

        // Create task
        let input = NewProjectTask {
            project_id: "test-project".to_string(),
            title: "Test task".to_string(),
            description: None,
            priority: TaskPriority::Medium,
            tags: vec![],
            parent_task_id: None,
            user_id: None,
        };

        let task = service.create_task(input).await.unwrap();
        assert_eq!(task.status, ProjectTaskStatus::Pending);

        // Start task
        let session = service
            .start_task(task.id, "conv-123", None)
            .await
            .unwrap();
        assert_eq!(session.task_id, task.id);

        let updated = service.get_task(task.id).await.unwrap().unwrap();
        assert_eq!(updated.status, ProjectTaskStatus::InProgress);

        // Complete task
        service
            .complete_task(task.id, Some("All done"))
            .await
            .unwrap();

        let completed = service.get_task(task.id).await.unwrap().unwrap();
        assert_eq!(completed.status, ProjectTaskStatus::Completed);
        assert!(completed.completed_at.is_some());
    }

    #[tokio::test]
    async fn test_artifact_linking() {
        let pool = setup_test_db().await;
        let service = ProjectTaskService::new(pool);

        let input = NewProjectTask {
            project_id: "test-project".to_string(),
            title: "Task with artifacts".to_string(),
            description: None,
            priority: TaskPriority::Medium,
            tags: vec![],
            parent_task_id: None,
            user_id: None,
        };

        let task = service.create_task(input).await.unwrap();

        // Link artifact
        service
            .link_artifact(task.id, "artifact-123", "src/main.rs")
            .await
            .unwrap();

        let (_, context) = service.get_task_with_context(task.id).await.unwrap().unwrap();
        assert_eq!(context.len(), 1);
        assert_eq!(context[0].context_type, "artifact");
    }
}
