// src/api/ws/operations/mod.rs
// WebSocket handlers for operation lifecycle
// PHASE 8: Updated to pass session_id and user_content to run_operation

pub mod stream;

use crate::api::ws::message::SystemAccessMode;
use crate::operations::{OperationEngine, OperationEngineEvent};
use crate::project::ProjectStore;
use anyhow::Result;
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// Manages active operations and their cancellation tokens
pub struct OperationManager {
    engine: Arc<OperationEngine>,
    pool: SqlitePool,
    project_store: Arc<ProjectStore>,
    active_operations: Arc<tokio::sync::RwLock<HashMap<String, CancellationToken>>>,
}

impl OperationManager {
    pub fn new(engine: Arc<OperationEngine>, pool: SqlitePool, project_store: Arc<ProjectStore>) -> Self {
        Self {
            engine,
            pool,
            project_store,
            active_operations: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
        }
    }

    /// Start a new operation with cancellation support
    /// PHASE 8: Now calls run_operation with session_id and user_content
    pub async fn start_operation(
        &self,
        session_id: String,
        message: String,
        project_id: Option<String>,
        system_access_mode: SystemAccessMode,
        ws_tx: mpsc::Sender<serde_json::Value>,
    ) -> Result<String> {
        // Use provided project_id, or try to resolve from session's project_path
        let project_id = match project_id {
            Some(id) => Some(id),
            None => self.resolve_project_id(&session_id).await,
        };

        // 1. Create operation
        let op = self
            .engine
            .create_operation(
                session_id.clone(),
                "code_generation".to_string(),
                message.clone(),
            )
            .await?;

        // 2. Create cancellation token
        let cancel_token = CancellationToken::new();
        self.active_operations
            .write()
            .await
            .insert(op.id.clone(), cancel_token.clone());

        // 3. Create event channel
        let (event_tx, mut event_rx) = mpsc::channel(100);

        // 4. Spawn task to forward events to WebSocket
        tokio::spawn(async move {
            while let Some(event) = event_rx.recv().await {
                let json = stream::event_to_json(event);
                let _ = ws_tx.send(json).await;
            }
        });

        // 5. Spawn operation task
        let engine = self.engine.clone();
        let op_id = op.id.clone();
        let session = session_id.clone();
        let user_message = message.clone();
        let cancel = cancel_token.clone();
        let active_ops = self.active_operations.clone();
        let access_mode = system_access_mode;

        tokio::spawn(async move {
            tracing::info!(
                operation_id = %op_id,
                access_mode = ?access_mode,
                "Starting operation with system access mode"
            );

            // Pass project_id and system_access_mode for dynamic path resolution and enforcement
            let result = engine
                .run_operation(
                    &op_id,
                    &session,
                    &user_message,
                    project_id.as_deref(),
                    access_mode,
                    Some(cancel),
                    &event_tx,
                )
                .await;

            // Clean up
            active_ops.write().await.remove(&op_id);

            if let Err(e) = result {
                let _ = event_tx
                    .send(OperationEngineEvent::Failed {
                        operation_id: op_id,
                        error: e.to_string(),
                    })
                    .await;
            }
        });

        Ok(op.id)
    }

    /// Resolve project_id from session's project_path
    async fn resolve_project_id(&self, session_id: &str) -> Option<String> {
        // Look up session to get project_path
        let session_result: Result<Option<(Option<String>,)>, _> = sqlx::query_as(
            "SELECT project_path FROM chat_sessions WHERE id = ?"
        )
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await;

        if let Ok(Some((Some(project_path),))) = session_result {
            // Look up or create project by path
            match self.project_store.get_or_create_by_path(&project_path, None).await {
                Ok(project) => {
                    tracing::debug!(
                        session_id = %session_id,
                        project_id = %project.id,
                        "Resolved project from session path"
                    );
                    return Some(project.id);
                }
                Err(e) => {
                    tracing::debug!(
                        session_id = %session_id,
                        error = %e,
                        "Could not resolve project from session path"
                    );
                }
            }
        }
        None
    }

    /// Cancel an active operation
    pub async fn cancel_operation(&self, operation_id: &str) -> Result<()> {
        if let Some(token) = self.active_operations.read().await.get(operation_id) {
            token.cancel();
            Ok(())
        } else {
            Err(anyhow::anyhow!("Operation not found or already completed"))
        }
    }
}
