//! Proactive context tools for Chat
//!
//! Delegates to the MCP proactive implementation for shared logic.

use anyhow::Result;
use serde_json::{json, Value};
use sqlx::SqlitePool;
use std::path::Path;
use std::sync::Arc;

use crate::core::SemanticSearch;
use crate::tools::proactive as mcp_proactive;
use crate::tools::types::GetProactiveContextRequest;

/// Proactive context tool implementations
pub struct ProactiveTools<'a> {
    pub cwd: &'a Path,
    pub db: &'a Option<SqlitePool>,
    pub semantic: &'a Option<Arc<SemanticSearch>>,
}

impl<'a> ProactiveTools<'a> {
    /// Get project_id from cwd
    async fn get_project_id(&self) -> Option<i64> {
        let db = self.db.as_ref()?;
        let project_path = self.cwd.to_string_lossy().to_string();
        sqlx::query_scalar("SELECT id FROM projects WHERE path = $1")
            .bind(&project_path)
            .fetch_optional(db)
            .await
            .ok()
            .flatten()
    }

    /// Get all relevant context for the current work
    pub async fn get_proactive_context(&self, args: &Value) -> Result<String> {
        let db = match &self.db {
            Some(db) => db,
            None => return Ok("Error: database not configured".into()),
        };

        let semantic = match &self.semantic {
            Some(sem) => sem,
            None => return Ok("Error: semantic search not configured".into()),
        };

        let project_id = self.get_project_id().await;

        // Parse files array
        let files: Option<Vec<String>> = args
            .get("files")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            });

        // Parse topics array
        let topics: Option<Vec<String>> = args
            .get("topics")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            });

        let req = GetProactiveContextRequest {
            files,
            topics,
            task: args["task"].as_str().map(String::from),
            error: args["error"].as_str().map(String::from),
            limit_per_category: args["limit_per_category"].as_i64().map(|v| v as i32),
        };

        // Delegate to MCP implementation
        match mcp_proactive::get_proactive_context(db, semantic, req, project_id).await {
            Ok(context) => Ok(context.to_string()),
            Err(e) => Ok(format!("Error: {}", e)),
        }
    }
}
