// src/operations/engine.rs

use crate::operations::{Artifact, Operation, OperationEvent, get_delegation_tools, parse_tool_call};
use crate::llm::provider::gpt5::{Gpt5Provider, Gpt5StreamEvent};
use crate::llm::provider::deepseek::DeepSeekProvider;
use crate::llm::provider::Message;
use anyhow::{Context, Result};
use sqlx::SqlitePool;
use std::sync::Arc;
use tokio::sync::mpsc;
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

    /// Run the full operation: GPT-5 analysis → tool call detection → DeepSeek execution
    /// This is the heart of Phase 6
    pub async fn run_operation(
        &self,
        operation_id: &str,
        messages: Vec<Message>,
        system_prompt: String,
        previous_response_id: Option<String>,
        event_tx: &mpsc::Sender<OperationEngineEvent>,
    ) -> Result<String> {
        // Start the operation
        self.start_operation(operation_id, event_tx).await?;

        // Get delegation tools
        let tools = get_delegation_tools();

        // Update status to "analyzing"
        self.update_operation_status(operation_id, "analyzing", event_tx).await?;

        // Create GPT-5 stream with tools
        let mut stream = self.gpt5
            .create_stream_with_tools(messages, system_prompt, tools, previous_response_id)
            .await
            .context("Failed to create GPT-5 stream")?;

        let mut accumulated_text = String::new();
        let mut tool_calls = Vec::new();
        let mut _final_response_id = None; // Prefixed with _ to silence unused warning

        // Process the stream
        while let Some(event) = stream.next().await {
            match event? {
                Gpt5StreamEvent::TextDelta { delta } => {
                    // Stream content to frontend
                    accumulated_text.push_str(&delta);
                    let _ = event_tx.send(OperationEngineEvent::Streaming {
                        operation_id: operation_id.to_string(),
                        content: delta,
                    }).await;
                }
                Gpt5StreamEvent::ToolCallComplete { id, name, arguments } => {
                    // Accumulate tool calls
                    tool_calls.push((id, name, arguments));
                }
                Gpt5StreamEvent::Done { response_id, .. } => {
                    // Save response_id for next turn
                    _final_response_id = Some(response_id);
                }
                _ => {}
            }
        }

        // Handle tool calls (delegation to DeepSeek)
        if !tool_calls.is_empty() {
            self.update_operation_status(operation_id, "delegating", event_tx).await?;

            for (_tool_id, tool_name, tool_args) in tool_calls {
                // Build tool call JSON in format expected by parse_tool_call
                // tool_args is already a Value (JSON), convert to string for parse_tool_call
                let tool_call_json = serde_json::json!({
                    "function": {
                        "name": tool_name,
                        "arguments": serde_json::to_string(&tool_args)?,
                    }
                });

                // Parse the tool call
                let (parsed_name, parsed_args) = parse_tool_call(&tool_call_json)?;

                // Emit delegation event
                let _ = event_tx.send(OperationEngineEvent::Delegated {
                    operation_id: operation_id.to_string(),
                    delegated_to: "deepseek".to_string(),
                    reason: format!("Tool call: {}", parsed_name),
                }).await;

                // Delegate to DeepSeek
                let result = self.delegate_to_deepseek(
                    operation_id,
                    &parsed_name,
                    parsed_args,
                    event_tx
                ).await?;

                // Create artifact from result
                if let Some(artifact_data) = result.get("artifact") {
                    self.create_artifact(operation_id, artifact_data.clone(), event_tx).await?;
                }
            }

            self.update_operation_status(operation_id, "completed", event_tx).await?;
        }

        // Build final response
        let response_text = if accumulated_text.is_empty() {
            "Operation completed successfully".to_string()
        } else {
            accumulated_text
        };

        // Complete operation
        self.complete_operation(operation_id, Some(response_text.clone()), event_tx).await?;

        Ok(response_text)
    }

    /// Delegate to DeepSeek for code generation
    async fn delegate_to_deepseek(
        &self,
        operation_id: &str,
        tool_name: &str,
        args: serde_json::Value,
        event_tx: &mpsc::Sender<OperationEngineEvent>,
    ) -> Result<serde_json::Value> {
        use crate::llm::provider::deepseek::CodeGenRequest;

        // Increment delegate_calls counter
        sqlx::query!(
            "UPDATE operations SET delegate_calls = delegate_calls + 1 WHERE id = ?",
            operation_id
        )
        .execute(&*self.db)
        .await?;

        // Build CodeGenRequest based on tool type
        let request = match tool_name {
            "generate_code" => {
                CodeGenRequest {
                    path: args.get("path")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| anyhow::anyhow!("Missing 'path' in generate_code args"))?
                        .to_string(),
                    description: args.get("description")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| anyhow::anyhow!("Missing 'description' in generate_code args"))?
                        .to_string(),
                    language: args.get("language")
                        .and_then(|v| v.as_str())
                        .unwrap_or("rust")
                        .to_string(),
                    framework: args.get("framework")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                    dependencies: args.get("dependencies")
                        .and_then(|v| v.as_array())
                        .map(|arr| arr.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect())
                        .unwrap_or_default(),
                    style_guide: args.get("style_guide")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                    context: String::new(), // TODO: Add relevant context from memory/files
                }
            }
            "refactor_code" => {
                let current_code = args.get("current_code")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("Missing 'current_code' in refactor_code args"))?;
                let changes = args.get("changes_requested")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("Missing 'changes_requested' in refactor_code args"))?;

                CodeGenRequest {
                    path: args.get("path")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| anyhow::anyhow!("Missing 'path' in refactor_code args"))?
                        .to_string(),
                    description: format!("Refactor code with changes: {}\n\nCurrent code:\n{}", changes, current_code),
                    language: args.get("language")
                        .and_then(|v| v.as_str())
                        .unwrap_or("rust")
                        .to_string(),
                    framework: None,
                    dependencies: Vec::new(),
                    style_guide: args.get("preserve_behavior")
                        .and_then(|v| v.as_bool())
                        .map(|preserve| if preserve {
                            "Preserve exact behavior and functionality".to_string()
                        } else {
                            "Improve behavior while refactoring".to_string()
                        }),
                    context: format!("Original code:\n{}", current_code),
                }
            }
            "debug_code" => {
                let buggy_code = args.get("buggy_code")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("Missing 'buggy_code' in debug_code args"))?;
                let error_msg = args.get("error_message")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("Missing 'error_message' in debug_code args"))?;
                let expected = args.get("expected_behavior")
                    .and_then(|v| v.as_str())
                    .map(|s| format!("\n\nExpected behavior: {}", s))
                    .unwrap_or_default();

                CodeGenRequest {
                    path: args.get("path")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| anyhow::anyhow!("Missing 'path' in debug_code args"))?
                        .to_string(),
                    description: format!("Debug and fix this code.\n\nError: {}{}", error_msg, expected),
                    language: args.get("language")
                        .and_then(|v| v.as_str())
                        .unwrap_or("rust")
                        .to_string(),
                    framework: None,
                    dependencies: Vec::new(),
                    style_guide: Some("Fix bugs while maintaining code style".to_string()),
                    context: format!("Buggy code:\n{}\n\nError message:\n{}", buggy_code, error_msg),
                }
            }
            _ => return Err(anyhow::anyhow!("Unknown tool: {}", tool_name)),
        };

        // Call DeepSeek
        let response = self.deepseek.generate_code(request).await?;

        // Emit artifact preview
        let preview = response.artifact.content.chars().take(200).collect::<String>();
        
        let _ = event_tx.send(OperationEngineEvent::ArtifactPreview {
            operation_id: operation_id.to_string(),
            artifact_id: uuid::Uuid::new_v4().to_string(),
            path: response.artifact.path.clone(),
            preview,
        }).await;

        // Return as JSON
        Ok(serde_json::to_value(&response)?)
    }

    /// Create an artifact from DeepSeek response
    async fn create_artifact(
        &self,
        operation_id: &str,
        response_data: serde_json::Value,
        event_tx: &mpsc::Sender<OperationEngineEvent>,
    ) -> Result<Artifact> {
        // Parse the CodeGenResponse from JSON
        let code_response: crate::llm::provider::deepseek::CodeGenResponse = 
            serde_json::from_value(response_data)?;

        let artifact_data = code_response.artifact;

        // Create hash for deduplication using SHA256
        let mut hasher = Sha256::new();
        hasher.update(artifact_data.content.as_bytes());
        let content_hash = format!("{:x}", hasher.finalize());

        // Check if artifact with same hash exists
        let existing = sqlx::query!(
            "SELECT id FROM artifacts WHERE content_hash = ? LIMIT 1",
            content_hash
        )
        .fetch_optional(&*self.db)
        .await?;

        if existing.is_some() {
            // Artifact already exists, skip
            return Err(anyhow::anyhow!("Artifact with same content already exists"));
        }

        // Insert artifact using Artifact::new()
        let artifact = Artifact::new(
            operation_id.to_string(),
            "code".to_string(),
            Some(artifact_data.path),
            artifact_data.content,
            content_hash,
            Some(artifact_data.language),
            None, // No diff for now
        );

        sqlx::query!(
            r#"
            INSERT INTO artifacts (
                id, operation_id, kind, file_path, content, content_hash,
                language, created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            "#,
            artifact.id,
            artifact.operation_id,
            artifact.kind,
            artifact.file_path,
            artifact.content,
            artifact.content_hash,
            artifact.language,
            artifact.created_at,
        )
        .execute(&*self.db)
        .await?;

        // Emit artifact completed event
        let _ = event_tx.send(OperationEngineEvent::ArtifactCompleted {
            operation_id: operation_id.to_string(),
            artifact: artifact.clone(),
        }).await;

        Ok(artifact)
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
    /// Events are stored with sequence numbers for ordering
    async fn emit_event(
        &self,
        operation_id: &str,
        event_type: &str,
        event_data: Option<serde_json::Value>,
    ) -> Result<()> {
        // Get next sequence number
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

        // max_seq comes back as i32 from SQLite, cast to i64
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
            r#"SELECT 
                id, session_id, kind, status, created_at, started_at, completed_at,
                user_message, context_snapshot, complexity_score, delegated_to,
                primary_model, delegation_reason, response_id, parent_response_id,
                parent_operation_id, target_language, target_framework,
                operation_intent, files_affected, result, error, tokens_input,
                tokens_output, tokens_reasoning, cost_usd, delegate_calls, metadata
            FROM operations WHERE id = ?"#,
            operation_id
        )
        .fetch_one(&*self.db)
        .await?;

        Ok(Operation {
            id: row.id.unwrap_or_default(), // Should never be None, but handle it
            session_id: row.session_id,
            kind: row.kind,
            status: row.status,
            created_at: row.created_at,
            started_at: row.started_at,
            completed_at: row.completed_at,
            user_message: row.user_message,
            context_snapshot: row.context_snapshot,
            complexity_score: row.complexity_score,
            delegated_to: row.delegated_to,
            primary_model: row.primary_model,
            delegation_reason: row.delegation_reason,
            response_id: row.response_id,
            parent_response_id: row.parent_response_id,
            parent_operation_id: row.parent_operation_id,
            target_language: row.target_language,
            target_framework: row.target_framework,
            operation_intent: row.operation_intent,
            files_affected: row.files_affected,
            result: row.result,
            error: row.error,
            tokens_input: row.tokens_input,
            tokens_output: row.tokens_output,
            tokens_reasoning: row.tokens_reasoning,
            cost_usd: row.cost_usd,
            delegate_calls: row.delegate_calls.unwrap_or(0),
            metadata: row.metadata,
        })
    }

    /// Get all events for an operation
    pub async fn get_operation_events(&self, operation_id: &str) -> Result<Vec<OperationEvent>> {
        let rows = sqlx::query!(
            "SELECT id, operation_id, event_type, created_at, sequence_number, event_data FROM operation_events WHERE operation_id = ? ORDER BY sequence_number ASC",
            operation_id
        )
        .fetch_all(&*self.db)
        .await?;

        let events = rows.into_iter().map(|row| {
            OperationEvent {
                id: row.id.unwrap_or(0), // Auto-increment, should never be None
                operation_id: row.operation_id,
                event_type: row.event_type,
                created_at: row.created_at,
                sequence_number: row.sequence_number,
                event_data: row.event_data,
            }
        }).collect();

        Ok(events)
    }
}
