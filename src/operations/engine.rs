// src/operations/engine.rs
// Operation Engine - orchestrates coding workflows with GPT-5 + DeepSeek delegation
// FIXED: create_artifact tool calls are now handled immediately during streaming
// UPDATED: Now loads file tree and code intelligence context for operations

use crate::config::CONFIG;
use crate::llm::provider::Message;
use crate::llm::provider::gpt5::{Gpt5Provider, Gpt5StreamEvent};
use crate::llm::provider::deepseek::DeepSeekProvider;
use crate::llm::structured::tool_schema::get_create_artifact_tool_schema;
use crate::memory::service::MemoryService;
use crate::operations::{Operation, OperationEvent, Artifact};
use crate::operations::delegation_tools::{get_delegation_tools, parse_tool_call};
use crate::persona::PersonaOverlay;
use crate::tools::types::Tool;
use crate::prompt::UnifiedPromptBuilder;
use crate::memory::features::recall_engine::RecallContext;
use crate::memory::core::types::MemoryEntry;
use crate::relationship::service::RelationshipService;
use crate::git::client::{GitClient, FileNode};
use crate::git::client::project_ops::ProjectOps;

use anyhow::{Context, Result};
use sqlx::SqlitePool;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};
use serde_json;
use futures::StreamExt;
use sha2::{Sha256, Digest};

/// Events emitted by the operation engine
#[derive(Debug, Clone)]
pub enum OperationEngineEvent {
    Started {
        operation_id: String,
    },
    StatusChanged {
        operation_id: String,
        old_status: String,
        new_status: String,
    },
    Streaming {
        operation_id: String,
        content: String,
    },
    Delegated {
        operation_id: String,
        delegated_to: String,
        reason: String,
    },
    ArtifactPreview {
        operation_id: String,
        artifact_id: String,
        path: String,
        preview: String,
    },
    ArtifactCompleted {
        operation_id: String,
        artifact: Artifact,
    },
    Completed {
        operation_id: String,
        result: Option<String>,
        artifacts: Vec<Artifact>,
    },
    Failed {
        operation_id: String,
        error: String,
    },
}

pub struct OperationEngine {
    db: Arc<SqlitePool>,
    gpt5: Gpt5Provider,
    deepseek: DeepSeekProvider,
    memory_service: Arc<MemoryService>,
    relationship_service: Arc<RelationshipService>,
    git_client: GitClient,  // NEW: For loading file trees
    code_intelligence: Arc<crate::memory::features::code_intelligence::CodeIntelligenceService>,  // NEW: For code search
}

impl OperationEngine {
    pub fn new(
        db: Arc<SqlitePool>,
        gpt5: Gpt5Provider,
        deepseek: DeepSeekProvider,
        memory_service: Arc<MemoryService>,
        relationship_service: Arc<RelationshipService>,
        git_client: GitClient,  // NEW: Pass in git client
        code_intelligence: Arc<crate::memory::features::code_intelligence::CodeIntelligenceService>,  // NEW: Pass in code intelligence
    ) -> Self {
        Self { 
            db, 
            gpt5, 
            deepseek,
            memory_service,
            relationship_service,
            git_client,  // NEW: Store it
            code_intelligence,  // NEW: Store it
        }
    }

    pub async fn create_operation(
        &self,
        session_id: String,
        kind: String,
        user_message: String,
    ) -> Result<Operation> {
        let op = Operation::new(session_id, kind, user_message);
        
        sqlx::query!(
            r#"
            INSERT INTO operations (
                id, session_id, kind, status, created_at, user_message,
                delegate_calls
            ) VALUES (?, ?, ?, ?, ?, ?, ?)
            "#,
            op.id,
            op.session_id,
            op.kind,
            op.status,
            op.created_at,
            op.user_message,
            op.delegate_calls,
        )
        .execute(&*self.db)
        .await
        .context("Failed to create operation")?;

        Ok(op)
    }

    pub async fn start_operation(
        &self,
        operation_id: &str,
        event_tx: &mpsc::Sender<OperationEngineEvent>,
    ) -> Result<()> {
        let started_at = chrono::Utc::now().timestamp();
        let old_status = "pending";
        let new_status = "planning";

        sqlx::query!(
            r#"
            UPDATE operations
            SET started_at = ?, status = ?
            WHERE id = ?
            "#,
            started_at,
            new_status,
            operation_id,
        )
        .execute(&*self.db)
        .await
        .context("Failed to start operation")?;

        self.emit_event(
            operation_id,
            "status_change",
            Some(serde_json::json!({
                "old_status": old_status,
                "new_status": new_status,
            })),
        )
        .await?;

        let _ = event_tx
            .send(OperationEngineEvent::Started {
                operation_id: operation_id.to_string(),
            })
            .await;

        let _ = event_tx
            .send(OperationEngineEvent::StatusChanged {
                operation_id: operation_id.to_string(),
                old_status: old_status.to_string(),
                new_status: new_status.to_string(),
            })
            .await;

        Ok(())
    }

    pub async fn complete_operation(
        &self,
        operation_id: &str,
        session_id: &str,
        result: Option<String>,
        event_tx: &mpsc::Sender<OperationEngineEvent>,
    ) -> Result<()> {
        let completed_at = chrono::Utc::now().timestamp();
        let old_status = self.get_operation_status(operation_id).await?;
        let new_status = "completed";

        sqlx::query!(
            r#"
            UPDATE operations
            SET completed_at = ?, status = ?, result = ?
            WHERE id = ?
            "#,
            completed_at,
            new_status,
            result,
            operation_id,
        )
        .execute(&*self.db)
        .await
        .context("Failed to complete operation")?;

        // Store assistant response in memory
        if let Some(ref response_content) = result {
            match self.memory_service.save_assistant_message(
                session_id,
                response_content,
                None,
            ).await {
                Ok(msg_id) => {
                    info!("Stored operation result in memory: message_id={}", msg_id);
                    
                    // Note: Embeddings are handled by MessagePipelineCoordinator automatically
                }
                Err(e) => {
                    warn!("Failed to store operation result in memory: {}", e);
                }
            }
        }

        self.emit_event(
            operation_id,
            "completed",
            Some(serde_json::json!({
                "old_status": old_status,
                "new_status": new_status,
                "result": result,
            })),
        )
        .await?;

        let _ = event_tx
            .send(OperationEngineEvent::StatusChanged {
                operation_id: operation_id.to_string(),
                old_status,
                new_status: new_status.to_string(),
            })
            .await;

        // Retrieve artifacts and include them in the Completed event
        let artifacts = self.get_artifacts_for_operation(operation_id).await?;

        let _ = event_tx
            .send(OperationEngineEvent::Completed {
                operation_id: operation_id.to_string(),
                result,
                artifacts,
            })
            .await;

        Ok(())
    }

    pub async fn fail_operation(
        &self,
        operation_id: &str,
        error: String,
        event_tx: &mpsc::Sender<OperationEngineEvent>,
    ) -> Result<()> {
        let completed_at = chrono::Utc::now().timestamp();
        let old_status = self.get_operation_status(operation_id).await?;
        let new_status = "failed";

        sqlx::query!(
            r#"
            UPDATE operations
            SET completed_at = ?, status = ?, error = ?
            WHERE id = ?
            "#,
            completed_at,
            new_status,
            error,
            operation_id,
        )
        .execute(&*self.db)
        .await
        .context("Failed to mark operation as failed")?;

        self.emit_event(
            operation_id,
            "error",
            Some(serde_json::json!({
                "error": error,
                "old_status": old_status,
                "new_status": new_status,
            })),
        )
        .await?;

        let _ = event_tx
            .send(OperationEngineEvent::StatusChanged {
                operation_id: operation_id.to_string(),
                old_status,
                new_status: new_status.to_string(),
            })
            .await;

        let _ = event_tx
            .send(OperationEngineEvent::Failed {
                operation_id: operation_id.to_string(),
                error,
            })
            .await;

        Ok(())
    }

    pub async fn run_operation(
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

        let user_msg_id = self.memory_service.save_user_message(
            session_id,
            user_content,
            project_id,
        ).await?;
        
        info!("Stored user message in memory: message_id={}", user_msg_id);

        // Note: Embeddings are handled by MessagePipelineCoordinator automatically

        // Load memory context
        let recall_context = self.load_memory_context(session_id, user_content).await?;
        
        // NEW: Load file tree if project is selected
        let file_tree = if let Some(pid) = project_id {
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
        } else {
            None
        };
        
        // NEW: Load code intelligence context (semantic search on code)
        let code_context = if let Some(pid) = project_id {
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
        } else {
            None
        };
        
        // Build system prompt with full context
        let system_prompt = self.build_system_prompt_with_context(
            session_id, 
            &recall_context,
            code_context.as_ref(),
            file_tree.as_ref(),
        ).await;
        
        let messages = vec![Message::user(user_content.to_string())];

        self.start_operation(operation_id, event_tx).await?;

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
                let _ = self.fail_operation(operation_id, "Operation cancelled".to_string(), event_tx).await;
                return Err(anyhow::anyhow!("Operation cancelled"));
            }
        }

        let mut stream = self.gpt5
            .create_stream_with_tools(messages, system_prompt, tools, previous_response_id)
            .await
            .context("Failed to create GPT-5 stream")?;

        let mut accumulated_text = String::new();
        let mut delegation_calls = Vec::new();
        let mut _final_response_id = None;

        // FIX: Process tool calls during streaming
        while let Some(event) = stream.next().await {
            if let Some(token) = &cancel_token {
                if token.is_cancelled() {
                    let _ = self.fail_operation(operation_id, "Operation cancelled during streaming".to_string(), event_tx).await;
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
                    
                    // FIX: Handle create_artifact immediately
                    if name == "create_artifact" {
                        info!("[ENGINE] GPT-5 called create_artifact, creating immediately");
                        if let Err(e) = self.create_artifact(operation_id, arguments.clone(), event_tx).await {
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
                    let _ = self.fail_operation(operation_id, "Operation cancelled before delegation".to_string(), event_tx).await;
                    return Err(anyhow::anyhow!("Operation cancelled"));
                }
            }

            self.update_operation_status(operation_id, "delegating", event_tx).await?;

            for (_tool_id, tool_name, tool_args) in delegation_calls {
                if let Some(token) = &cancel_token {
                    if token.is_cancelled() {
                        let _ = self.fail_operation(operation_id, "Operation cancelled during delegation".to_string(), event_tx).await;
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

                let result = self.delegate_to_deepseek(
                    operation_id,
                    &parsed_name,
                    parsed_args,
                    event_tx,
                    cancel_token.clone(),
                ).await?;

                // DeepSeek can also return artifacts
                if let Some(artifact_data) = result.get("artifact") {
                    self.create_artifact(operation_id, artifact_data.clone(), event_tx).await?;
                }
            }

            self.update_operation_status(operation_id, "generating", event_tx).await?;
        }

        self.complete_operation(operation_id, session_id, Some(accumulated_text), event_tx).await?;

        Ok(())
    }

    async fn load_memory_context(
        &self,
        session_id: &str,
        query: &str,
    ) -> Result<RecallContext> {
        debug!("Loading memory context for session: {}", session_id);
        
        let recent_count = CONFIG.context_recent_messages as usize;
        let semantic_count = CONFIG.context_semantic_matches as usize;
        
        match self.memory_service.parallel_recall_context(
            session_id,
            query,
            recent_count,
            semantic_count,
        ).await {
            Ok(mut context) => {
                info!(
                    "Loaded context: {} recent, {} semantic memories",
                    context.recent.len(),
                    context.semantic.len()
                );
                
                if CONFIG.use_rolling_summaries_in_context {
                    context.rolling_summary = self.memory_service
                        .get_rolling_summary(session_id)
                        .await
                        .ok()
                        .flatten();
                    
                    context.session_summary = self.memory_service
                        .get_session_summary(session_id)
                        .await
                        .ok()
                        .flatten();
                    
                    if context.rolling_summary.is_some() || context.session_summary.is_some() {
                        info!("Loaded summaries for context");
                    }
                }
                
                Ok(context)
            }
            Err(e) => {
                warn!("Failed to load memory context: {}, using empty context", e);
                Ok(RecallContext {
                    recent: vec![],
                    semantic: vec![],
                    rolling_summary: None,
                    session_summary: None,
                })
            }
        }
    }

    async fn build_system_prompt_with_context(
        &self,
        session_id: &str,
        context: &RecallContext,
        code_context: Option<&Vec<MemoryEntry>>,  // NEW: Code intelligence context
        file_tree: Option<&Vec<FileNode>>,         // NEW: Repository structure
    ) -> String {
        let persona = PersonaOverlay::Default;
        let tools_json = get_delegation_tools();
        
        let tools: Vec<Tool> = tools_json.iter()
            .filter_map(|v| serde_json::from_value::<Tool>(v.clone()).ok())
            .collect();
        
        let _relationship_ctx = self.relationship_service
            .context_loader()
            .load_context(session_id)
            .await
            .ok();
        
        // NEW: Pass code context and file tree to builder
        UnifiedPromptBuilder::build_system_prompt(
            &persona,
            context,
            Some(&tools),
            None,  // metadata
            None,  // project_id
            code_context.map(|v| &**v),  // NEW: Pass code intelligence
            file_tree.map(|v| &**v),      // NEW: Pass file tree
        )
    }

    async fn delegate_to_deepseek(
        &self,
        _operation_id: &str,
        tool_name: &str,
        args: serde_json::Value,
        _event_tx: &mpsc::Sender<OperationEngineEvent>,
        cancel_token: Option<CancellationToken>,
    ) -> Result<serde_json::Value> {
        if let Some(token) = &cancel_token {
            if token.is_cancelled() {
                return Err(anyhow::anyhow!("Operation cancelled before DeepSeek delegation"));
            }
        }

        info!("Delegating {} to DeepSeek", tool_name);

        // Build CodeGenRequest based on tool type
        let request = match tool_name {
            "generate_code" => {
                let path = args.get("path")
                    .and_then(|v| v.as_str())
                    .unwrap_or("untitled.rs")
                    .to_string();
                let description = args.get("description")
                    .and_then(|v| v.as_str())
                    .or_else(|| args.get("task").and_then(|v| v.as_str()))
                    .unwrap_or("Generate code")
                    .to_string();

                crate::llm::provider::deepseek::CodeGenRequest {
                    path,
                    description,
                    language: args.get("language").and_then(|v| v.as_str()).unwrap_or("rust").to_string(),
                    framework: args.get("framework").and_then(|v| v.as_str()).map(String::from),
                    dependencies: vec![],
                    style_guide: None,
                    context: args.get("context")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                }
            }
            "modify_code" | "refactor_code" => {
                let path = args.get("path")
                    .and_then(|v| v.as_str())
                    .unwrap_or("untitled.rs")
                    .to_string();
                let instructions = args.get("instructions")
                    .and_then(|v| v.as_str())
                    .or_else(|| args.get("refactoring_goals").and_then(|v| v.as_str()))
                    .unwrap_or("Modify code")
                    .to_string();
                let existing = args.get("existing_code")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                let description = if existing.is_empty() {
                    instructions
                } else {
                    format!("{}\n\nExisting code:\n{}", instructions, existing)
                };

                crate::llm::provider::deepseek::CodeGenRequest {
                    path,
                    description,
                    language: args.get("language").and_then(|v| v.as_str()).unwrap_or("rust").to_string(),
                    framework: args.get("framework").and_then(|v| v.as_str()).map(String::from),
                    dependencies: vec![],
                    style_guide: None,
                    context: args.get("context")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                }
            }
            "fix_code" => {
                let path = args.get("path")
                    .and_then(|v| v.as_str())
                    .unwrap_or("untitled.rs")
                    .to_string();
                let error_msg = args.get("error_message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Fix error");
                let code = args.get("code")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                let description = format!("Fix the following error:\n{}\n\nExisting code:\n{}", error_msg, code);

                crate::llm::provider::deepseek::CodeGenRequest {
                    path,
                    description,
                    language: args.get("language").and_then(|v| v.as_str()).unwrap_or("rust").to_string(),
                    framework: args.get("framework").and_then(|v| v.as_str()).map(String::from),
                    dependencies: vec![],
                    style_guide: None,
                    context: String::new(),
                }
            }
            _ => {
                return Err(anyhow::anyhow!("Unknown tool: {}", tool_name));
            }
        };

        if let Some(token) = &cancel_token {
            if token.is_cancelled() {
                return Err(anyhow::anyhow!("Operation cancelled during DeepSeek request"));
            }
        }

        let response = self.deepseek.generate_code(request).await?;

        Ok(serde_json::json!({
            "artifact": {
                "path": response.artifact.path,
                "content": response.artifact.content,
                "language": response.artifact.language,
                "explanation": response.artifact.explanation,
            }
        }))
    }

    async fn create_artifact(
        &self,
        operation_id: &str,
        args: serde_json::Value,
        event_tx: &mpsc::Sender<OperationEngineEvent>,
    ) -> Result<()> {
        let path = args.get("path")
            .and_then(|v| v.as_str())
            .context("Missing 'path' in create_artifact")?
            .to_string();

        let content = args.get("content")
            .and_then(|v| v.as_str())
            .context("Missing 'content' in create_artifact")?
            .to_string();

        let language = args.get("language")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let explanation = args.get("explanation")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        info!("[ENGINE] Creating artifact: {} ({} bytes)", path, content.len());

        // Generate content hash
        let mut hasher = Sha256::new();
        hasher.update(content.as_bytes());
        let hash = format!("{:x}", hasher.finalize());

        // Check if previous artifact exists
        let previous = sqlx::query!(
            "SELECT content FROM artifacts WHERE operation_id = ? AND file_path = ? ORDER BY created_at DESC LIMIT 1",
            operation_id,
            path
        )
        .fetch_optional(&*self.db)
        .await?;

        let diff = if let Some(prev) = previous {
            Some(Self::compute_diff(&prev.content, &content))
        } else {
            None
        };

        // FIXED: Use Artifact::new() constructor instead of direct struct initialization
        let artifact = Artifact::new(
            operation_id.to_string(),
            "code".to_string(),
            Some(path.clone()),
            content.clone(),
            hash,
            language,
            diff,
        );

        sqlx::query!(
            r#"
            INSERT INTO artifacts (
                id, operation_id, kind, file_path, content, content_hash,
                language, diff_from_previous, created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
            artifact.id,
            artifact.operation_id,
            artifact.kind,
            artifact.file_path,
            artifact.content,
            artifact.content_hash,
            artifact.language,
            artifact.diff,
            artifact.created_at,
        )
        .execute(&*self.db)
        .await
        .context("Failed to create artifact")?;

        let preview = if content.len() > 200 {
            format!("{}...", &content[..200])
        } else {
            content.to_string()
        };

        let _ = event_tx.send(OperationEngineEvent::ArtifactPreview {
            operation_id: operation_id.to_string(),
            artifact_id: artifact.id.clone(),
            path: path.to_string(),
            preview,
        }).await;

        let _ = event_tx.send(OperationEngineEvent::ArtifactCompleted {
            operation_id: operation_id.to_string(),
            artifact,
        }).await;

        if let Some(expl) = explanation {
            info!("[ENGINE] Artifact explanation: {}", expl);
        }

        Ok(())
    }

    /// Compute a simple unified diff between old and new content
    fn compute_diff(old_content: &str, new_content: &str) -> String {
        // Simple line-by-line diff
        let old_lines: Vec<&str> = old_content.lines().collect();
        let new_lines: Vec<&str> = new_content.lines().collect();
        
        let mut diff = String::new();
        diff.push_str(&format!("--- old\n+++ new\n"));
        
        // Simple implementation: just show what changed
        let max_lines = old_lines.len().max(new_lines.len());
        let mut changes = Vec::new();
        
        for i in 0..max_lines {
            let old_line = old_lines.get(i).copied();
            let new_line = new_lines.get(i).copied();
            
            match (old_line, new_line) {
                (Some(old), Some(new)) if old != new => {
                    changes.push(format!("-{}", old));
                    changes.push(format!("+{}", new));
                }
                (Some(old), None) => {
                    changes.push(format!("-{}", old));
                }
                (None, Some(new)) => {
                    changes.push(format!("+{}", new));
                }
                _ => {} // Lines are the same or both None
            }
        }
        
        if !changes.is_empty() {
            diff.push_str(&format!("@@ -{},{} +{},{} @@\n", 
                1, old_lines.len(), 
                1, new_lines.len()
            ));
            for change in changes {
                diff.push_str(&change);
                diff.push('\n');
            }
        }
        
        diff
    }

    async fn get_artifacts_for_operation(&self, operation_id: &str) -> Result<Vec<Artifact>> {
        let rows = sqlx::query!(
            r#"
            SELECT id, operation_id, kind, file_path, content, content_hash, language, diff_from_previous, created_at
            FROM artifacts
            WHERE operation_id = ?
            ORDER BY created_at
            "#,
            operation_id
        )
        .fetch_all(&*self.db)
        .await
        .context("Failed to fetch artifacts")?;

        // FIXED: Properly construct Artifact with all 24 fields
        let artifacts = rows.into_iter().map(|row| {
            let mut artifact = Artifact::new(
                row.operation_id,
                row.kind,
                row.file_path,
                row.content,
                row.content_hash.unwrap_or_default(),
                row.language,
                row.diff_from_previous,
            );
            // Override the auto-generated id and created_at with values from DB
            artifact.id = row.id.unwrap_or_default();
            artifact.created_at = row.created_at;
            artifact
        }).collect();

        Ok(artifacts)
    }

    async fn update_operation_status(
        &self,
        operation_id: &str,
        new_status: &str,
        event_tx: &mpsc::Sender<OperationEngineEvent>,
    ) -> Result<()> {
        let old_status = self.get_operation_status(operation_id).await?;

        sqlx::query!(
            "UPDATE operations SET status = ? WHERE id = ?",
            new_status,
            operation_id
        )
        .execute(&*self.db)
        .await?;

        self.emit_event(
            operation_id,
            "status_change",
            Some(serde_json::json!({
                "old_status": old_status,
                "new_status": new_status,
            })),
        )
        .await?;

        let _ = event_tx.send(OperationEngineEvent::StatusChanged {
            operation_id: operation_id.to_string(),
            old_status,
            new_status: new_status.to_string(),
        }).await;

        Ok(())
    }

    async fn emit_event(
        &self,
        operation_id: &str,
        event_type: &str,
        event_data: Option<serde_json::Value>,
    ) -> Result<()> {
        let sequence_number = self.get_next_sequence_number(operation_id).await?;
        let created_at = chrono::Utc::now().timestamp();

        let event_data_str = event_data.map(|v| v.to_string());

        sqlx::query!(
            r#"
            INSERT INTO operation_events (operation_id, event_type, created_at, sequence_number, event_data)
            VALUES (?, ?, ?, ?, ?)
            "#,
            operation_id,
            event_type,
            created_at,
            sequence_number,
            event_data_str,
        )
        .execute(&*self.db)
        .await
        .context("Failed to emit operation event")?;

        Ok(())
    }

    async fn get_next_sequence_number(&self, operation_id: &str) -> Result<i64> {
        let result = sqlx::query!(
            "SELECT COALESCE(MAX(sequence_number), -1) as max_seq FROM operation_events WHERE operation_id = ?",
            operation_id
        )
        .fetch_one(&*self.db)
        .await?;

        Ok((result.max_seq + 1) as i64)
    }

    async fn get_operation_status(&self, operation_id: &str) -> Result<String> {
        let result = sqlx::query!(
            "SELECT status FROM operations WHERE id = ?",
            operation_id
        )
        .fetch_one(&*self.db)
        .await?;

        Ok(result.status)
    }

    pub async fn get_operation(&self, operation_id: &str) -> Result<Operation> {
        let row = sqlx::query!(
            r#"
            SELECT id, session_id, kind, status, created_at, started_at, completed_at,
                   user_message, delegate_calls, result, error
            FROM operations
            WHERE id = ?
            "#,
            operation_id
        )
        .fetch_one(&*self.db)
        .await?;

        Ok(Operation {
            id: row.id.unwrap_or_else(|| operation_id.to_string()),
            session_id: row.session_id,
            kind: row.kind,
            status: row.status,
            created_at: row.created_at,
            started_at: row.started_at,
            completed_at: row.completed_at,
            user_message: row.user_message,
            context_snapshot: None,
            complexity_score: None,
            delegated_to: None,
            primary_model: None,
            delegation_reason: None,
            response_id: None,
            parent_response_id: None,
            parent_operation_id: None,
            target_language: None,
            target_framework: None,
            operation_intent: None,
            files_affected: None,
            result: row.result,
            error: row.error,
            tokens_input: None,
            tokens_output: None,
            tokens_reasoning: None,
            cost_usd: None,
            delegate_calls: row.delegate_calls.unwrap_or(0),
            metadata: None,
        })
    }

    pub async fn get_operation_events(&self, operation_id: &str) -> Result<Vec<OperationEvent>> {
        let rows = sqlx::query!(
            r#"
            SELECT id, event_type, created_at, sequence_number, event_data
            FROM operation_events
            WHERE operation_id = ?
            ORDER BY sequence_number ASC
            "#,
            operation_id
        )
        .fetch_all(&*self.db)
        .await?;

        let events = rows.into_iter().map(|row| {
            OperationEvent {
                id: row.id.unwrap_or(0),
                operation_id: operation_id.to_string(),
                event_type: row.event_type,
                created_at: row.created_at,
                sequence_number: row.sequence_number,
                event_data: row.event_data,
            }
        }).collect();

        Ok(events)
    }
}
