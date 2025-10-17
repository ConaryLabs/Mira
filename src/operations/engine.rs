// src/operations/engine.rs

use crate::operations::{Artifact, Operation, OperationEvent};
use anyhow::{Context, Result};
use sqlx::SqlitePool;
use std::sync::Arc;
use tokio::sync::mpsc;

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
}

impl OperationEngine {
    /// Create a new operation engine
    pub fn new(db: Arc<SqlitePool>) -> Self {
        Self { db }
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
        let event_data_json = event_data.map(|v| v.to_string());

        sqlx::query!(
            r#"
            INSERT INTO operation_events (
                operation_id, event_type, event_data, sequence_number, created_at
            ) VALUES (?, ?, ?, ?, ?)
            "#,
            operation_id,
            event_type,
            event_data_json,
            sequence_number,
            created_at,
        )
        .execute(&*self.db)
        .await
        .context("Failed to emit event")?;

        Ok(())
    }

    /// Get next sequence number for an operation's events
    async fn get_next_sequence_number(&self, operation_id: &str) -> Result<i64> {
        let result = sqlx::query!(
            r#"
            SELECT COALESCE(MAX(sequence_number), 0) as "max_seq!"
            FROM operation_events
            WHERE operation_id = ?
            "#,
            operation_id,
        )
        .fetch_one(&*self.db)
        .await
        .context("Failed to get next sequence number")?;

        Ok(result.max_seq as i64 + 1)
    }

    /// Get current operation status
    async fn get_operation_status(&self, operation_id: &str) -> Result<String> {
        let result = sqlx::query!(
            r#"
            SELECT status
            FROM operations
            WHERE id = ?
            "#,
            operation_id,
        )
        .fetch_one(&*self.db)
        .await
        .context("Failed to get operation status")?;

        Ok(result.status)
    }

    /// Get an operation by ID
    pub async fn get_operation(&self, operation_id: &str) -> Result<Operation> {
        sqlx::query_as::<_, Operation>(
            r#"
            SELECT * FROM operations WHERE id = ?
            "#,
        )
        .bind(operation_id)
        .fetch_one(&*self.db)
        .await
        .context("Failed to get operation")
    }

    /// Get all events for an operation
    pub async fn get_operation_events(&self, operation_id: &str) -> Result<Vec<OperationEvent>> {
        sqlx::query_as::<_, OperationEvent>(
            r#"
            SELECT * FROM operation_events
            WHERE operation_id = ?
            ORDER BY sequence_number ASC
            "#,
        )
        .bind(operation_id)
        .fetch_all(&*self.db)
        .await
        .context("Failed to get operation events")
    }

    // ===== STUB METHODS FOR PHASE 5 =====
    // These will be implemented in Phase 5 when we integrate GPT-5 and DeepSeek
    
    /// Run an operation (STUB - Phase 5)
    /// This will orchestrate: GPT-5 analysis → DeepSeek delegation → artifact creation
    pub async fn run_operation(
        &self,
        _operation_id: &str,
        _event_tx: mpsc::Sender<OperationEngineEvent>,
    ) -> Result<()> {
        // Phase 5: Implement full orchestration
        // 1. Start operation
        // 2. Call GPT-5 with streaming
        // 3. Handle tool calls → delegate to DeepSeek
        // 4. Create artifacts
        // 5. Complete operation
        
        todo!("Phase 5: Implement full operation orchestration")
    }

    /// Stream content to client (STUB - Phase 5)
    async fn stream_content(
        &self,
        operation_id: &str,
        content: String,
        event_tx: &mpsc::Sender<OperationEngineEvent>,
    ) -> Result<()> {
        // Emit streaming event
        self.emit_event(
            operation_id,
            "gpt5_analysis",
            Some(serde_json::json!({
                "content": content,
            })),
        )
        .await?;

        // Send engine event
        let _ = event_tx
            .send(OperationEngineEvent::Streaming {
                operation_id: operation_id.to_string(),
                content,
            })
            .await;

        Ok(())
    }

    /// Delegate to DeepSeek (STUB - Phase 5)
    async fn delegate_to_deepseek(
        &self,
        _operation_id: &str,
        _tool_call: String,
        _args: serde_json::Value,
        _event_tx: &mpsc::Sender<OperationEngineEvent>,
    ) -> Result<String> {
        // Phase 5: Implement DeepSeek delegation
        // 1. Update operation status to "delegated"
        // 2. Create child operation with parent_response_id
        // 3. Call DeepSeek with tool args
        // 4. Stream results
        // 5. Create artifacts
        // 6. Return tool result
        
        todo!("Phase 5: Implement DeepSeek delegation")
    }

    /// Create an artifact (STUB - Phase 5)
    pub async fn create_artifact(
        &self,
        _operation_id: &str,
        _kind: String,
        _file_path: Option<String>,
        _content: String,
        _language: Option<String>,
        _event_tx: &mpsc::Sender<OperationEngineEvent>,
    ) -> Result<Artifact> {
        // Phase 5: Implement artifact creation
        // 1. Calculate content hash
        // 2. Check for previous artifact (for diffing)
        // 3. Generate diff if exists
        // 4. Insert to database
        // 5. Emit events (preview + completed)
        
        todo!("Phase 5: Implement artifact creation")
    }
}
