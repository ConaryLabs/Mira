// src/operations/engine.rs

use crate::operations::{Artifact, Operation, OperationEvent, get_delegation_tools, parse_tool_call};
use crate::llm::provider::gpt5::{Gpt5Provider, Gpt5StreamEvent};
use crate::llm::provider::deepseek::DeepSeekProvider;
use crate::llm::provider::Message;
use anyhow::{Context, Result};
use sqlx::SqlitePool;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use futures::StreamExt;
use sha2::{Sha256, Digest};

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
pub struct OperationEngine {
    db: Arc<SqlitePool>,
    gpt5: Gpt5Provider,
    deepseek: DeepSeekProvider,
}

impl OperationEngine {
    /// Create a new operation engine
    pub fn new(db: Arc<SqlitePool>, gpt5: Gpt5Provider, deepseek: DeepSeekProvider) -> Self {
        Self { db, gpt5, deepseek }
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
    pub async fn complete_operation(
        &self,
        operation_id: &str,
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
    /// UPDATED: Added cancel_token parameter for cancellation support
    pub async fn run_operation(
        &self,
        operation_id: &str,
        messages: Vec<Message>,
        system_prompt: String,
        cancel_token: Option<CancellationToken>,
        event_tx: &mpsc::Sender<OperationEngineEvent>,
    ) -> Result<()> {
        // Check cancellation before starting
        if let Some(token) = &cancel_token {
            if token.is_cancelled() {
                return Err(anyhow::anyhow!("Operation cancelled before start"));
            }
        }

        // Start the operation
        self.start_operation(operation_id, event_tx).await?;

        // Build delegation tools
        let tools = get_delegation_tools();
        let previous_response_id = None; // First turn, no previous response

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
                    cancel_token.clone(), // Pass cancel token to delegation
                ).await?;

                if let Some(artifact_data) = result.get("artifact") {
                    self.create_artifact(operation_id, artifact_data.clone(), event_tx).await?;
                }
            }

            self.update_operation_status(operation_id, "generating", event_tx).await?;
        }

        // Complete the operation
        self.complete_operation(operation_id, Some(accumulated_text), event_tx).await?;

        Ok(())
    }

    /// Delegate to DeepSeek with the given tool call
    /// UPDATED: Added cancel_token parameter
    async fn delegate_to_deepseek(
        &self,
        operation_id: &str,
        tool_name: &str,
        tool_args: serde_json::Value,
        event_tx: &mpsc::Sender<OperationEngineEvent>,
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

        // FIX: Changed .code to .content
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

        // FIX: Corrected Artifact::new() parameters (7 params, proper order)
        // Create artifact
        let artifact = Artifact::new(
            operation_id.to_string(),
            "code".to_string(),
            Some(path.to_string()),
            content.to_string(),
            content_hash,
            Some(language.to_string()),
            None, // diff
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
    /// FIX: Return all 26 fields required by Operation struct
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
    /// FIX: Include id and operation_id fields in OperationEvent
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
                id: 0, // Placeholder - DB auto-generates this
                operation_id: operation_id.to_string(),
                event_type: row.event_type,
                created_at: row.created_at,
                sequence_number: row.sequence_number,
                event_data,
            }
        }).collect();

        Ok(events)
    }
}
