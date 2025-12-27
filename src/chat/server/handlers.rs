//! HTTP handlers for status and message history

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{
        sse::{Event, KeepAlive, Sse},
        Json,
    },
};
use futures::stream::Stream;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::convert::Infallible;
use std::time::Duration;

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

#[derive(Serialize, Clone)]
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

#[derive(Serialize, Clone)]
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

// ============================================================================
// SSE Orchestration Stream
// ============================================================================

/// Events emitted by the orchestration SSE stream
#[derive(Serialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OrchestrationEvent {
    /// New instruction created or status changed
    InstructionUpdate {
        instruction: InstructionEntry,
    },
    /// New MCP tool call recorded
    McpActivity {
        entry: McpHistoryEntry,
    },
    /// Heartbeat to keep connection alive
    Heartbeat {
        ts: i64,
    },
}

/// SSE stream for orchestration events (instructions + MCP activity)
pub async fn orchestration_stream_handler(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let stream = async_stream::stream! {
        let Some(db) = &state.db else {
            // No database, just send heartbeats
            loop {
                let ts = chrono::Utc::now().timestamp();
                let event = OrchestrationEvent::Heartbeat { ts };
                let data = serde_json::to_string(&event).unwrap_or_default();
                yield Ok(Event::default().data(data));
                tokio::time::sleep(Duration::from_secs(30)).await;
            }
        };

        // Track last seen IDs to only send new entries
        let mut last_instruction_ts: Option<String> = None;
        let mut last_mcp_id: Option<i64> = None;

        // Initial load: get current latest timestamps
        if let Ok(row) = sqlx::query_as::<_, (Option<String>,)>(
            "SELECT MAX(created_at) FROM instruction_queue"
        ).fetch_one(db).await {
            last_instruction_ts = row.0;
        }
        if let Ok(row) = sqlx::query_as::<_, (Option<i64>,)>(
            "SELECT MAX(id) FROM mcp_history"
        ).fetch_one(db).await {
            last_mcp_id = row.0;
        }

        loop {
            // Check for new instructions
            let instr_rows: Vec<(String, String, Option<String>, String, String, String, Option<String>, Option<String>, Option<String>)> =
                if let Some(ref ts) = last_instruction_ts {
                    sqlx::query_as(
                        r#"SELECT id, instruction, context, priority, status, created_at, completed_at, result, error
                           FROM instruction_queue
                           WHERE created_at > $1
                           ORDER BY created_at ASC
                           LIMIT 20"#
                    )
                    .bind(ts)
                    .fetch_all(db)
                    .await
                    .unwrap_or_default()
                } else {
                    // First poll, get recent instructions
                    sqlx::query_as(
                        r#"SELECT id, instruction, context, priority, status, created_at, completed_at, result, error
                           FROM instruction_queue
                           ORDER BY created_at DESC
                           LIMIT 10"#
                    )
                    .fetch_all(db)
                    .await
                    .unwrap_or_default()
                };

            for row in instr_rows {
                let entry = InstructionEntry {
                    id: row.0,
                    instruction: row.1,
                    context: row.2,
                    priority: row.3,
                    status: row.4,
                    created_at: row.5.clone(),
                    completed_at: row.6,
                    result: row.7,
                    error: row.8,
                };
                last_instruction_ts = Some(row.5);
                let event = OrchestrationEvent::InstructionUpdate { instruction: entry };
                let data = serde_json::to_string(&event).unwrap_or_default();
                yield Ok(Event::default().data(data));
            }

            // Check for status updates on existing instructions
            // (This catches in_progress -> completed transitions)
            let updated_rows: Vec<(String, String, Option<String>, String, String, String, Option<String>, Option<String>, Option<String>)> =
                sqlx::query_as(
                    r#"SELECT id, instruction, context, priority, status, created_at, completed_at, result, error
                       FROM instruction_queue
                       WHERE status IN ('in_progress', 'completed', 'failed')
                         AND (completed_at IS NOT NULL OR status = 'in_progress')
                       ORDER BY COALESCE(completed_at, created_at) DESC
                       LIMIT 5"#
                )
                .fetch_all(db)
                .await
                .unwrap_or_default();

            for row in updated_rows {
                let entry = InstructionEntry {
                    id: row.0,
                    instruction: row.1,
                    context: row.2,
                    priority: row.3,
                    status: row.4,
                    created_at: row.5,
                    completed_at: row.6,
                    result: row.7,
                    error: row.8,
                };
                let event = OrchestrationEvent::InstructionUpdate { instruction: entry };
                let data = serde_json::to_string(&event).unwrap_or_default();
                yield Ok(Event::default().data(data));
            }

            // Check for new MCP history entries
            let mcp_rows: Vec<(i64, String, String, Option<String>, bool, Option<i64>, String)> =
                if let Some(id) = last_mcp_id {
                    sqlx::query_as(
                        r#"SELECT id, tool_name, args_json, result_summary, success, duration_ms, created_at
                           FROM mcp_history
                           WHERE id > $1
                           ORDER BY id ASC
                           LIMIT 20"#
                    )
                    .bind(id)
                    .fetch_all(db)
                    .await
                    .unwrap_or_default()
                } else {
                    // First poll, get recent entries
                    sqlx::query_as(
                        r#"SELECT id, tool_name, args_json, result_summary, success, duration_ms, created_at
                           FROM mcp_history
                           ORDER BY id DESC
                           LIMIT 10"#
                    )
                    .fetch_all(db)
                    .await
                    .unwrap_or_default()
                };

            for row in mcp_rows {
                let args_preview = if row.2.len() > 100 {
                    format!("{}...", &row.2[..100])
                } else {
                    row.2
                };
                let entry = McpHistoryEntry {
                    id: row.0,
                    tool_name: row.1,
                    args_preview,
                    result_summary: row.3,
                    success: row.4,
                    duration_ms: row.5,
                    created_at: row.6,
                };
                last_mcp_id = Some(row.0);
                let event = OrchestrationEvent::McpActivity { entry };
                let data = serde_json::to_string(&event).unwrap_or_default();
                yield Ok(Event::default().data(data));
            }

            // Wait before next poll
            tokio::time::sleep(Duration::from_secs(2)).await;

            // Send heartbeat every few cycles
            let ts = chrono::Utc::now().timestamp();
            let event = OrchestrationEvent::Heartbeat { ts };
            let data = serde_json::to_string(&event).unwrap_or_default();
            yield Ok(Event::default().data(data));
        }
    };

    Sse::new(stream).keep_alive(KeepAlive::default())
}
