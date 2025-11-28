// src/operations/engine/orchestration.rs
// Main operation orchestration: run_operation method (GPT 5.1)

use crate::llm::provider::Message;
use crate::memory::service::MemoryService;
use crate::operations::ContextLoader;
use crate::operations::delegation_tools::get_delegation_tools;
use crate::operations::engine::{
    context::ContextBuilder, gpt5_orchestrator::Gpt5Orchestrator,
    events::OperationEngineEvent, lifecycle::LifecycleManager,
};

use anyhow::{Context, Result};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

pub struct Orchestrator {
    gpt5_orchestrator: Option<Arc<Gpt5Orchestrator>>,
    memory_service: Arc<MemoryService>,
    context_builder: ContextBuilder,
    context_loader: ContextLoader,
    lifecycle_manager: LifecycleManager,
}

impl Orchestrator {
    pub fn new(
        gpt5_orchestrator: Option<Arc<Gpt5Orchestrator>>,
        memory_service: Arc<MemoryService>,
        context_builder: ContextBuilder,
        context_loader: ContextLoader,
        lifecycle_manager: LifecycleManager,
    ) -> Self {
        Self {
            gpt5_orchestrator,
            memory_service,
            context_builder,
            context_loader,
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

        // Gather Context Oracle intelligence (co-change patterns, expertise, fixes, etc.)
        let oracle_context = self
            .context_builder
            .gather_oracle_context(
                user_content,
                session_id,
                project_id,
                None, // current_file - could be extracted from user_content in future
                None, // error_message - could be extracted from user_content in future
            )
            .await;

        // Build system prompt with full context
        let mut system_prompt = self
            .context_builder
            .build_system_prompt(
                session_id,
                &recall_context,
                code_context.as_ref(),
                file_tree.as_ref(),
            )
            .await;

        // Append Context Oracle intelligence to system prompt if available
        if let Some(oracle) = &oracle_context {
            let oracle_output = oracle.format_for_prompt();
            if !oracle_output.is_empty() {
                system_prompt.push_str("\n\n=== CODEBASE INTELLIGENCE ===\n");
                system_prompt.push_str(&oracle_output);
                info!(
                    "[ENGINE] Added oracle context: {} sources, ~{} tokens",
                    oracle.sources_used.len(),
                    oracle.estimated_tokens
                );
            }
        }

        // Append active project tasks to system prompt if available
        if let Some(task_context) = self.context_builder.load_task_context(project_id).await {
            system_prompt.push_str("\n\n=== ACTIVE TASKS ===\n");
            system_prompt.push_str(&task_context);
            info!("[ENGINE] Added task context to system prompt");
        }

        self.lifecycle_manager
            .start_operation(operation_id, event_tx)
            .await?;

        // Use GPT 5.1 orchestration
        info!("[ENGINE] Using GPT 5.1 orchestration");
        self.execute_with_gpt5(
            operation_id,
            session_id,
            user_content,
            system_prompt,
            project_id,
            event_tx,
        )
        .await
    }

    async fn execute_with_gpt5(
        &self,
        operation_id: &str,
        session_id: &str,
        user_content: &str,
        system_prompt: String,
        project_id: Option<&str>,
        event_tx: &mpsc::Sender<OperationEngineEvent>,
    ) -> Result<()> {
        let gpt5 = match &self.gpt5_orchestrator {
            Some(orch) => orch,
            None => return Err(anyhow::anyhow!("GPT 5.1 orchestrator not initialized")),
        };

        // Build messages with system prompt
        let messages = vec![
            Message::system(system_prompt),
            Message::user(user_content.to_string()),
        ];

        // Build tools for GPT 5.1
        let tools = get_delegation_tools();

        // Execute with GPT 5.1 orchestrator
        // Use session_id as user_id for budget tracking (they map 1:1 in current design)
        let response = gpt5
            .execute_with_context(session_id, operation_id, messages, tools, project_id, session_id, event_tx)
            .await
            .context("GPT 5.1 orchestration failed")?;

        // Complete operation
        self.lifecycle_manager
            .complete_operation(
                operation_id,
                session_id,
                Some(response),
                event_tx,
                vec![], // Artifacts are handled by orchestrator
            )
            .await?;

        Ok(())
    }
}
