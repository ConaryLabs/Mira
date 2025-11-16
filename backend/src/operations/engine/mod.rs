// src/operations/engine/mod.rs
// Operation Engine - orchestrates coding workflows with GPT-5 + DeepSeek delegation
// Refactored into focused modules for maintainability

pub mod artifacts;
pub mod context;
pub mod delegation;
pub mod events;
pub mod external_handlers;
pub mod file_handlers;
pub mod git_handlers;
pub mod lifecycle;
pub mod orchestration;
pub mod simple_mode;
pub mod skills;
pub mod tool_router;

pub use events::OperationEngineEvent;

use crate::git::client::GitClient;
use crate::llm::provider::deepseek::DeepSeekProvider;
use crate::llm::provider::gpt5::Gpt5Provider;
use crate::memory::service::MemoryService;
use crate::operations::{Artifact, Operation, OperationEvent};
use crate::relationship::service::RelationshipService;

use anyhow::Result;
use sqlx::SqlitePool;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::operations::ContextLoader;
use artifacts::ArtifactManager;
use context::ContextBuilder;
use delegation::DelegationHandler;
use lifecycle::LifecycleManager;
use orchestration::Orchestrator;
use simple_mode::{SimpleModeDetector, SimpleModeExecutor};
use tool_router::ToolRouter;

/// Main operation engine coordinating GPT-5 and DeepSeek
pub struct OperationEngine {
    lifecycle_manager: LifecycleManager,
    artifact_manager: ArtifactManager,
    orchestrator: Orchestrator,
    simple_mode_executor: SimpleModeExecutor,
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

        let context_loader = ContextLoader::new(git_client.clone(), Arc::clone(&code_intelligence));

        let delegation_handler = DelegationHandler::new(deepseek.clone());

        // Create tool router for file operations
        // TODO: Get project directory from git_client or config
        // For now, use current working directory as fallback
        let project_dir = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        let tool_router = Some(ToolRouter::new(deepseek, project_dir));

        let artifact_manager = ArtifactManager::new(Arc::clone(&db));
        let lifecycle_manager = LifecycleManager::new(Arc::clone(&db), Arc::clone(&memory_service));

        let simple_mode_executor = SimpleModeExecutor::new(gpt5.clone());

        // Initialize skill registry
        // Get skills directory: backend/skills or ./skills
        let skills_dir = std::env::current_dir()
            .unwrap_or_else(|_| std::path::PathBuf::from("."))
            .join("skills");

        let skill_registry = Arc::new(skills::SkillRegistry::new(skills_dir));

        // Spawn background task to load skills
        let registry_clone = Arc::clone(&skill_registry);
        tokio::spawn(async move {
            if let Err(e) = registry_clone.load_all().await {
                tracing::warn!("[ENGINE] Failed to load skills: {}", e);
            } else {
                tracing::info!("[ENGINE] Skills loaded successfully");
            }
        });

        let orchestrator = Orchestrator::new(
            gpt5,
            memory_service,
            context_builder,
            context_loader,
            delegation_handler,
            tool_router,
            skill_registry,
            artifact_manager.clone(),
            lifecycle_manager.clone(),
        );

        Self {
            lifecycle_manager,
            artifact_manager,
            orchestrator,
            simple_mode_executor,
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
    /// Automatically detects if request is "simple" and uses fast path if appropriate
    pub async fn run_operation(
        &self,
        operation_id: &str,
        session_id: &str,
        user_content: &str,
        project_id: Option<&str>,
        cancel_token: Option<CancellationToken>,
        event_tx: &mpsc::Sender<OperationEngineEvent>,
    ) -> Result<()> {
        // Check if request is simple enough for fast path
        let simplicity = SimpleModeDetector::simplicity_score(user_content);

        if simplicity > 0.7 {
            // Use simple mode - skip full orchestration
            tracing::info!(
                "[ENGINE] Simple request detected (score: {:.2}), using fast path",
                simplicity
            );

            // Start operation (minimal tracking)
            self.lifecycle_manager
                .start_operation(operation_id, event_tx)
                .await?;

            // Execute simple request
            match self.simple_mode_executor.execute_simple(user_content).await {
                Ok(response) => {
                    // Stream response
                    let _ = event_tx
                        .send(OperationEngineEvent::Streaming {
                            operation_id: operation_id.to_string(),
                            content: response.clone(),
                        })
                        .await;

                    // Complete operation
                    self.lifecycle_manager
                        .complete_operation(
                            operation_id,
                            session_id,
                            Some(response),
                            event_tx,
                            vec![], // No artifacts
                        )
                        .await?;

                    Ok(())
                }
                Err(e) => {
                    // Fall back to full orchestration on error
                    tracing::warn!(
                        "[ENGINE] Simple mode failed: {}, falling back to full orchestration",
                        e
                    );
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
            }
        } else {
            // Use full orchestration
            tracing::info!(
                "[ENGINE] Complex request detected (score: {:.2}), using full orchestration",
                simplicity
            );
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
