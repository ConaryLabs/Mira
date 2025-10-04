// src/memory/features/code_intelligence/websocket_storage.rs
use sqlx::SqlitePool;
use anyhow::Result;
use serde::{Serialize, Deserialize};
use super::types::{WebSocketCall, WebSocketHandler, WebSocketResponse};

pub struct WebSocketStorage {
    pool: SqlitePool,
}

impl WebSocketStorage {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
    
    pub async fn store_websocket_calls(
        &self,
        project_id: &str,
        file_id: i64,
        file_path: &str,
        calls: &[WebSocketCall],
    ) -> Result<()> {
        for call in calls {
            // Extract frontend_element from file_path (simplified)
            let frontend_element = format!("{}::{}", file_path, call.line_number);
            let line_num = call.line_number as i64;
            
            sqlx::query!(
                "INSERT INTO websocket_calls 
                 (frontend_file_id, frontend_element, call_line, message_type, method, project_id)
                 VALUES (?, ?, ?, ?, ?, ?)",
                file_id,
                frontend_element,
                line_num,
                call.message_type,
                call.method,
                project_id
            )
            .execute(&self.pool)
            .await?;
        }
        
        Ok(())
    }
    
    pub async fn store_websocket_handlers(
        &self,
        project_id: &str,
        file_id: i64,
        handlers: &[WebSocketHandler],
    ) -> Result<()> {
        for handler in handlers {
            let line_num = handler.line_number as i64;
            
            sqlx::query!(
                "INSERT OR REPLACE INTO websocket_handlers 
                 (backend_file_id, handler_function, handler_line, message_type, method, project_id)
                 VALUES (?, ?, ?, ?, ?, ?)",
                file_id,
                handler.handler_function,
                line_num,
                handler.message_type,
                handler.method,
                project_id
            )
            .execute(&self.pool)
            .await?;
        }
        
        Ok(())
    }
    
    pub async fn store_websocket_responses(
        &self,
        project_id: &str,
        file_id: i64,
        responses: &[WebSocketResponse],
    ) -> Result<()> {
        for response in responses {
            let line_num = response.line_number as i64;
            
            sqlx::query!(
                "INSERT INTO websocket_responses 
                 (backend_file_id, sending_function, response_line, response_type, data_type, project_id)
                 VALUES (?, ?, ?, ?, ?, ?)",
                file_id,
                response.sending_function,
                line_num,
                response.response_type,
                response.data_type,
                project_id
            )
            .execute(&self.pool)
            .await?;
        }
        
        Ok(())
    }
    
    pub async fn link_calls_to_handlers(&self, project_id: &str) -> Result<()> {
        // Link frontend calls to backend handlers by matching message_type + method
        sqlx::query!(
            "UPDATE websocket_calls 
             SET handler_id = (
                 SELECT id FROM websocket_handlers 
                 WHERE websocket_handlers.project_id = websocket_calls.project_id
                   AND websocket_handlers.message_type = websocket_calls.message_type
                   AND (websocket_handlers.method = websocket_calls.method 
                        OR websocket_calls.method IS NULL)
                 LIMIT 1
             )
             WHERE project_id = ?",
            project_id
        )
        .execute(&self.pool)
        .await?;
        
        Ok(())
    }
    
    pub async fn find_orphaned_calls(&self, project_id: &str) -> Result<Vec<OrphanedCall>> {
        // Frontend calls with no backend handler
        let rows = sqlx::query!(
            "SELECT frontend_element, message_type, method
             FROM websocket_calls
             WHERE project_id = ? AND handler_id IS NULL",
            project_id
        )
        .fetch_all(&self.pool)
        .await?;
        
        Ok(rows.into_iter().map(|row| OrphanedCall {
            frontend_element: row.frontend_element,
            message_type: row.message_type,
            method: row.method,
        }).collect())
    }
    
    pub async fn find_unused_handlers(&self, project_id: &str) -> Result<Vec<UnusedHandler>> {
        // Backend handlers with no frontend callers
        let rows = sqlx::query!(
            "SELECT handler_function, message_type, method
             FROM websocket_handlers
             WHERE project_id = ?
               AND NOT EXISTS (
                   SELECT 1 FROM websocket_calls 
                   WHERE websocket_calls.handler_id = websocket_handlers.id
               )",
            project_id
        )
        .fetch_all(&self.pool)
        .await?;
        
        Ok(rows.into_iter().map(|row| UnusedHandler {
            handler_function: row.handler_function,
            message_type: row.message_type,
            method: row.method,
        }).collect())
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OrphanedCall {
    pub frontend_element: String,
    pub message_type: String,
    pub method: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UnusedHandler {
    pub handler_function: String,
    pub message_type: String,
    pub method: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DependencyReport {
    pub orphaned_calls: Vec<OrphanedCall>,
    pub unused_handlers: Vec<UnusedHandler>,
}
