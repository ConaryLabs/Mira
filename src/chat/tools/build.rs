//! Build tracking tools for Chat
//!
//! Thin wrapper delegating to core::ops::build for shared implementation with MCP.

use anyhow::Result;
use serde_json::{json, Value};
use sqlx::SqlitePool;
use std::path::Path;

use crate::core::ops::build as core_build;
use crate::core::OpContext;

/// Build tracking tool implementations
pub struct BuildTools<'a> {
    pub cwd: &'a Path,
    pub db: &'a Option<SqlitePool>,
}

impl<'a> BuildTools<'a> {
    /// Consolidated build tool - handles record, record_error, get_errors, resolve
    pub async fn build(&self, args: &Value) -> Result<String> {
        let action = args["action"].as_str().unwrap_or("get_errors");
        let db = match &self.db {
            Some(db) => db,
            None => return Ok("Error: database not configured".into()),
        };

        let ctx = OpContext::just_db(db.clone());

        match action {
            "record" => {
                let command = args["command"].as_str().unwrap_or("");
                if command.is_empty() {
                    return Ok("Error: command is required".into());
                }

                let input = core_build::RecordBuildInput {
                    command: command.to_string(),
                    success: args["success"].as_bool().unwrap_or(true),
                    duration_ms: args["duration_ms"].as_i64(),
                };

                match core_build::record_build(&ctx, input).await {
                    Ok(output) => Ok(json!({
                        "status": "recorded",
                        "build_run_id": output.build_run_id,
                        "command": output.command,
                        "success": output.success,
                    })
                    .to_string()),
                    Err(e) => Ok(format!("Error: {}", e)),
                }
            }

            "record_error" => {
                let message = args["message"].as_str().unwrap_or("");
                if message.is_empty() {
                    return Ok("Error: message is required".into());
                }

                let input = core_build::RecordBuildErrorInput {
                    message: message.to_string(),
                    category: args["category"].as_str().map(String::from),
                    severity: args["severity"].as_str().map(String::from),
                    file_path: args["file_path"].as_str().map(String::from),
                    line_number: args["line_number"].as_i64().map(|v| v as i32),
                    code: args["code"].as_str().map(String::from),
                };

                match core_build::record_build_error(&ctx, input).await {
                    Ok(output) => Ok(json!({
                        "status": "recorded",
                        "error_id": output.error_id,
                        "error_hash": output.error_hash,
                        "severity": output.severity,
                    })
                    .to_string()),
                    Err(e) => Ok(format!("Error: {}", e)),
                }
            }

            "get_errors" => {
                let input = core_build::GetBuildErrorsInput {
                    file_path: args["file_path"].as_str().map(String::from),
                    category: args["category"].as_str().map(String::from),
                    include_resolved: args["include_resolved"].as_bool().unwrap_or(false),
                    limit: args["limit"].as_i64().unwrap_or(20),
                };

                match core_build::get_build_errors(&ctx, input).await {
                    Ok(errors) => {
                        let errors_json: Vec<Value> = errors
                            .into_iter()
                            .map(|e| {
                                json!({
                                    "id": e.id,
                                    "category": e.category,
                                    "severity": e.severity,
                                    "message": e.message,
                                    "file_path": e.file_path,
                                    "line_number": e.line_number,
                                    "code": e.code,
                                    "resolved": e.resolved,
                                    "created_at": e.created_at,
                                })
                            })
                            .collect();
                        let count = errors_json.len();
                        Ok(json!({
                            "errors": errors_json,
                            "count": count,
                        })
                        .to_string())
                    }
                    Err(e) => Ok(format!("Error: {}", e)),
                }
            }

            "resolve" => {
                let error_id = args["error_id"].as_i64().unwrap_or(0);
                if error_id == 0 {
                    return Ok("Error: error_id is required".into());
                }

                match core_build::resolve_error(&ctx, error_id).await {
                    Ok(true) => Ok(json!({
                        "status": "resolved",
                        "error_id": error_id,
                    })
                    .to_string()),
                    Ok(false) => Ok(json!({
                        "status": "not_found",
                        "error_id": error_id,
                    })
                    .to_string()),
                    Err(e) => Ok(format!("Error: {}", e)),
                }
            }

            _ => Ok(format!(
                "Unknown action: {}. Use record/record_error/get_errors/resolve",
                action
            )),
        }
    }
}
