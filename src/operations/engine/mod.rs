// src/operations/engine/mod.rs
// Operation Engine - orchestrates coding workflows with GPT-5 + DeepSeek delegation
// Refactored into focused modules for maintainability

pub mod events;
pub mod context;
pub mod delegation;
pub mod artifacts;
pub mod lifecycle;
pub mod orchestration;

pub use events::OperationEngineEvent;

use crate::llm::provider::gpt5::Gpt5Provider;
use crate::llm::provider::deepseek::DeepSeekProvider;
use crate::memory::service::MemoryService;
use crate::relationship::service::RelationshipService;
use crate::git::client::GitClient;
use crate::operations::{Operation, OperationEvent, Artifact};

use anyhow::Result;
use sqlx::SqlitePool;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use context::ContextBuilder;
use delegation::DelegationHandler;
use artifacts::ArtifactManager;
use lifecycle::LifecycleManager;
use orchestration::Orchestrator;

/// Main operation engine coordinating GPT-5 and DeepSeek
pub struct OperationEngine {
    lifecycle_manager: LifecycleManager,
    artifact_manager: ArtifactManager,
    orchestrator: Orchestrator,
}

impl OperationEngine {
    pub fn new(
        db: Arc<SqlitePool>,
        gpt5: Gpt5Provider,
        deepseek: DeepSeekProvider,
        memory_service: Arc<MemoryService>,
        relationship_service: Arc<RelationshipService>,
        git_client: GitClient,
        code_intelligence: Arc<crate::memory::features::code_intelligence::CodeIntelligenceService>,
    ) -> Self {
        // Build sub-components
        let context_builder = ContextBuilder::new(
            Arc::clone(&memory_service),
            Arc::clone(&relationship_service),
        );
        
        let delegation_handler = DelegationHandler::new(deepseek);
        let artifact_manager = ArtifactManager::new(Arc::clone(&db));
        let lifecycle_manager = LifecycleManager::new(Arc::clone(&db), Arc::clone(&memory_service));
        
        let orchestrator = Orchestrator::new(
            gpt5,
            memory_service,
            git_client,
            code_intelligence,
            context_builder,
            delegation_handler,
            artifact_manager.clone(),
            lifecycle_manager.clone(),
        );

        Self {
            lifecycle_manager,
            artifact_manager,
            orchestrator,
        }
    }

    // ========================================================================
    // Public API - Delegate to appropriate sub-components
    // ========================================================================

    /// Create a new operation
    pub async fn create_operation(
        &self,
        session_id: String,
        kind: String,
        user_message: String,
    ) -> Result<Operation> {
        self.lifecycle_manager.create_operation(session_id, kind, user_message).await
    }

    /// Execute an operation (main entry point)
    pub async fn run_operation(
        &self,
        operation_id: &str,
        session_id: &str,
        user_content: &str,
        project_id: Option<&str>,
        cancel_token: Option<CancellationToken>,
        event_tx: &mpsc::Sender<OperationEngineEvent>,
    ) -> Result<()> {
        self.orchestrator.run_operation(
            operation_id,
            session_id,
            user_content,
            project_id,
            cancel_token,
            event_tx,
        ).await
    }

    /// Get operation by ID
    pub async fn get_operation(&self, operation_id: &str) -> Result<Operation> {
        self.lifecycle_manager.get_operation(operation_id).await
    }

    /// Get operation events
    pub async fn get_operation_events(&self, operation_id: &str) -> Result<Vec<OperationEvent>> {
        self.lifecycle_manager.get_operation_events(operation_id).await
    }

    /// Get artifacts for operation
    pub async fn get_artifacts_for_operation(&self, operation_id: &str) -> Result<Vec<Artifact>> {
        self.artifact_manager.get_artifacts_for_operation(operation_id).await
    }
}
