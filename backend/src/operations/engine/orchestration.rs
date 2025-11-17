// src/operations/engine/orchestration.rs
// Main operation orchestration: run_operation method (DeepSeek-only)

use crate::llm::provider::Message;
use crate::memory::service::MemoryService;
use crate::operations::ContextLoader;
use crate::operations::delegation_tools::get_deepseek_tools;
use crate::operations::engine::{
    artifacts::ArtifactManager, context::ContextBuilder,
    deepseek_orchestrator::DeepSeekOrchestrator, events::OperationEngineEvent,
    lifecycle::LifecycleManager, tool_router::ToolRouter,
};

use anyhow::{Context, Result};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

pub struct Orchestrator {
    deepseek_orchestrator: Option<Arc<DeepSeekOrchestrator>>,
    memory_service: Arc<MemoryService>,
    context_builder: ContextBuilder,
    context_loader: ContextLoader,
    tool_router: Option<Arc<ToolRouter>>,
    artifact_manager: ArtifactManager,
    lifecycle_manager: LifecycleManager,
}

impl Orchestrator {
    pub fn new(
        deepseek_orchestrator: Option<Arc<DeepSeekOrchestrator>>,
        memory_service: Arc<MemoryService>,
        context_builder: ContextBuilder,
        context_loader: ContextLoader,
        tool_router: Option<Arc<ToolRouter>>,
        artifact_manager: ArtifactManager,
        lifecycle_manager: LifecycleManager,
    ) -> Self {
        Self {
            deepseek_orchestrator,
            memory_service,
            context_builder,
            context_loader,
            tool_router,
            artifact_manager,
            lifecycle_manager,
        }
    }

    /// Main operation orchestration with error handling wrapper
    ///
    /// This wrapper ensures that ANY error (cancellation, API failures, etc.)
    /// properly emits a Failed event before propagating the error.
    pub async fn run_operation(
        &self,
        operation_id: &str,
        session_id: &str,
        user_content: &str,
        project_id: Option<&str>,
        cancel_token: Option<CancellationToken>,
        event_tx: &mpsc::Sender<OperationEngineEvent>,
    ) -> Result<()> {
        // Run the inner operation logic
        let result = self
            .run_operation_inner(
                operation_id,
                session_id,
                user_content,
                project_id,
                cancel_token,
                event_tx,
            )
            .await;

        // If ANY error occurred, emit Failed event
        if let Err(e) = &result {
            let error_msg = e.to_string();
            warn!("[ENGINE] Operation {} failed: {}", operation_id, error_msg);

            // Emit failed event (ignore errors from this since we're already failing)
            let _ = self
                .lifecycle_manager
                .fail_operation(operation_id, error_msg, event_tx)
                .await;
        }

        result
    }

    /// Internal operation orchestration logic
    async fn run_operation_inner(
        &self,
        operation_id: &str,
        session_id: &str,
        user_content: &str,
        project_id: Option<&str>,
        cancel_token: Option<CancellationToken>,
        event_tx: &mpsc::Sender<OperationEngineEvent>,
    ) -> Result<()> {
        if let Some(token) = &cancel_token {
            if token.is_cancelled() {
                return Err(anyhow::anyhow!("Operation cancelled before start"));
            }
        }

        // Store user message in memory
        let user_msg_id = self
            .memory_service
            .save_user_message(session_id, user_content, project_id)
            .await?;

        info!("Stored user message in memory: message_id={}", user_msg_id);

        // Load memory context
        let recall_context = self
            .context_builder
            .load_memory_context(session_id, user_content)
            .await?;

        // Load project context (file tree + code intelligence) using shared loader
        let (file_tree, code_context) = self
            .context_loader
            .load_project_context(user_content, project_id, 10)
            .await;

        // Build system prompt with full context
        let system_prompt = self
            .context_builder
            .build_system_prompt(
                session_id,
                &recall_context,
                code_context.as_ref(),
                file_tree.as_ref(),
            )
            .await;

        self.lifecycle_manager
            .start_operation(operation_id, event_tx)
            .await?;

        // Always use DeepSeek orchestration (DeepSeek-only architecture)
        info!("[ENGINE] Using DeepSeek orchestration");
        self.execute_with_deepseek(
            operation_id,
            session_id,
            user_content,
            system_prompt,
            event_tx,
        )
        .await
    }
    async fn execute_with_deepseek(
        &self,
        operation_id: &str,
        session_id: &str,
        user_content: &str,
        system_prompt: String,
        event_tx: &mpsc::Sender<OperationEngineEvent>,
    ) -> Result<()> {
        let deepseek = match &self.deepseek_orchestrator {
            Some(orch) => orch,
            None => return Err(anyhow::anyhow!("DeepSeek orchestrator not initialized")),
        };

        // Build messages with system prompt
        let messages = vec![
            Message::system(system_prompt),
            Message::user(user_content.to_string()),
        ];

        // Build tools for DeepSeek (excludes GPT-5 meta-tools)
        let tools = get_deepseek_tools();

        // Execute with DeepSeek orchestrator
        let response = deepseek
            .execute(operation_id, messages, tools, event_tx)
            .await
            .context("DeepSeek orchestration failed")?;

        // Complete operation
        self.lifecycle_manager
            .complete_operation(
                operation_id,
                session_id,
                Some(response),
                event_tx,
                vec![], // Artifacts are handled by DeepSeek orchestrator
            )
            .await?;

        Ok(())
    }
}
