//! HTTP handlers for status and message history

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::types::{MessageBlock, MessageWithUsage, MessagesQuery, UsageInfo};
use super::AppState;

/// Health check and status endpoint
pub async fn status_handler(State(state): State<AppState>) -> Json<Value> {
    Json(json!({
        "status": "ok",
        "semantic_search": state.semantic.is_available(),
        "database": state.db.is_some(),
        "model": "deepseek-reasoner",
        "default_reasoning_effort": state.default_reasoning_effort,
    }))
}

/// Paginated message history endpoint
pub async fn messages_handler(
    State(state): State<AppState>,
    Query(params): Query<MessagesQuery>,
) -> Result<Json<Vec<MessageWithUsage>>, (StatusCode, String)> {
    let Some(db) = &state.db else {
        return Ok(Json(vec![]));
    };

    // Query active (non-archived) messages with their usage data
    let messages: Vec<(String, String, String, i64, Option<i32>, Option<i32>, Option<i32>, Option<i32>)> = if let Some(before) = params.before {
        sqlx::query_as(
            r#"
            SELECT m.id, m.role, m.blocks, m.created_at,
                   u.input_tokens, u.output_tokens, u.reasoning_tokens, u.cached_tokens
            FROM chat_messages m
            LEFT JOIN chat_usage u ON u.message_id = m.id
            WHERE m.archived_at IS NULL AND m.created_at < $1
            ORDER BY m.created_at DESC
            LIMIT $2
            "#,
        )
        .bind(before)
        .bind(params.limit)
        .fetch_all(db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    } else {
        sqlx::query_as(
            r#"
            SELECT m.id, m.role, m.blocks, m.created_at,
                   u.input_tokens, u.output_tokens, u.reasoning_tokens, u.cached_tokens
            FROM chat_messages m
            LEFT JOIN chat_usage u ON u.message_id = m.id
            WHERE m.archived_at IS NULL
            ORDER BY m.created_at DESC
            LIMIT $1
            "#,
        )
        .bind(params.limit)
        .fetch_all(db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    };

    let result: Vec<MessageWithUsage> = messages
        .into_iter()
        .map(|(id, role, blocks_json, created_at, input, output, reasoning, cached)| {
            let blocks: Vec<MessageBlock> =
                serde_json::from_str(&blocks_json).unwrap_or_default();
            let usage = if input.is_some() || output.is_some() {
                Some(UsageInfo {
                    input_tokens: input.unwrap_or(0) as u32,
                    output_tokens: output.unwrap_or(0) as u32,
                    reasoning_tokens: reasoning.unwrap_or(0) as u32,
                    cached_tokens: cached.unwrap_or(0) as u32,
                })
            } else {
                None
            };
            MessageWithUsage {
                id,
                role,
                blocks,
                created_at,
                usage,
            }
        })
        .collect();

    Ok(Json(result))
}

// ============================================================================
// Orchestration Endpoints (Claude Code Activity & Instructions)
// ============================================================================

#[derive(Deserialize)]
pub struct McpHistoryQuery {
    #[serde(default = "default_limit")]
    pub limit: i64,
    pub tool_name: Option<String>,
}

fn default_limit() -> i64 { 50 }

#[derive(Serialize)]
pub struct McpHistoryEntry {
    pub id: i64,
    pub tool_name: String,
    pub args_preview: String,
    pub result_summary: Option<String>,
    pub success: bool,
    pub duration_ms: Option<i64>,
    pub created_at: String,
}

/// Get recent MCP tool call history (Claude Code activity)
pub async fn mcp_history_handler(
    State(state): State<AppState>,
    Query(params): Query<McpHistoryQuery>,
) -> Result<Json<Vec<McpHistoryEntry>>, (StatusCode, String)> {
    let Some(db) = &state.db else {
        return Ok(Json(vec![]));
    };

    let rows: Vec<(i64, String, String, Option<String>, bool, Option<i64>, String)> = if let Some(tool) = &params.tool_name {
        sqlx::query_as(
            r#"SELECT id, tool_name, args_json, result_summary, success, duration_ms, created_at
               FROM mcp_history
               WHERE tool_name LIKE $1
               ORDER BY created_at DESC
               LIMIT $2"#
        )
        .bind(format!("%{}%", tool))
        .bind(params.limit)
        .fetch_all(db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    } else {
        sqlx::query_as(
            r#"SELECT id, tool_name, args_json, result_summary, success, duration_ms, created_at
               FROM mcp_history
               ORDER BY created_at DESC
               LIMIT $1"#
        )
        .bind(params.limit)
        .fetch_all(db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    };

    let entries: Vec<McpHistoryEntry> = rows
        .into_iter()
        .map(|(id, tool_name, args_json, result_summary, success, duration_ms, created_at)| {
            let args_preview = if args_json.len() > 100 {
                format!("{}...", &args_json[..100])
            } else {
                args_json
            };
            McpHistoryEntry {
                id,
                tool_name,
                args_preview,
                result_summary,
                success,
                duration_ms,
                created_at,
            }
        })
        .collect();

    Ok(Json(entries))
}

#[derive(Deserialize)]
pub struct InstructionsQuery {
    #[serde(default = "default_limit")]
    pub limit: i64,
    pub status: Option<String>,
}

#[derive(Serialize)]
pub struct InstructionEntry {
    pub id: String,
    pub instruction: String,
    pub context: Option<String>,
    pub priority: String,
    pub status: String,
    pub created_at: String,
    pub completed_at: Option<String>,
    pub result: Option<String>,
    pub error: Option<String>,
}

/// List instructions in the queue
pub async fn instructions_handler(
    State(state): State<AppState>,
    Query(params): Query<InstructionsQuery>,
) -> Result<Json<Vec<InstructionEntry>>, (StatusCode, String)> {
    let Some(db) = &state.db else {
        return Ok(Json(vec![]));
    };

    let rows: Vec<(String, String, Option<String>, String, String, String, Option<String>, Option<String>, Option<String>)> =
        if let Some(status) = &params.status {
            if status == "all" {
                sqlx::query_as(
                    r#"SELECT id, instruction, context, priority, status, created_at, completed_at, result, error
                       FROM instruction_queue
                       ORDER BY created_at DESC
                       LIMIT $1"#
                )
                .bind(params.limit)
                .fetch_all(db)
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
            } else {
                sqlx::query_as(
                    r#"SELECT id, instruction, context, priority, status, created_at, completed_at, result, error
                       FROM instruction_queue
                       WHERE status = $1
                       ORDER BY created_at DESC
                       LIMIT $2"#
                )
                .bind(status)
                .bind(params.limit)
                .fetch_all(db)
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
            }
        } else {
            // Default: show active (pending, delivered, in_progress)
            sqlx::query_as(
                r#"SELECT id, instruction, context, priority, status, created_at, completed_at, result, error
                   FROM instruction_queue
                   WHERE status IN ('pending', 'delivered', 'in_progress')
                   ORDER BY
                     CASE status
                       WHEN 'in_progress' THEN 1
                       WHEN 'pending' THEN 2
                       WHEN 'delivered' THEN 3
                       ELSE 4
                     END,
                     created_at DESC
                   LIMIT $1"#
            )
            .bind(params.limit)
            .fetch_all(db)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        };

    let entries: Vec<InstructionEntry> = rows
        .into_iter()
        .map(|(id, instruction, context, priority, status, created_at, completed_at, result, error)| {
            InstructionEntry {
                id,
                instruction,
                context,
                priority,
                status,
                created_at,
                completed_at,
                result,
                error,
            }
        })
        .collect();

    Ok(Json(entries))
}

#[derive(Deserialize)]
pub struct CreateInstructionRequest {
    pub instruction: String,
    pub context: Option<String>,
    #[serde(default = "default_priority")]
    pub priority: String,
}

fn default_priority() -> String { "normal".to_string() }

#[derive(Serialize)]
pub struct CreateInstructionResponse {
    pub id: String,
    pub status: String,
}

/// Create a new instruction
pub async fn create_instruction_handler(
    State(state): State<AppState>,
    Json(req): Json<CreateInstructionRequest>,
) -> Result<Json<CreateInstructionResponse>, (StatusCode, String)> {
    let Some(db) = &state.db else {
        return Err((StatusCode::SERVICE_UNAVAILABLE, "Database not available".to_string()));
    };

    let id = format!("instr_{}", uuid::Uuid::new_v4().to_string().split('-').next().unwrap_or("0"));

    sqlx::query(
        r#"INSERT INTO instruction_queue (id, instruction, context, priority, status)
           VALUES ($1, $2, $3, $4, 'pending')"#
    )
    .bind(&id)
    .bind(&req.instruction)
    .bind(&req.context)
    .bind(&req.priority)
    .execute(db)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(CreateInstructionResponse {
        id,
        status: "pending".to_string(),
    }))
}
