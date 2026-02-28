// crates/mira-server/src/tools/core/test_utils.rs
// Shared test utilities for tool integration tests

use crate::db::pool::{CodePool, DatabasePool, MainPool};
use crate::tools::core::ToolContext;
use async_trait::async_trait;
use mira_types::ProjectContext;
use std::sync::Arc;
use tokio::sync::RwLock;

// ============================================================================
// MockToolContext
// ============================================================================

pub struct MockToolContext {
    pub pool: MainPool,
    pub code_pool: CodePool,
    project: RwLock<Option<ProjectContext>>,
    session_id: RwLock<Option<String>>,
    branch: RwLock<Option<String>>,
}

impl MockToolContext {
    pub async fn new() -> Self {
        let pool = MainPool::new(Arc::new(
            DatabasePool::open_in_memory()
                .await
                .expect("Failed to open in-memory pool"),
        ));
        let code_pool = CodePool::new(Arc::new(
            DatabasePool::open_code_db_in_memory()
                .await
                .expect("Failed to open in-memory code pool"),
        ));
        Self {
            pool,
            code_pool,
            project: RwLock::new(None),
            session_id: RwLock::new(None),
            branch: RwLock::new(None),
        }
    }

    /// Create a mock with a project already inserted into the DB.
    pub async fn with_project() -> Self {
        let ctx = Self::new().await;
        let project_id = ctx
            .pool
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO projects (path, name) VALUES (?1, ?2)",
                    rusqlite::params!["/test/project", "test-project"],
                )?;
                Ok::<_, rusqlite::Error>(conn.last_insert_rowid())
            })
            .await
            .expect("Failed to insert test project");

        *ctx.project.write().await = Some(ProjectContext {
            id: project_id,
            path: "/test/project".into(),
            name: Some("test-project".into()),
        });
        ctx
    }
}

#[async_trait]
impl ToolContext for MockToolContext {
    fn pool(&self) -> &MainPool {
        &self.pool
    }
    fn code_pool(&self) -> &CodePool {
        &self.code_pool
    }
    fn embeddings(&self) -> Option<&Arc<crate::embeddings::EmbeddingClient>> {
        None
    }
    async fn get_project(&self) -> Option<ProjectContext> {
        self.project.read().await.clone()
    }
    async fn set_project(&self, project: ProjectContext) {
        *self.project.write().await = Some(project);
    }
    async fn get_session_id(&self) -> Option<String> {
        self.session_id.read().await.clone()
    }
    async fn set_session_id(&self, session_id: String) {
        *self.session_id.write().await = Some(session_id);
    }
    async fn get_or_create_session(&self) -> String {
        if let Some(id) = self.get_session_id().await {
            return id;
        }
        let id = uuid::Uuid::new_v4().to_string();
        let id_clone = id.clone();
        let project_id = self.project_id().await;
        self.pool
            .run(move |conn| {
                crate::db::create_session_ext_sync(
                    conn,
                    &id_clone,
                    project_id,
                    Some("startup"),
                    None,
                )
            })
            .await
            .expect("MockToolContext: failed to persist session to DB");
        self.set_session_id(id.clone()).await;
        id
    }
    async fn get_branch(&self) -> Option<String> {
        self.branch.read().await.clone()
    }
    async fn set_branch(&self, branch: Option<String>) {
        *self.branch.write().await = branch;
    }
    fn get_user_identity(&self) -> Option<String> {
        Some("test-user".into())
    }
    fn get_team_membership(&self) -> Option<crate::hooks::session::TeamMembership> {
        None
    }
}
