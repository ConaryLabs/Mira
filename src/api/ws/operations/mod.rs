// src/api/ws/operations/mod.rs
// WebSocket handlers for operation lifecycle
// PHASE 8: Updated to pass session_id and user_content to run_operation

pub mod stream;

use crate::operations::{OperationEngine, OperationEngineEvent};
use crate::config::CONFIG;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use anyhow::Result;
use std::sync::Arc;
use std::collections::HashMap;

/// Manages active operations and their cancellation tokens
pub struct OperationManager {
    engine: Arc<OperationEngine>,
    active_operations: Arc<tokio::sync::RwLock<HashMap<String, CancellationToken>>>,
}

impl OperationManager {
    pub fn new(engine: Arc<OperationEngine>) -> Self {
        Self {
            engine,
            active_operations: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
        }
    }
    
    /// Start a new operation with cancellation support
    /// PHASE 8: Now calls run_operation with session_id and user_content
    pub async fn start_operation(
        &self,
        session_id: String,
        message: String,
        ws_tx: mpsc::Sender<serde_json::Value>,
    ) -> Result<String> {
        // 1. Create operation
        let op = self.engine.create_operation(
            session_id.clone(),
            "code_generation".to_string(),
            message.clone(),
        ).await?;
        
        // 2. Create cancellation token
        let cancel_token = CancellationToken::new();
        self.active_operations.write().await.insert(op.id.clone(), cancel_token.clone());
        
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
        // PHASE 8: Call new run_operation signature with session_id and user_content
        let engine = self.engine.clone();
        let op_id = op.id.clone();
        let session = session_id.clone();
        let user_message = message.clone();
        let cancel = cancel_token.clone();
        let active_ops = self.active_operations.clone();
        
        tokio::spawn(async move {
            // PHASE 8: New signature - passes session_id, user_content directly
            // Engine will handle context loading and message storage internally
            let result = engine.run_operation(
                &op_id,
                &session,
                &user_message,
                None, // No project_id for now
                Some(cancel),
                &event_tx,
            ).await;
            
            // Clean up
            active_ops.write().await.remove(&op_id);
            
            if let Err(e) = result {
                let _ = event_tx.send(OperationEngineEvent::Failed {
                    operation_id: op_id,
                    error: e.to_string(),
                }).await;
            }
        });
        
        Ok(op.id)
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
