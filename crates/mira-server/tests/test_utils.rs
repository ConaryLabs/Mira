//! Test utilities for Mira integration tests

use async_trait::async_trait;
use mira::{
    background::watcher::WatcherHandle, db::pool::DatabasePool, embeddings::EmbeddingClient,
};
use mira_types::ProjectContext;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

/// Test context that implements ToolContext for integration testing
pub struct TestContext {
    pool: Arc<DatabasePool>,
    code_pool: Arc<DatabasePool>,
    project_state: Arc<RwLock<Option<ProjectContext>>>,
    session_state: Arc<RwLock<Option<String>>>,
    branch_state: Arc<RwLock<Option<String>>>,
}

impl TestContext {
    /// Create a new test context with in-memory database
    pub async fn new() -> Self {
        // Create pools with in-memory databases
        let pool = Arc::new(
            DatabasePool::open_in_memory()
                .await
                .expect("Failed to create in-memory pool"),
        );
        let code_pool = Arc::new(
            DatabasePool::open_code_db_in_memory()
                .await
                .expect("Failed to create in-memory code pool"),
        );

        Self {
            pool,
            code_pool,
            project_state: Arc::new(RwLock::new(None)),
            session_state: Arc::new(RwLock::new(None)),
            branch_state: Arc::new(RwLock::new(None)),
        }
    }

    /// Get a reference to the pool
    pub fn pool(&self) -> &Arc<DatabasePool> {
        &self.pool
    }
}

#[async_trait]
impl mira::tools::core::ToolContext for TestContext {
    fn pool(&self) -> &Arc<DatabasePool> {
        &self.pool
    }

    fn code_pool(&self) -> &Arc<DatabasePool> {
        &self.code_pool
    }

    fn embeddings(&self) -> Option<&Arc<EmbeddingClient>> {
        None // No embeddings client for tests
    }

    async fn get_project(&self) -> Option<ProjectContext> {
        self.project_state.read().await.clone()
    }

    async fn set_project(&self, project: ProjectContext) {
        *self.project_state.write().await = Some(project);
    }

    async fn get_session_id(&self) -> Option<String> {
        self.session_state.read().await.clone()
    }

    async fn set_session_id(&self, session_id: String) {
        *self.session_state.write().await = Some(session_id);
    }

    async fn get_or_create_session(&self) -> String {
        if let Some(existing_id) = self.get_session_id().await {
            return existing_id;
        }

        let new_id = Uuid::new_v4().to_string();
        self.set_session_id(new_id.clone()).await;
        new_id
    }

    async fn get_branch(&self) -> Option<String> {
        self.branch_state.read().await.clone()
    }

    async fn set_branch(&self, branch: Option<String>) {
        *self.branch_state.write().await = branch;
    }

    fn watcher(&self) -> Option<&WatcherHandle> {
        None
    }
}
