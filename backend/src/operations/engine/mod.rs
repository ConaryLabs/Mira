// src/operations/engine/mod.rs
// Operation Engine - orchestrates coding workflows with LLM
// Refactored into focused modules for maintainability

pub mod artifacts;
pub mod code_handlers;
pub mod context;
pub mod delegation;
pub mod events;
pub mod external_handlers;
pub mod file_handlers;
pub mod git_handlers;
pub mod llm_orchestrator;
pub mod guidelines_handlers;
pub mod lifecycle;
pub mod orchestration;
pub mod skills;
pub mod task_handlers;
pub mod tool_router;

pub use events::OperationEngineEvent;

use crate::budget::BudgetTracker;
use crate::cache::LlmCache;
use crate::checkpoint::CheckpointManager;
use crate::context_oracle::ContextOracle;
use crate::git::client::GitClient;
use crate::hooks::HookManager;
use crate::llm::provider::LlmProvider;
use crate::llm::router::ModelRouter;
use crate::memory::service::MemoryService;
use crate::operations::{Artifact, Operation, OperationEvent};
use crate::project::guidelines::ProjectGuidelinesService;
use crate::project::ProjectTaskService;
use crate::relationship::service::RelationshipService;

use anyhow::Result;
use sqlx::SqlitePool;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

use crate::operations::ContextLoader;
use artifacts::ArtifactManager;
use context::ContextBuilder;
use lifecycle::LifecycleManager;
use orchestration::Orchestrator;
use tool_router::ToolRouter;

/// Main operation engine with LLM architecture
pub struct OperationEngine {
    lifecycle_manager: LifecycleManager,
    artifact_manager: ArtifactManager,
    orchestrator: Orchestrator,
}

impl OperationEngine {
    pub fn new(
        db: Arc<SqlitePool>,
        llm: Arc<dyn LlmProvider>, // Used by ToolRouter for tool-specific LLM calls
        model_router: Arc<ModelRouter>, // Used by LlmOrchestrator for multi-tier routing
        memory_service: Arc<MemoryService>,
        relationship_service: Arc<RelationshipService>,
        git_client: GitClient,
        code_intelligence: Arc<crate::memory::features::code_intelligence::CodeIntelligenceService>,
        sudo_service: Option<Arc<crate::sudo::SudoPermissionService>>,
        context_oracle: Option<Arc<ContextOracle>>,
        budget_tracker: Option<Arc<BudgetTracker>>,
        llm_cache: Option<Arc<LlmCache>>,
        project_task_service: Option<Arc<ProjectTaskService>>,
        guidelines_service: Option<Arc<ProjectGuidelinesService>>,
        hook_manager: Option<Arc<RwLock<HookManager>>>,
        checkpoint_manager: Option<Arc<CheckpointManager>>,
        project_store: Option<Arc<crate::project::ProjectStore>>,
    ) -> Self {
        // Build sub-components
        let mut context_builder = ContextBuilder::new(
            Arc::clone(&memory_service),
            Arc::clone(&relationship_service),
        );

        // Add Context Oracle if provided
        if let Some(oracle) = context_oracle {
            context_builder = context_builder.with_context_oracle(oracle);
        }

        // Add ProjectTaskService for task context injection (used by orchestrator)
        let project_task_service_for_context = project_task_service.clone();
        if let Some(task_service) = project_task_service_for_context {
            context_builder = context_builder.with_project_task_service(task_service);
        }

        let context_loader = ContextLoader::new(git_client.clone(), Arc::clone(&code_intelligence));

        // Create tool router for file operations and code intelligence
        let project_dir = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        let mut tool_router = ToolRouter::new(llm.clone(), project_dir, code_intelligence, sudo_service);

        // Add project task service if provided
        if let Some(task_service) = project_task_service {
            tool_router = tool_router.with_project_task_service(task_service);
        }

        // Add guidelines service if provided
        if let Some(guidelines_svc) = guidelines_service {
            tool_router = tool_router.with_guidelines_service(guidelines_svc);
        }

        // Add project store for dynamic working directory resolution
        if let Some(store) = project_store {
            tool_router = tool_router.with_project_store(store);
        }

        let tool_router_arc = Arc::new(tool_router);

        let artifact_manager = ArtifactManager::new(Arc::clone(&db));
        let lifecycle_manager = LifecycleManager::new(Arc::clone(&db), Arc::clone(&memory_service));

        // Create LLM orchestrator with multi-tier routing, budget tracking and caching
        use crate::operations::engine::llm_orchestrator::LlmOrchestrator;

        let llm_orchestrator = LlmOrchestrator::with_services(
            model_router,
            Some(Arc::clone(&tool_router_arc)),
            budget_tracker,
            llm_cache,
            hook_manager,
            checkpoint_manager,
        );

        let orchestrator = Orchestrator::new(
            Some(Arc::new(llm_orchestrator)),
            memory_service,
            context_builder,
            context_loader,
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
        self.lifecycle_manager
            .create_operation(session_id, kind, user_message)
            .await
    }

    /// Execute an operation (main entry point)
    ///
    /// Routes all requests to LLM orchestration
    pub async fn run_operation(
        &self,
        operation_id: &str,
        session_id: &str,
        user_content: &str,
        project_id: Option<&str>,
        cancel_token: Option<CancellationToken>,
        event_tx: &mpsc::Sender<OperationEngineEvent>,
    ) -> Result<()> {
        self.orchestrator
            .run_operation(
                operation_id,
                session_id,
                user_content,
                project_id,
                cancel_token,
                event_tx,
            )
            .await
    }

    /// Get operation by ID
    pub async fn get_operation(&self, operation_id: &str) -> Result<Operation> {
        self.lifecycle_manager.get_operation(operation_id).await
    }

    /// Get operation events
    pub async fn get_operation_events(&self, operation_id: &str) -> Result<Vec<OperationEvent>> {
        self.lifecycle_manager
            .get_operation_events(operation_id)
            .await
    }

    /// Get artifacts for operation
    pub async fn get_artifacts_for_operation(&self, operation_id: &str) -> Result<Vec<Artifact>> {
        self.artifact_manager
            .get_artifacts_for_operation(operation_id)
            .await
    }
}
