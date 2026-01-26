//! Test utilities for Mira integration tests

use mira::{db::pool::DatabasePool, llm::ProviderFactory, embeddings::EmbeddingClient, llm::DeepSeekClient, background::watcher::WatcherHandle};
use mira_types::{ProjectContext, WsEvent};
use std::sync::Arc;
use tokio::sync::{RwLock, oneshot};
use std::collections::HashMap;
use async_trait::async_trait;
use uuid::Uuid;

/// Test context that implements ToolContext for integration testing
pub struct TestContext {
    pool: Arc<DatabasePool>,
    llm_factory: Arc<ProviderFactory>,
    project_state: Arc<RwLock<Option<ProjectContext>>>,
    session_state: Arc<RwLock<Option<String>>>,
    branch_state: Arc<RwLock<Option<String>>>,
}

#[allow(dead_code)]
impl TestContext {
    /// Create a new test context with in-memory database
    pub async fn new() -> Self {
        // Create pool with in-memory database
        let pool = Arc::new(DatabasePool::open_in_memory().await.expect("Failed to create in-memory pool"));

        // Create LLM factory (will have no clients since no API keys are set in test env)
        let llm_factory = Arc::new(ProviderFactory::new());

        Self {
            pool,
            llm_factory,
            project_state: Arc::new(RwLock::new(None)),
            session_state: Arc::new(RwLock::new(None)),
            branch_state: Arc::new(RwLock::new(None)),
        }
    }

    /// Get a reference to the pool
    pub fn pool(&self) -> &Arc<DatabasePool> {
        &self.pool
    }

    /// Get a reference to the LLM factory
    pub fn llm_factory(&self) -> &Arc<ProviderFactory> {
        &self.llm_factory
    }

    /// Clear project state (useful for tests that need fresh state)
    pub async fn clear_project(&self) {
        *self.project_state.write().await = None;
    }

    /// Clear session state
    pub async fn clear_session(&self) {
        *self.session_state.write().await = None;
    }
}

#[async_trait]
impl mira::tools::core::ToolContext for TestContext {
    fn pool(&self) -> &Arc<DatabasePool> {
        &self.pool
    }

    fn embeddings(&self) -> Option<&Arc<EmbeddingClient>> {
        None // No embeddings client for tests
    }

    fn deepseek(&self) -> Option<&Arc<DeepSeekClient>> {
        None // No DeepSeek client for tests
    }

    fn llm_factory(&self) -> &ProviderFactory {
        &self.llm_factory
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

    fn broadcast(&self, _event: WsEvent) {
        // No-op for tests
    }

    fn pending_responses(&self) -> Option<&Arc<RwLock<HashMap<String, oneshot::Sender<String>>>>> {
        None
    }

    fn watcher(&self) -> Option<&WatcherHandle> {
        None
    }
}
