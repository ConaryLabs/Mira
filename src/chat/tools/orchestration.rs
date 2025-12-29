//! Orchestration tools for Studio -> Claude Code communication
//!
//! These tools allow Studio to:
//! - View Claude Code's recent MCP activity
//! - Send instructions for Claude Code to execute
//! - Track instruction status

use anyhow::Result;
use serde_json::{json, Value};
use sqlx::SqlitePool;

/// Orchestration tools for managing Claude Code
pub struct OrchestrationTools<'a> {
    pub db: &'a Option<SqlitePool>,
}

impl<'a> OrchestrationTools<'a> {
    /// View recent MCP tool calls from Claude Code
    pub async fn view_claude_activity(&self, args: &Value) -> Result<String> {
        let Some(db) = self.db else {
            return Ok("Error: Database not available".to_string());
        };

        let tool_name = args.get("tool_name").and_then(|v| v.as_str());
        let query = args.get("query").and_then(|v| v.as_str());
        let limit = args.get("limit").and_then(|v| v.as_i64()).unwrap_or(20).min(100) as i32;

        // Build query dynamically based on filters
        let rows = if let Some(tool) = tool_name {
            sqlx::query_as::<_, (String, String, Option<String>, String)>(
                r#"SELECT tool_name, arguments, result_summary, created_at
                   FROM mcp_history
                   WHERE tool_name LIKE $1
                   ORDER BY created_at DESC
                   LIMIT $2"#
            )
            .bind(format!("%{}%", tool))
            .bind(limit)
            .fetch_all(db)
            .await?
        } else if let Some(q) = query {
            sqlx::query_as::<_, (String, String, Option<String>, String)>(
                r#"SELECT tool_name, arguments, result_summary, created_at
                   FROM mcp_history
                   WHERE result_summary LIKE $1 OR arguments LIKE $1
                   ORDER BY created_at DESC
                   LIMIT $2"#
            )
            .bind(format!("%{}%", q))
            .bind(limit)
            .fetch_all(db)
            .await?
        } else {
            sqlx::query_as::<_, (String, String, Option<String>, String)>(
                r#"SELECT tool_name, arguments, result_summary, created_at
                   FROM mcp_history
                   ORDER BY created_at DESC
                   LIMIT $1"#
            )
            .bind(limit)
            .fetch_all(db)
            .await?
        };

        if rows.is_empty() {
            return Ok("No recent Claude Code activity found.".to_string());
        }

        let mut output = Vec::new();
        output.push(format!("# Recent Claude Code Activity ({} entries)\n", rows.len()));

        for (tool_name, arguments, result_summary, created_at) in rows {
            // Truncate args for display
            let args_preview = if arguments.len() > 100 {
                format!("{}...", &arguments[..100])
            } else {
                arguments
            };

            let summary = result_summary.unwrap_or_else(|| "(no summary)".to_string());
            output.push(format!(
                "**{}** @ {}\n  Args: {}\n  Result: {}\n",
                tool_name, created_at, args_preview, summary
            ));
        }

        Ok(output.join("\n"))
    }

    /// Send an instruction for Claude Code to pick up
    pub async fn send_instruction(&self, args: &Value) -> Result<String> {
        let Some(db) = self.db else {
            return Ok("Error: Database not available".to_string());
        };

        let instruction = args.get("instruction")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("instruction required"))?;

        let context = args.get("context").and_then(|v| v.as_str());
        let priority = args.get("priority")
            .and_then(|v| v.as_str())
            .unwrap_or("normal");

        // Generate a unique ID
        let id = format!("instr_{}", uuid::Uuid::new_v4().to_string().split('-').next().unwrap_or("0"));

        // Get project_id if available
        let project_id: Option<(i64,)> = sqlx::query_as(
            "SELECT id FROM projects WHERE path = (SELECT value FROM kv WHERE key = 'active_project_path') LIMIT 1"
        )
        .fetch_optional(db)
        .await?;

        let project_id = project_id.map(|(id,)| id);

        sqlx::query(
            r#"INSERT INTO instruction_queue (id, project_id, instruction, context, priority, status)
               VALUES ($1, $2, $3, $4, $5, 'pending')"#
        )
        .bind(&id)
        .bind(project_id)
        .bind(instruction)
        .bind(context)
        .bind(priority)
        .execute(db)
        .await?;

        Ok(json!({
            "status": "queued",
            "instruction_id": id,
            "priority": priority,
            "message": format!("Instruction queued. Claude Code will pick it up when checking for pending work.")
        }).to_string())
    }

    /// List instructions in the queue
    pub async fn list_instructions(&self, args: &Value) -> Result<String> {
        let Some(db) = self.db else {
            return Ok("Error: Database not available".to_string());
        };

        let status_filter = args.get("status").and_then(|v| v.as_str()).unwrap_or("all");
        let limit = args.get("limit").and_then(|v| v.as_i64()).unwrap_or(20) as i32;

        // Types: id, instruction, context, priority, status, created_at, completed_at
        let rows = if status_filter == "all" {
            sqlx::query_as::<_, (String, String, Option<String>, String, String, String, Option<String>)>(
                r#"SELECT id, instruction, context, priority, status, created_at, completed_at
                   FROM instruction_queue
                   ORDER BY
                     CASE status
                       WHEN 'in_progress' THEN 1
                       WHEN 'pending' THEN 2
                       WHEN 'delivered' THEN 3
                       ELSE 4
                     END,
                     CASE priority
                       WHEN 'urgent' THEN 1
                       WHEN 'high' THEN 2
                       WHEN 'normal' THEN 3
                       ELSE 4
                     END,
                     created_at DESC
                   LIMIT $1"#
            )
            .bind(limit)
            .fetch_all(db)
            .await?
        } else {
            sqlx::query_as::<_, (String, String, Option<String>, String, String, String, Option<String>)>(
                r#"SELECT id, instruction, context, priority, status, created_at, completed_at
                   FROM instruction_queue
                   WHERE status = $1
                   ORDER BY
                     CASE priority
                       WHEN 'urgent' THEN 1
                       WHEN 'high' THEN 2
                       WHEN 'normal' THEN 3
                       ELSE 4
                     END,
                     created_at DESC
                   LIMIT $2"#
            )
            .bind(status_filter)
            .bind(limit)
            .fetch_all(db)
            .await?
        };

        if rows.is_empty() {
            return Ok("No instructions found.".to_string());
        }

        let mut output = Vec::new();
        output.push(format!("# Instruction Queue ({} entries)\n", rows.len()));

        for (id, instruction, context, priority, status, created_at, completed_at) in rows {
            let status_icon = match status.as_str() {
                "pending" => "⏳",
                "delivered" => "📬",
                "in_progress" => "🔄",
                "completed" => "✅",
                "failed" => "❌",
                "cancelled" => "🚫",
                _ => "❓",
            };

            // Truncate instruction for display
            let instr_preview = if instruction.len() > 80 {
                format!("{}...", &instruction[..80])
            } else {
                instruction
            };

            let mut entry = format!(
                "{} **[{}]** {} ({})\n  {}\n  Created: {}",
                status_icon, id, priority.to_uppercase(), status, instr_preview, created_at
            );

            if let Some(ctx) = context {
                if !ctx.is_empty() {
                    let ctx_preview = if ctx.len() > 50 { format!("{}...", &ctx[..50]) } else { ctx };
                    entry.push_str(&format!("\n  Context: {}", ctx_preview));
                }
            }

            if let Some(completed) = completed_at {
                entry.push_str(&format!("\n  Completed: {}", completed));
            }

            output.push(entry);
        }

        Ok(output.join("\n\n"))
    }

    /// Cancel a pending instruction
    pub async fn cancel_instruction(&self, args: &Value) -> Result<String> {
        let Some(db) = self.db else {
            return Ok("Error: Database not available".to_string());
        };

        let instruction_id = args.get("instruction_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("instruction_id required"))?;

        // Only cancel if still pending
        let result = sqlx::query(
            r#"UPDATE instruction_queue
               SET status = 'cancelled'
               WHERE id = $1 AND status = 'pending'"#
        )
        .bind(instruction_id)
        .execute(db)
        .await?;

        if result.rows_affected() > 0 {
            Ok(json!({
                "status": "cancelled",
                "instruction_id": instruction_id
            }).to_string())
        } else {
            // Check if it exists but is not pending
            let exists: Option<(String,)> = sqlx::query_as(
                "SELECT status FROM instruction_queue WHERE id = $1"
            )
            .bind(instruction_id)
            .fetch_optional(db)
            .await?;

            match exists {
                Some((status,)) => Ok(json!({
                    "error": format!("Cannot cancel instruction in '{}' status", status),
                    "instruction_id": instruction_id
                }).to_string()),
                None => Ok(json!({
                    "error": "Instruction not found",
                    "instruction_id": instruction_id
                }).to_string()),
            }
        }
    }
}
