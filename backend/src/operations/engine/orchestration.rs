// src/operations/engine/orchestration.rs
// Main operation orchestration: run_operation method (Gemini 3 Pro)

use crate::api::ws::message::SystemAccessMode;
use crate::llm::provider::Message;
use crate::memory::service::MemoryService;
use crate::operations::ContextLoader;
use crate::operations::delegation_tools::get_delegation_tools;
use crate::operations::engine::{
    context::ContextBuilder, llm_orchestrator::LlmOrchestrator,
    events::OperationEngineEvent, lifecycle::LifecycleManager,
};

use anyhow::{Context, Result};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

pub struct Orchestrator {
    llm_orchestrator: Option<Arc<LlmOrchestrator>>,
    memory_service: Arc<MemoryService>,
    context_builder: ContextBuilder,
    context_loader: ContextLoader,
    lifecycle_manager: LifecycleManager,
}

impl Orchestrator {
    pub fn new(
        llm_orchestrator: Option<Arc<LlmOrchestrator>>,
        memory_service: Arc<MemoryService>,
        context_builder: ContextBuilder,
        context_loader: ContextLoader,
        lifecycle_manager: LifecycleManager,
    ) -> Self {
        Self {
            llm_orchestrator,
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
        system_access_mode: SystemAccessMode,
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
                system_access_mode,
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
        system_access_mode: SystemAccessMode,
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

        // Load memory context with project boosting
        let recall_context = self
            .context_builder
            .load_memory_context(session_id, user_content, project_id)
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
                project_id,
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

        // Use LLM orchestration
        info!("[ENGINE] Using LLM orchestration with access_mode={:?}", system_access_mode);
        self.execute_with_llm(
            operation_id,
            session_id,
            user_content,
            system_prompt,
            project_id,
            system_access_mode,
            &recall_context,
            event_tx,
        )
        .await
    }

    async fn execute_with_llm(
        &self,
        operation_id: &str,
        session_id: &str,
        _user_content: &str, // Now included in recall_context.recent
        system_prompt: String,
        project_id: Option<&str>,
        system_access_mode: SystemAccessMode,
        recall_context: &crate::memory::features::recall_engine::RecallContext,
        event_tx: &mpsc::Sender<OperationEngineEvent>,
    ) -> Result<()> {
        let llm = match &self.llm_orchestrator {
            Some(orch) => orch,
            None => return Err(anyhow::anyhow!("LLM orchestrator not initialized")),
        };

        // Build messages with system prompt and conversation history
        // Limit to configured amount - rolling summaries handle older context
        let history_limit = crate::config::CONFIG.llm_message_history_limit;
        let mut messages = vec![Message::system(system_prompt)];

        // Take only the most recent N messages from recall_context.recent
        // The current user message was already saved and is included in recent,
        // so we don't need to add it separately
        let recent_slice = if recall_context.recent.len() > history_limit {
            &recall_context.recent[recall_context.recent.len() - history_limit..]
        } else {
            &recall_context.recent[..]
        };

        for entry in recent_slice {
            match entry.role.as_str() {
                "user" => messages.push(Message::user(entry.content.clone())),
                "assistant" => messages.push(Message::assistant(entry.content.clone())),
                _ => {} // Skip system messages or unknown roles
            }
        }

        info!(
            "[ENGINE] Built {} messages for LLM (1 system + {} of {} history, limit={})",
            messages.len(),
            recent_slice.len(),
            recall_context.recent.len(),
            history_limit
        );

        // Build tools for Gemini 3
        let tools = get_delegation_tools();

        // Execute with LLM orchestrator
        // Use session_id as user_id for budget tracking (they map 1:1 in current design)
        let response = llm
            .execute_with_context(
                session_id,
                operation_id,
                messages,
                tools,
                project_id,
                system_access_mode,
                session_id,
                event_tx,
            )
            .await
            .context("LLM orchestration failed")?;

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
