// src/operations/engine/orchestration.rs
// Main operation orchestration: run_operation method
// FIXED: Added error handling wrapper to emit Failed events on any error

use crate::llm::provider::Message;
use crate::llm::provider::gpt5::{Gpt5Provider, Gpt5StreamEvent};
use crate::llm::structured::tool_schema::get_create_artifact_tool_schema;
use crate::operations::delegation_tools::{get_delegation_tools, parse_tool_call};
use crate::operations::engine::{
    events::OperationEngineEvent,
    context::ContextBuilder,
    delegation::DelegationHandler,
    artifacts::ArtifactManager,
    lifecycle::LifecycleManager,
};
use crate::memory::core::types::MemoryEntry;
use crate::memory::service::MemoryService;
use crate::git::client::{GitClient, FileNode};
use crate::git::client::project_ops::ProjectOps;

use anyhow::{Context, Result};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};
use futures::StreamExt;

pub struct Orchestrator {
    gpt5: Gpt5Provider,
    memory_service: Arc<MemoryService>,
    git_client: GitClient,
    code_intelligence: Arc<crate::memory::features::code_intelligence::CodeIntelligenceService>,
    context_builder: ContextBuilder,
    delegation_handler: DelegationHandler,
    artifact_manager: ArtifactManager,
    lifecycle_manager: LifecycleManager,
}

impl Orchestrator {
    pub fn new(
        gpt5: Gpt5Provider,
        memory_service: Arc<MemoryService>,
        git_client: GitClient,
        code_intelligence: Arc<crate::memory::features::code_intelligence::CodeIntelligenceService>,
        context_builder: ContextBuilder,
        delegation_handler: DelegationHandler,
        artifact_manager: ArtifactManager,
        lifecycle_manager: LifecycleManager,
    ) -> Self {
        Self {
            gpt5,
            memory_service,
            git_client,
            code_intelligence,
            context_builder,
            delegation_handler,
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
        let result = self.run_operation_inner(
            operation_id,
            session_id,
            user_content,
            project_id,
            cancel_token,
            event_tx,
        ).await;

        // If ANY error occurred, emit Failed event
        if let Err(e) = &result {
            let error_msg = e.to_string();
            warn!("[ENGINE] Operation {} failed: {}", operation_id, error_msg);
            
            // Emit failed event (ignore errors from this since we're already failing)
            let _ = self.lifecycle_manager.fail_operation(
                operation_id,
                error_msg,
                event_tx,
            ).await;
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
        let user_msg_id = self.memory_service.save_user_message(
            session_id,
            user_content,
            project_id,
        ).await?;
        
        info!("Stored user message in memory: message_id={}", user_msg_id);

        // Load memory context
        let recall_context = self.context_builder.load_memory_context(session_id, user_content).await?;
        
        // Load file tree if project is selected
        let file_tree = self.load_file_tree(project_id).await;
        
        // Load code intelligence context (semantic search on code)
        let code_context = self.load_code_context(user_content, project_id).await;
        
        // Build system prompt with full context
        let system_prompt = self.context_builder.build_system_prompt(
            session_id, 
            &recall_context,
            code_context.as_ref(),
            file_tree.as_ref(),
        ).await;
        
        let messages = vec![Message::user(user_content.to_string())];

        self.lifecycle_manager.start_operation(operation_id, event_tx).await?;

        // Build tools: delegation tools + create_artifact
        let mut tools = get_delegation_tools();
        tools.push(get_create_artifact_tool_schema());
        
        let tool_names: Vec<String> = tools.iter()
            .filter_map(|t| t.get("function")
                .and_then(|f| f.get("name"))
                .and_then(|n| n.as_str())
                .map(|s| s.to_string()))
            .collect();
        info!("[ENGINE] Passing {} tools to GPT-5: {:?}", tools.len(), tool_names);
        
        let previous_response_id = None;

        if let Some(token) = &cancel_token {
            if token.is_cancelled() {
                return Err(anyhow::anyhow!("Operation cancelled"));
            }
        }

        // Stream GPT-5 responses and handle tool calls
        let mut stream = self.gpt5
            .create_stream_with_tools(messages, system_prompt, tools, previous_response_id)
            .await
            .context("Failed to create GPT-5 stream")?;

        let mut accumulated_text = String::new();
        let mut delegation_calls = Vec::new();
        let mut _final_response_id = None;

        // Process tool calls during streaming
        while let Some(event) = stream.next().await {
            if let Some(token) = &cancel_token {
                if token.is_cancelled() {
                    return Err(anyhow::anyhow!("Operation cancelled"));
                }
            }

            match event? {
                Gpt5StreamEvent::TextDelta { delta } => {
                    accumulated_text.push_str(&delta);
                    let _ = event_tx.send(OperationEngineEvent::Streaming {
                        operation_id: operation_id.to_string(),
                        content: delta,
                    }).await;
                }
                Gpt5StreamEvent::ToolCallComplete { id, name, arguments } => {
                    info!("[ENGINE] GPT-5 tool call: {} (id: {})", name, id);
                    
                    // Handle create_artifact immediately
                    if name == "create_artifact" {
                        info!("[ENGINE] GPT-5 called create_artifact, creating immediately");
                        if let Err(e) = self.artifact_manager.create_artifact(operation_id, arguments.clone(), event_tx).await {
                            warn!("[ENGINE] Failed to create artifact: {}", e);
                        }
                    } else {
                        // Queue other tools for DeepSeek delegation
                        info!("[ENGINE] Queueing {} for DeepSeek delegation", name);
                        delegation_calls.push((id, name, arguments));
                    }
                }
                Gpt5StreamEvent::Done { response_id, .. } => {
                    _final_response_id = Some(response_id);
                }
                _ => {}
            }
        }

        // Handle delegation calls (everything except create_artifact)
        if !delegation_calls.is_empty() {
            if let Some(token) = &cancel_token {
                if token.is_cancelled() {
                    return Err(anyhow::anyhow!("Operation cancelled"));
                }
            }

            self.lifecycle_manager.update_status(operation_id, "delegating", event_tx).await?;

            for (_tool_id, tool_name, tool_args) in delegation_calls {
                if let Some(token) = &cancel_token {
                    if token.is_cancelled() {
                        return Err(anyhow::anyhow!("Operation cancelled"));
                    }
                }

                let tool_call_json = serde_json::json!({
                    "function": {
                        "name": tool_name,
                        "arguments": serde_json::to_string(&tool_args)?,
                    }
                });

                let (parsed_name, parsed_args) = parse_tool_call(&tool_call_json)?;

                let _ = event_tx.send(OperationEngineEvent::Delegated {
                    operation_id: operation_id.to_string(),
                    delegated_to: "deepseek".to_string(),
                    reason: format!("Tool call: {}", parsed_name),
                }).await;

                // Delegate to DeepSeek with rich context
                let result = self.delegation_handler.delegate_to_deepseek(
                    &parsed_name,
                    parsed_args,
                    cancel_token.clone(),
                    file_tree.as_ref(),
                    code_context.as_ref(),
                    &recall_context,
                ).await?;

                // DeepSeek can also return artifacts
                if let Some(artifact_data) = result.get("artifact") {
                    self.artifact_manager.create_artifact(operation_id, artifact_data.clone(), event_tx).await?;
                }
            }

            self.lifecycle_manager.update_status(operation_id, "generating", event_tx).await?;
        }

        // Complete the operation
        let artifacts = self.artifact_manager.get_artifacts_for_operation(operation_id).await?;
        self.lifecycle_manager.complete_operation(
            operation_id, 
            session_id, 
            Some(accumulated_text), 
            event_tx,
            artifacts,
        ).await?;

        Ok(())
    }

    /// Load file tree for project
    async fn load_file_tree(&self, project_id: Option<&str>) -> Option<Vec<FileNode>> {
        let pid = project_id?;
        debug!("Loading file tree for project {}", pid);
        match self.git_client.get_project_tree(pid).await {
            Ok(tree) => {
                debug!("Loaded file tree with {} items", tree.len());
                Some(tree)
            }
            Err(e) => {
                warn!("Failed to load file tree: {}", e);
                None
            }
        }
    }

    /// Load code intelligence context
    async fn load_code_context(&self, user_content: &str, project_id: Option<&str>) -> Option<Vec<MemoryEntry>> {
        let pid = project_id?;
        debug!("Loading code intelligence context for operation");
        match self.code_intelligence.search_code(user_content, pid, 10).await {
            Ok(entries) => {
                if !entries.is_empty() {
                    debug!("Loaded {} code intelligence entries", entries.len());
                    Some(entries)
                } else {
                    None
                }
            }
            Err(e) => {
                warn!("Failed to load code context: {}", e);
                None
            }
        }
    }
}
