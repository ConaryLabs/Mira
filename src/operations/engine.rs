// src/operations/engine.rs
// UPDATED: complete_operation now retrieves and includes artifacts

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
use crate::relationship::RelationshipService;
use anyhow::{Context, Result};
use sqlx::SqlitePool;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use futures::StreamExt;
use sha2::{Sha256, Digest};
use tracing::{info, warn, debug};

/// Events emitted during operation lifecycle
/// UPDATED: Completed now includes artifacts array
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
    /// UPDATED: Now includes artifacts
    Completed {
        operation_id: String,
        result: Option<String>,
        artifacts: Vec<Artifact>,  // ← NEW: Artifacts attached to completion
    },
    Failed {
        operation_id: String,
        error: String,
    },
}

pub struct OperationEngine {
    pub db: Arc<SqlitePool>,
    pub gpt5: Gpt5Provider,
    pub deepseek: DeepSeekProvider,
    pub memory_service: Arc<MemoryService>,
    pub relationship_service: Arc<RelationshipService>,
}

impl OperationEngine {
    pub fn new(
        db: Arc<SqlitePool>,
        gpt5: Gpt5Provider,
        deepseek: DeepSeekProvider,
        memory_service: Arc<MemoryService>,
        relationship_service: Arc<RelationshipService>,
    ) -> Self {
        Self {
            db,
            gpt5,
            deepseek,
            memory_service,
            relationship_service,
        }
    }

    pub async fn create_operation(
        &self,
        session_id: String,
        kind: String,
        user_message: String,
    ) -> Result<Operation> {
        let operation = Operation::new(session_id, kind, user_message);

        sqlx::query!(
            r#"
            INSERT INTO operations (
                id, session_id, kind, status, created_at, user_message, delegate_calls
            ) VALUES (?, ?, ?, ?, ?, ?, ?)
            "#,
            operation.id,
            operation.session_id,
            operation.kind,
            operation.status,
            operation.created_at,
            operation.user_message,
            operation.delegate_calls,
        )
        .execute(&*self.db)
        .await
        .context("Failed to create operation")?;

        Ok(operation)
    }

    pub async fn start_operation(
        &self,
        operation_id: &str,
        event_tx: &mpsc::Sender<OperationEngineEvent>,
    ) -> Result<()> {
        let started_at = chrono::Utc::now().timestamp();
        let old_status = self.get_operation_status(operation_id).await?;
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

    /// UPDATED: Now retrieves artifacts and includes them in Completed event
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
                    
                    if let Err(e) = self.process_assistant_message_embeddings(
                        msg_id,
                        response_content,
                    ).await {
                        warn!("Failed to process assistant message embeddings {}: {}", msg_id, e);
                    }
                    
                    if let Err(e) = self.process_relationship_updates(
                        session_id,
                        response_content,
                        None,
                    ).await {
                        warn!("Failed to process relationship updates: {}", e);
                    }
                }
                Err(e) => {
                    warn!("Failed to store operation result in memory: {}", e);
                }
            }
        }

        // CRITICAL FIX: Retrieve artifacts for this operation
        let artifacts = self.get_artifacts_for_operation(operation_id).await?;
        
        info!("Operation {} completed with {} artifacts", operation_id, artifacts.len());

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
            .send(OperationEngineEvent::StatusChanged {
                operation_id: operation_id.to_string(),
                old_status,
                new_status: new_status.to_string(),
            })
            .await;

        // CRITICAL FIX: Include artifacts in Completed event
        let _ = event_tx
            .send(OperationEngineEvent::Completed {
                operation_id: operation_id.to_string(),
                result,
                artifacts,  // ← NEW: Artifacts now included
            })
            .await;

        Ok(())
    }

    /// NEW: Retrieve all artifacts for an operation
    async fn get_artifacts_for_operation(&self, operation_id: &str) -> Result<Vec<Artifact>> {
        let artifacts = sqlx::query_as!(
            Artifact,
            r#"
            SELECT id, operation_id, kind, file_path, content, content_hash, language, diff, created_at
            FROM artifacts
            WHERE operation_id = ?
            ORDER BY created_at ASC
            "#,
            operation_id
        )
        .fetch_all(&*self.db)
        .await
        .context("Failed to retrieve artifacts")?;

        Ok(artifacts)
    }

    async fn process_relationship_updates(
        &self,
        session_id: &str,
        _response_content: &str,
        relationship_impact: Option<&str>,
    ) -> Result<()> {
        let user_id = session_id;
        
        self.relationship_service
            .process_llm_updates(user_id, relationship_impact)
            .await?;
        
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

        if let Err(e) = self.process_user_message_embeddings(
            user_msg_id,
            user_content,
        ).await {
            warn!("Failed to process user message embeddings {}: {}", user_msg_id, e);
        }

        let recall_context = self.load_memory_context(session_id, user_content).await?;
        
        info!("Loaded context: {} recent, {} semantic memories", 
              recall_context.recent_messages.len(), 
              recall_context.semantic_matches.len());

        let user_profile = self.relationship_service.get_user_profile(session_id).await;
        
        let mut prompt_builder = UnifiedPromptBuilder::new()
            .with_content(user_content);

        if let Some(profile) = user_profile {
            prompt_builder = prompt_builder.with_user_profile(&profile);
        }

        prompt_builder = prompt_builder.with_recall_context(recall_context);

        let system_prompt = prompt_builder.build()?;
        
        let messages = vec![
            Message {
                role: "user".to_string(),
                content: system_prompt,
            }
        ];

        let delegation_tools = get_delegation_tools();

        let stream = self.gpt5.create_chat_completion_stream(
            messages,
            Some(delegation_tools),
            None,
        ).await?;

        let mut accumulated_text = String::new();
        let mut tool_calls: Vec<(String, String, serde_json::Value)> = Vec::new();
        let mut _final_response_id: Option<String> = None;

        tokio::pin!(stream);

        while let Some(event) = stream.next().await {
            if let Some(token) = &cancel_token {
                if token.is_cancelled() {
                    let _ = self.fail_operation(operation_id, "Operation cancelled".to_string(), event_tx).await;
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

        if !tool_calls.is_empty() {
            if let Some(token) = &cancel_token {
                if token.is_cancelled() {
                    let _ = self.fail_operation(operation_id, "Operation cancelled before delegation".to_string(), event_tx).await;
                    return Err(anyhow::anyhow!("Operation cancelled"));
                }
            }

            self.update_operation_status(operation_id, "delegating", event_tx).await?;

            for (_tool_id, tool_name, tool_args) in tool_calls {
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

        self.complete_operation(operation_id, session_id, Some(accumulated_text), event_tx).await?;

        Ok(())
    }

    async fn load_memory_context(&self, session_id: &str, user_content: &str) -> Result<RecallContext> {
        self.memory_service.recall_engine.load_context(
            session_id,
            user_content,
            None,
        ).await
    }

    async fn process_user_message_embeddings(&self, message_id: i64, content: &str) -> Result<()> {
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

    async fn process_assistant_message_embeddings(&self, message_id: i64, content: &str) -> Result<()> {
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

    async fn delegate_to_deepseek(
        &self,
        operation_id: &str,
        tool_name: &str,
        tool_args: serde_json::Value,
        _event_tx: &mpsc::Sender<OperationEngineEvent>,
        _cancel_token: Option<CancellationToken>,
    ) -> Result<serde_json::Value> {
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

                crate::llm::provider::deepseek::CodeGenRequest {
                    path,
                    description,
                    language: tool_args.get("language").and_then(|v| v.as_str()).unwrap_or("rust").to_string(),
                    framework: tool_args.get("framework").and_then(|v| v.as_str()).map(String::from),
                    dependencies: vec![],
                    style_guide: None,
                    context: String::new(),
                }
            }
            "refactor_code" => {
                let path = tool_args.get("path")
                    .and_then(|v| v.as_str())
                    .unwrap_or("untitled.rs")
                    .to_string();
                let description = tool_args.get("refactor_description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Refactor code")
                    .to_string();

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

        sqlx::query!(
            "UPDATE operations SET delegate_calls = delegate_calls + 1 WHERE id = ?",
            operation_id
        )
        .execute(&*self.db)
        .await?;

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

        let mut hasher = Sha256::new();
        hasher.update(content.as_bytes());
        let content_hash = format!("{:x}", hasher.finalize());

        let artifact = Artifact::new(
            operation_id.to_string(),
            "code".to_string(),
            Some(path.to_string()),
            content.to_string(),
            content_hash,
            Some(language.to_string()),
            None,
        );

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
            id: row.id,
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
            delegate_calls: row.delegate_calls,
            metadata: None,
        })
    }

    pub async fn get_operation_events(&self, operation_id: &str) -> Result<Vec<OperationEvent>> {
        let rows = sqlx::query_as!(
            OperationEvent,
            r#"
            SELECT id, operation_id, event_type, created_at, sequence_number, event_data
            FROM operation_events
            WHERE operation_id = ?
            ORDER BY sequence_number ASC
            "#,
            operation_id
        )
        .fetch_all(&*self.db)
        .await?;

        Ok(rows)
    }
}
