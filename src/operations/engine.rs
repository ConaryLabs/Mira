// src/operations/engine.rs

use crate::operations::{Artifact, Operation, OperationEvent, get_delegation_tools, parse_tool_call};
use crate::llm::provider::gpt5::{Gpt5Provider, Gpt5StreamEvent};
use crate::llm::provider::deepseek::DeepSeekProvider;
use crate::llm::provider::Message;
use crate::memory::service::MemoryService;
use crate::memory::RecallContext;
use crate::prompt::UnifiedPromptBuilder;
use crate::persona::PersonaOverlay;
use crate::tools::types::Tool;
use crate::config::CONFIG;
use crate::relationship::{RelationshipService, FactsService};
use anyhow::{Context, Result};
use sqlx::SqlitePool;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use futures::StreamExt;
use sha2::{Sha256, Digest};
use tracing::{info, warn, debug};

/// Events emitted during operation lifecycle
/// These get sent via channel for WebSocket streaming
#[derive(Debug, Clone)]
pub enum OperationEngineEvent {
    /// Operation started
    Started {
        operation_id: String,
    },
    /// Status changed
    StatusChanged {
        operation_id: String,
        old_status: String,
        new_status: String,
    },
    /// Content streaming from GPT-5
    Streaming {
        operation_id: String,
        content: String,
    },
    /// Delegated to another model
    Delegated {
        operation_id: String,
        delegated_to: String,
        reason: String,
    },
    /// Artifact preview available
    ArtifactPreview {
        operation_id: String,
        artifact_id: String,
        path: String,
        preview: String,
    },
    /// Artifact completed
    ArtifactCompleted {
        operation_id: String,
        artifact: Artifact,
    },
    /// Operation completed successfully
    Completed {
        operation_id: String,
        result: Option<String>,
    },
    /// Operation failed
    Failed {
        operation_id: String,
        error: String,
    },
}

/// Core orchestrator for operation lifecycle
/// Manages: create → run → complete
/// Handles: GPT-5 streaming, DeepSeek delegation, event emission, DB tracking
/// PHASE 8: Now with memory storage and context loading
/// STEP 5: Now with relationship integration
pub struct OperationEngine {
    db: Arc<SqlitePool>,
    gpt5: Gpt5Provider,
    deepseek: DeepSeekProvider,
    memory_service: Arc<MemoryService>,
    relationship_service: Arc<RelationshipService>,
    facts_service: Arc<FactsService>,
}

impl OperationEngine {
    /// Create a new operation engine
    /// PHASE 8: Requires MemoryService for context/storage
    /// STEP 5: Requires RelationshipService and FactsService
    pub fn new(
        db: Arc<SqlitePool>,
        gpt5: Gpt5Provider,
        deepseek: DeepSeekProvider,
        memory_service: Arc<MemoryService>,
        relationship_service: Arc<RelationshipService>,
        facts_service: Arc<FactsService>,
    ) -> Self {
        Self { 
            db, 
            gpt5, 
            deepseek,
            memory_service,
            relationship_service,
            facts_service,
        }
    }

    /// Create a new operation in the database
    /// Returns the operation with generated ID
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

    /// Start an operation
    /// Updates started_at and status to "planning"
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

        // Emit event
        self.emit_event(
            operation_id,
            "status_change",
            Some(serde_json::json!({
                "old_status": old_status,
                "new_status": new_status,
            })),
        )
        .await?;

        // Send engine event
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

    /// Complete an operation successfully
    /// PHASE 8: Stores result in memory
    /// STEP 5: Processes relationship updates
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

        // PHASE 8: Store assistant response in memory
        if let Some(ref response_content) = result {
            match self.memory_service.save_assistant_message(
                session_id,
                response_content,
                None,
            ).await {
                Ok(msg_id) => {
                    info!("Stored operation result in memory: message_id={}", msg_id);
                    
                    // PHASE 8.5: Process assistant message embeddings
                    if let Err(e) = self.process_assistant_message_embeddings(
                        msg_id,
                        response_content,
                    ).await {
                        warn!("Failed to process assistant message embeddings {}: {}", msg_id, e);
                    }
                    
                    // STEP 5: Process relationship updates from response
                    if let Err(e) = self.process_relationship_updates(
                        session_id,
                        response_content,
                        None, // TODO: Get relationship_impact from analysis
                    ).await {
                        warn!("Failed to process relationship updates: {}", e);
                    }
                }
                Err(e) => {
                    warn!("Failed to store operation result in memory: {}", e);
                }
            }
        }

        // Emit event
        self.emit_event(
            operation_id,
            "status_change",
            Some(serde_json::json!({
                "old_status": old_status,
                "new_status": new_status,
            })),
        )
        .await?;

        // Send engine event
        let _ = event_tx
            .send(OperationEngineEvent::StatusChanged {
                operation_id: operation_id.to_string(),
                old_status,
                new_status: new_status.to_string(),
            })
            .await;

        let _ = event_tx
            .send(OperationEngineEvent::Completed {
                operation_id: operation_id.to_string(),
                result,
            })
            .await;

        Ok(())
    }

    /// STEP 5: Process relationship updates from LLM response
    /// Delegates to RelationshipService for actual processing
    async fn process_relationship_updates(
        &self,
        session_id: &str,
        _response_content: &str,
        relationship_impact: Option<&str>,
    ) -> Result<()> {
        let user_id = session_id;
        
        // Delegate to relationship service
        self.relationship_service
            .process_llm_updates(user_id, relationship_impact)
            .await?;
        
        Ok(())
    }

    /// Fail an operation
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

        // Emit event
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

        // Send engine event
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

    /// Run an operation with full orchestration
    /// PHASE 8: Now loads context and stores messages
    /// STEP 5: Now includes user profile in system prompt
    pub async fn run_operation(
        &self,
        operation_id: &str,
        session_id: &str,
        user_content: &str,
        project_id: Option<&str>,
        cancel_token: Option<CancellationToken>,
        event_tx: &mpsc::Sender<OperationEngineEvent>,
    ) -> Result<()> {
        // Check cancellation before starting
        if let Some(token) = &cancel_token {
            if token.is_cancelled() {
                return Err(anyhow::anyhow!("Operation cancelled before start"));
            }
        }

        // PHASE 8 STEP 1: Store user message in memory
        let user_msg_id = self.memory_service.save_user_message(
            session_id,
            user_content,
            project_id,
        ).await?;
        
        info!("Stored user message in memory: message_id={}", user_msg_id);

        // PHASE 8.5: Process user message (analyze + embed with smart skip logic)
        if let Err(e) = self.process_user_message_embeddings(
            user_msg_id,
            user_content,
        ).await {
            warn!("Failed to process user message embeddings {}: {}", user_msg_id, e);
        }

        // PHASE 8 STEP 2: Load memory context
        let recall_context = self.load_memory_context(session_id, user_content).await?;
        
        // PHASE 8 STEP 3 + STEP 5: Build system prompt with context AND user profile
        let system_prompt = self.build_system_prompt_with_context(session_id, &recall_context).await;
        
        // Build messages
        let messages = vec![Message::user(user_content.to_string())];

        // Start the operation
        self.start_operation(operation_id, event_tx).await?;

        // Build delegation tools
        let tools = get_delegation_tools();
        let previous_response_id = None;

        // Check cancellation before GPT-5 call
        if let Some(token) = &cancel_token {
            if token.is_cancelled() {
                let _ = self.fail_operation(operation_id, "Operation cancelled".to_string(), event_tx).await;
                return Err(anyhow::anyhow!("Operation cancelled"));
            }
        }

        // Create GPT-5 stream with tools
        let mut stream = self.gpt5
            .create_stream_with_tools(messages, system_prompt, tools, previous_response_id)
            .await
            .context("Failed to create GPT-5 stream")?;

        let mut accumulated_text = String::new();
        let mut tool_calls = Vec::new();
        let mut _final_response_id = None;

        // Process the stream with cancellation checks
        while let Some(event) = stream.next().await {
            // Check cancellation during streaming
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
                    tool_calls.push((id, name, arguments));
                }
                Gpt5StreamEvent::Done { response_id, .. } => {
                    _final_response_id = Some(response_id);
                }
                _ => {}
            }
        }

        // Handle tool calls (delegation to DeepSeek)
        if !tool_calls.is_empty() {
            // Check cancellation before delegation
            if let Some(token) = &cancel_token {
                if token.is_cancelled() {
                    let _ = self.fail_operation(operation_id, "Operation cancelled before delegation".to_string(), event_tx).await;
                    return Err(anyhow::anyhow!("Operation cancelled"));
                }
            }

            self.update_operation_status(operation_id, "delegating", event_tx).await?;

            for (_tool_id, tool_name, tool_args) in tool_calls {
                // Check cancellation before each delegation
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

                if let Some(artifact_data) = result.get("artifact") {
                    self.create_artifact(operation_id, artifact_data.clone(), event_tx).await?;
                }
            }

            self.update_operation_status(operation_id, "generating", event_tx).await?;
        }

        // Complete the operation (now stores in memory + processes relationships)
        self.complete_operation(operation_id, session_id, Some(accumulated_text), event_tx).await?;

        Ok(())
    }

    /// PHASE 8: Load memory context for operation
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

    /// PHASE 8 + STEP 5: Build system prompt with memory AND relationship context
    async fn build_system_prompt_with_context(
        &self,
        session_id: &str,
        context: &RecallContext,
    ) -> String {
        let persona = PersonaOverlay::Default;
        let tools_json = get_delegation_tools();
        
        let tools: Vec<Tool> = tools_json.iter()
            .filter_map(|v| serde_json::from_value::<Tool>(v.clone()).ok())
            .collect();
        
        // STEP 5: Try to load user profile context
        let user_id = session_id;
        let profile_context = self.relationship_service
            .context_loader()
            .get_llm_context_string(user_id)
            .await
            .ok()
            .filter(|s| !s.is_empty());
        
        if profile_context.is_some() {
            info!("Loaded profile context for user {}", user_id);
        }
        
        // Build base prompt
        let mut base_prompt = UnifiedPromptBuilder::build_system_prompt(
            &persona,
            context,
            Some(&tools),
            None,
            None,
        );
        
        // Inject profile context if available
        if let Some(profile) = profile_context {
            base_prompt.push_str("\n\n## User Context\n");
            base_prompt.push_str(&profile);
        }
        
        base_prompt
    }

    /// Delegate to DeepSeek with the given tool call
    async fn delegate_to_deepseek(
        &self,
        operation_id: &str,
        tool_name: &str,
        tool_args: serde_json::Value,
        _event_tx: &mpsc::Sender<OperationEngineEvent>,
        cancel_token: Option<CancellationToken>,
    ) -> Result<serde_json::Value> {
        // Check cancellation before DeepSeek call
        if let Some(token) = &cancel_token {
            if token.is_cancelled() {
                return Err(anyhow::anyhow!("Operation cancelled before DeepSeek delegation"));
            }
        }

        let request = match tool_name {
            "generate_code" => {
                let path = tool_args.get("path")
                    .and_then(|v| v.as_str())
                    .unwrap_or("untitled.rs")
                    .to_string();
                let description = tool_args.get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let language = tool_args.get("language")
                    .and_then(|v| v.as_str())
                    .unwrap_or("rust")
                    .to_string();

                crate::llm::provider::deepseek::CodeGenRequest {
                    path,
                    description,
                    language,
                    framework: tool_args.get("framework").and_then(|v| v.as_str()).map(|s| s.to_string()),
                    dependencies: tool_args.get("dependencies")
                        .and_then(|v| v.as_array())
                        .map(|arr| arr.iter().filter_map(|v| v.as_str()).map(|s| s.to_string()).collect())
                        .unwrap_or_default(),
                    style_guide: tool_args.get("style_guide").and_then(|v| v.as_str()).map(|s| s.to_string()),
                    context: tool_args.get("context")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                }
            }
            "refactor_code" => {
                let path = tool_args.get("path")
                    .and_then(|v| v.as_str())
                    .unwrap_or("untitled.rs")
                    .to_string();
                let description = format!(
                    "Refactor existing code. Goals: {}",
                    tool_args.get("refactor_goals")
                        .and_then(|v| v.as_array())
                        .map(|arr| arr.iter()
                            .filter_map(|v| v.as_str())
                            .collect::<Vec<_>>()
                            .join(", "))
                        .unwrap_or_else(|| "improve code quality".to_string())
                );

                crate::llm::provider::deepseek::CodeGenRequest {
                    path,
                    description,
                    language: tool_args.get("language").and_then(|v| v.as_str()).unwrap_or("rust").to_string(),
                    framework: None,
                    dependencies: vec![],
                    style_guide: None,
                    context: tool_args.get("existing_code")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                }
            }
            "debug_code" => {
                let path = tool_args.get("path")
                    .and_then(|v| v.as_str())
                    .unwrap_or("untitled.rs")
                    .to_string();
                let description = format!(
                    "Debug and fix code. Error: {}",
                    tool_args.get("error_description")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown error")
                );

                crate::llm::provider::deepseek::CodeGenRequest {
                    path,
                    description,
                    language: tool_args.get("language").and_then(|v| v.as_str()).unwrap_or("rust").to_string(),
                    framework: None,
                    dependencies: vec![],
                    style_guide: None,
                    context: tool_args.get("problematic_code")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                }
            }
            _ => {
                return Err(anyhow::anyhow!("Unknown tool: {}", tool_name));
            }
        };

        // Increment delegate_calls counter
        sqlx::query!(
            "UPDATE operations SET delegate_calls = delegate_calls + 1 WHERE id = ?",
            operation_id
        )
        .execute(&*self.db)
        .await?;

        // Call DeepSeek
        let response = self.deepseek.generate_code(request).await?;

        // Return artifact data
        Ok(serde_json::json!({
            "artifact": {
                "path": response.artifact.path,
                "content": response.artifact.content,
                "language": response.artifact.language,
                "explanation": response.artifact.explanation,
            }
        }))
    }

    /// Create an artifact from DeepSeek response
    async fn create_artifact(
        &self,
        operation_id: &str,
        artifact_data: serde_json::Value,
        event_tx: &mpsc::Sender<OperationEngineEvent>,
    ) -> Result<()> {
        let path = artifact_data.get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing artifact path"))?;
        let content = artifact_data.get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing artifact content"))?;
        let language = artifact_data.get("language")
            .and_then(|v| v.as_str())
            .unwrap_or("plaintext");

        // Generate content hash
        let mut hasher = Sha256::new();
        hasher.update(content.as_bytes());
        let content_hash = format!("{:x}", hasher.finalize());

        // Create artifact
        let artifact = Artifact::new(
            operation_id.to_string(),
            "code".to_string(),
            Some(path.to_string()),
            content.to_string(),
            content_hash,
            Some(language.to_string()),
            None,
        );

        // Store in database
        sqlx::query!(
            r#"
            INSERT INTO artifacts (
                id, operation_id, kind, file_path, content, language,
                content_hash, created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            "#,
            artifact.id,
            artifact.operation_id,
            artifact.kind,
            artifact.file_path,
            artifact.content,
            artifact.language,
            artifact.content_hash,
            artifact.created_at,
        )
        .execute(&*self.db)
        .await
        .context("Failed to create artifact")?;

        // Emit events
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

        Ok(())
    }

    /// Update operation status
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

    /// Emit an operation event to the database
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

    /// Get next sequence number for an operation's events
    async fn get_next_sequence_number(&self, operation_id: &str) -> Result<i64> {
        let result = sqlx::query!(
            "SELECT COALESCE(MAX(sequence_number), -1) as max_seq FROM operation_events WHERE operation_id = ?",
            operation_id
        )
        .fetch_one(&*self.db)
        .await?;

        Ok((result.max_seq + 1) as i64)
    }

    /// Get current operation status
    async fn get_operation_status(&self, operation_id: &str) -> Result<String> {
        let result = sqlx::query!(
            "SELECT status FROM operations WHERE id = ?",
            operation_id
        )
        .fetch_one(&*self.db)
        .await?;

        Ok(result.status)
    }

    /// Get an operation by ID
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

    /// Get all events for an operation
    pub async fn get_operation_events(&self, operation_id: &str) -> Result<Vec<OperationEvent>> {
        let rows = sqlx::query!(
            r#"
            SELECT event_type, created_at, sequence_number, event_data
            FROM operation_events
            WHERE operation_id = ?
            ORDER BY sequence_number ASC
            "#,
            operation_id
        )
        .fetch_all(&*self.db)
        .await?;

        let events = rows.into_iter().map(|row| {
            let event_data = row.event_data
                .and_then(|s| serde_json::from_str(&s).ok());

            OperationEvent {
                id: 0,
                operation_id: operation_id.to_string(),
                event_type: row.event_type,
                created_at: row.created_at,
                sequence_number: row.sequence_number,
                event_data,
            }
        }).collect();

        Ok(events)
    }

    /// PHASE 8.5: Process user message embeddings
    async fn process_user_message_embeddings(
        &self,
        message_id: i64,
        content: &str,
    ) -> Result<()> {
        let pipeline_result = self.memory_service
            .message_pipeline
            .get_pipeline()
            .analyze_message(content, "user", None)
            .await?;

        if !pipeline_result.should_embed {
            debug!("Message {} skipped embedding (low salience or not relevant)", message_id);
            return Ok(());
        }

        info!("Would process embeddings for user message {}", message_id);
        
        Ok(())
    }

    /// PHASE 8.5: Process assistant message embeddings
    async fn process_assistant_message_embeddings(
        &self,
        message_id: i64,
        content: &str,
    ) -> Result<()> {
        let pipeline_result = self.memory_service
            .message_pipeline
            .get_pipeline()
            .analyze_message(content, "assistant", None)
            .await?;

        if !pipeline_result.should_embed {
            debug!("Message {} skipped embedding (low salience or large code)", message_id);
            return Ok(());
        }

        info!("Would process embeddings for assistant message {}", message_id);
        
        Ok(())
    }
}
