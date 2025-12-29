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
        "model": "gemini-2.5-flash",
        "default_reasoning_effort": state.default_reasoning_effort,
    }))
}

// ============================================================================
// Health Monitor Endpoint
// ============================================================================

#[derive(Serialize)]
pub struct HealthInfo {
    pub db_size_bytes: i64,
    pub db_size_human: String,
    pub recent_errors: Vec<BuildErrorEntry>,
    pub active_session_id: Option<String>,
    pub active_session_status: Option<String>,
    pub mcp_session_id: Option<String>,
}

#[derive(Serialize)]
pub struct BuildErrorEntry {
    pub id: i64,
    pub category: Option<String>,
    pub severity: String,
    pub message: String,
    pub file_path: Option<String>,
    pub line_number: Option<i32>,
    pub resolved: bool,
    pub created_at: i64,
}

/// Health monitor endpoint - returns DB size, recent errors, and active session
pub async fn health_handler(
    State(state): State<AppState>,
) -> Result<Json<HealthInfo>, (StatusCode, String)> {
    let Some(db) = &state.db else {
        return Ok(Json(HealthInfo {
            db_size_bytes: 0,
            db_size_human: "N/A".to_string(),
            recent_errors: vec![],
            active_session_id: None,
            active_session_status: None,
            mcp_session_id: None,
        }));
    };

    // Get database file size
    let db_size: (i64,) = sqlx::query_as(
        "SELECT page_count * page_size as size FROM pragma_page_count(), pragma_page_size()"
    )
    .fetch_one(db)
    .await
    .unwrap_or((0,));

    let db_size_human = format_bytes(db_size.0);

    // Get last 5 build errors
    let error_rows: Vec<(i64, Option<String>, String, String, Option<String>, Option<i32>, bool, i64)> =
        sqlx::query_as(
            r#"SELECT id, category, severity, message, file_path, line_number, resolved, created_at
               FROM build_errors
               ORDER BY created_at DESC
               LIMIT 5"#
        )
        .fetch_all(db)
        .await
        .unwrap_or_default();

    let recent_errors: Vec<BuildErrorEntry> = error_rows
        .into_iter()
        .map(|(id, category, severity, message, file_path, line_number, resolved, created_at)| {
            BuildErrorEntry {
                id,
                category,
                severity,
                message: if message.len() > 200 { format!("{}...", &message[..200]) } else { message },
                file_path,
                line_number,
                resolved,
                created_at,
            }
        })
        .collect();

    // Get active Claude Code session
    let session: Option<(String, String)> = sqlx::query_as(
        r#"SELECT id, status FROM claude_sessions
           WHERE status IN ('running', 'starting', 'paused')
           ORDER BY created_at DESC
           LIMIT 1"#
    )
    .fetch_optional(db)
    .await
    .unwrap_or(None);

    let (active_session_id, active_session_status) = session
        .map(|(id, status)| (Some(id), Some(status)))
        .unwrap_or((None, None));

    // Get current MCP session ID from mcp_sessions
    let mcp_session: Option<(String,)> = sqlx::query_as(
        r#"SELECT session_id FROM mcp_sessions
           WHERE ended_at IS NULL
           ORDER BY started_at DESC
           LIMIT 1"#
    )
    .fetch_optional(db)
    .await
    .unwrap_or(None);

    let mcp_session_id = mcp_session.map(|(id,)| id);

    Ok(Json(HealthInfo {
        db_size_bytes: db_size.0,
        db_size_human,
        recent_errors,
        active_session_id,
        active_session_status,
        mcp_session_id,
    }))
}

/// Format bytes into human-readable string
fn format_bytes(bytes: i64) -> String {
    const KB: i64 = 1024;
    const MB: i64 = KB * 1024;
    const GB: i64 = MB * 1024;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
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
    /// Project path for spawning a session (optional - if provided, auto-spawns)
    pub project_path: Option<String>,
    /// Whether to auto-spawn a session for this instruction (default: true if project_path provided)
    #[serde(default)]
    pub auto_spawn: Option<bool>,
}

fn default_priority() -> String { "normal".to_string() }

#[derive(Serialize)]
pub struct CreateInstructionResponse {
    pub id: String,
    pub status: String,
    /// Session ID if a session was spawned for this instruction
    pub session_id: Option<String>,
}

/// Create a new instruction
///
/// If `project_path` is provided and `auto_spawn` is true (default), a Claude Code
/// session will be spawned to execute the instruction automatically.
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

    // Auto-spawn session if project_path is provided
    let should_spawn = req.auto_spawn.unwrap_or(req.project_path.is_some());
    let session_id = if should_spawn {
        if let (Some(spawner), Some(project_path)) = (&state.spawner, &req.project_path) {
            // Build prompt with instruction context
            let prompt = if let Some(ctx) = &req.context {
                format!("{}\n\nContext: {}", req.instruction, ctx)
            } else {
                req.instruction.clone()
            };

            // Create spawn config with instruction metadata
            let mut config = SpawnConfig::new(project_path, &prompt);
            config.instruction_id = Some(id.clone());
            config.system_prompt = Some(format!(
                "You are executing instruction '{}'. When complete, call mark_instruction with status='completed' and include a summary of what you did.",
                id
            ));

            match spawner.spawn(config).await {
                Ok(sid) => {
                    // Update instruction with session ID and mark as in_progress
                    let _ = sqlx::query(
                        "UPDATE instruction_queue SET status = 'in_progress', session_id = $1, started_at = datetime('now') WHERE id = $2"
                    )
                    .bind(&sid)
                    .bind(&id)
                    .execute(db)
                    .await;

                    tracing::info!(instruction_id = %id, session_id = %sid, "Auto-spawned session for instruction");
                    Some(sid)
                }
                Err(e) => {
                    tracing::warn!(instruction_id = %id, error = %e, "Failed to auto-spawn session");
                    None
                }
            }
        } else {
            None
        }
    } else {
        None
    };

    let status = if session_id.is_some() { "in_progress" } else { "pending" };

    Ok(Json(CreateInstructionResponse {
        id,
        status: status.to_string(),
        session_id,
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

// ============================================================================
// Claude Code Spawner Endpoints
// ============================================================================

use crate::spawner::{build_context_snapshot, SessionDetails, SpawnConfig};

#[derive(Deserialize)]
pub struct SpawnSessionRequest {
    /// Project directory to run Claude Code in
    pub project_path: String,
    /// Initial prompt/task for Claude Code
    pub prompt: String,
    /// Optional system prompt to append
    pub system_prompt: Option<String>,
    /// Budget in USD (default: $5.00)
    pub budget_usd: Option<f64>,
    /// Allowed tools (None = all)
    pub allowed_tools: Option<Vec<String>>,
    /// Build and attach Mira context (goals, decisions, corrections)
    #[serde(default)]
    pub build_context: bool,
    /// Key files for context relevance (used when build_context is true)
    pub key_files: Option<Vec<String>>,
}

#[derive(Serialize)]
pub struct SpawnSessionResponse {
    pub session_id: String,
    pub status: String,
}

/// Spawn a new Claude Code session
pub async fn spawn_session_handler(
    State(state): State<AppState>,
    Json(req): Json<SpawnSessionRequest>,
) -> Result<Json<SpawnSessionResponse>, (StatusCode, String)> {
    let spawner = state.spawner.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Spawner not available".to_string(),
    ))?;

    let mut config = SpawnConfig::new(&req.project_path, &req.prompt);

    if let Some(budget) = req.budget_usd {
        config = config.with_budget(budget);
    }
    if let Some(tools) = req.allowed_tools {
        config = config.with_tools(tools);
    }
    if let Some(sys) = req.system_prompt {
        config.system_prompt = Some(sys);
    }

    // Build Mira context if requested
    if req.build_context {
        if let Some(db) = state.db.as_ref() {
            let key_files = req.key_files.clone().unwrap_or_default();
            match build_context_snapshot(db, &req.prompt, None, key_files).await {
                Ok(snapshot) => {
                    tracing::info!(
                        goals = snapshot.active_goals.len(),
                        decisions = snapshot.relevant_decisions.len(),
                        corrections = snapshot.corrections.len(),
                        "Built context snapshot for session"
                    );
                    config = config.with_context(snapshot);
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Failed to build context, spawning without");
                }
            }
        }
    }

    let session_id = spawner
        .spawn(config)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(SpawnSessionResponse {
        session_id,
        status: "starting".to_string(),
    }))
}

#[derive(Serialize)]
pub struct SessionInfo {
    pub session_id: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spawned_at: Option<i64>,
}

/// List active Claude Code sessions
pub async fn list_sessions_handler(
    State(state): State<AppState>,
) -> Result<Json<Vec<SessionInfo>>, (StatusCode, String)> {
    let spawner = state.spawner.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Spawner not available".to_string(),
    ))?;

    let sessions: Vec<SessionDetails> = spawner.list_sessions().await;
    let result: Vec<SessionInfo> = sessions
        .into_iter()
        .map(|s| SessionInfo {
            session_id: s.session_id,
            status: s.status.as_str().to_string(),
            project_path: s.project_path,
            spawned_at: s.spawned_at,
        })
        .collect();

    Ok(Json(result))
}

#[derive(Deserialize)]
pub struct AnswerQuestionRequest {
    /// Question ID to answer
    pub question_id: String,
    /// The answer
    pub answer: String,
}

#[derive(Serialize)]
pub struct AnswerQuestionResponse {
    pub status: String,
}

/// Answer a pending question from Claude Code
pub async fn answer_question_handler(
    State(state): State<AppState>,
    Json(req): Json<AnswerQuestionRequest>,
) -> Result<Json<AnswerQuestionResponse>, (StatusCode, String)> {
    let spawner = state.spawner.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Spawner not available".to_string(),
    ))?;

    spawner
        .answer_question(&req.question_id, &req.answer)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(AnswerQuestionResponse {
        status: "answered".to_string(),
    }))
}

#[derive(Deserialize)]
pub struct TerminateSessionRequest {
    pub session_id: String,
}

#[derive(Serialize)]
pub struct TerminateSessionResponse {
    pub exit_code: i32,
    pub status: String,
}

/// Terminate a Claude Code session
pub async fn terminate_session_handler(
    State(state): State<AppState>,
    Json(req): Json<TerminateSessionRequest>,
) -> Result<Json<TerminateSessionResponse>, (StatusCode, String)> {
    let spawner = state.spawner.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Spawner not available".to_string(),
    ))?;

    let exit_code = spawner
        .terminate(&req.session_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let status = if exit_code == 0 { "completed" } else { "failed" };

    Ok(Json(TerminateSessionResponse {
        exit_code,
        status: status.to_string(),
    }))
}

/// SSE stream for Claude Code session events (all sessions)
pub async fn session_events_handler(
    State(state): State<AppState>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, (StatusCode, String)> {
    let spawner = state.spawner.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Spawner not available".to_string(),
    ))?;

    let mut rx = spawner.subscribe();

    let stream = async_stream::stream! {
        loop {
            tokio::select! {
                result = rx.recv() => {
                    match result {
                        Ok(event) => {
                            let data = serde_json::to_string(&event).unwrap_or_default();
                            yield Ok(Event::default().data(data));
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            // Missed some events, log and continue
                            tracing::warn!("Session event stream lagged by {} events", n);
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                            // Channel closed, end stream
                            break;
                        }
                    }
                }
                _ = tokio::time::sleep(Duration::from_secs(30)) => {
                    // Send heartbeat
                    let heartbeat = serde_json::json!({"type": "heartbeat", "ts": chrono::Utc::now().timestamp()});
                    yield Ok(Event::default().data(heartbeat.to_string()));
                }
            }
        }
    };

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

/// SSE stream for a specific Claude Code session
/// Filters events to only show those for the requested session ID
/// The Studio "Big Screen" uses this to display live ReasoningDelta and ToolUse events
pub async fn session_stream_handler(
    State(state): State<AppState>,
    axum::extract::Path(session_id): axum::extract::Path<String>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, (StatusCode, String)> {
    use crate::spawner::types::SessionEvent;

    let spawner = state.spawner.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Spawner not available".to_string(),
    ))?;

    let mut rx = spawner.subscribe();

    // Get session info from database for initial state
    let db = state.db.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Database not available".to_string(),
    ))?;

    // Fetch current session status
    let initial_status: Option<(String, Option<i64>, Option<String>)> = sqlx::query_as(
        "SELECT status, last_heartbeat, summary FROM claude_sessions WHERE id = $1"
    )
    .bind(&session_id)
    .fetch_optional(db)
    .await
    .ok()
    .flatten();

    let target_session = session_id.clone();

    let stream = async_stream::stream! {
        // Send initial status event
        if let Some((status, last_heartbeat, summary)) = initial_status {
            let init_event = serde_json::json!({
                "type": "init",
                "session_id": target_session,
                "status": status,
                "last_heartbeat": last_heartbeat,
                "summary": summary
            });
            yield Ok(Event::default().data(init_event.to_string()));
        } else {
            let not_found = serde_json::json!({
                "type": "error",
                "message": "Session not found",
                "session_id": target_session
            });
            yield Ok(Event::default().data(not_found.to_string()));
            return;
        }

        loop {
            tokio::select! {
                result = rx.recv() => {
                    match result {
                        Ok(event) => {
                            // Filter to only events for this session
                            let matches = match &event {
                                SessionEvent::Started { session_id, .. } => session_id == &target_session,
                                SessionEvent::StatusChanged { session_id, .. } => session_id == &target_session,
                                SessionEvent::Output { session_id, .. } => session_id == &target_session,
                                SessionEvent::ToolCall { session_id, .. } => session_id == &target_session,
                                SessionEvent::QuestionPending { session_id, .. } => session_id == &target_session,
                                SessionEvent::Ended { session_id, .. } => session_id == &target_session,
                                SessionEvent::Heartbeat { .. } => true, // Always pass through heartbeats
                            };

                            if matches {
                                let data = serde_json::to_string(&event).unwrap_or_default();
                                yield Ok(Event::default().data(data));

                                // End stream if session ended
                                if matches!(event, SessionEvent::Ended { .. }) {
                                    break;
                                }
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            tracing::warn!(session_id = %target_session, lagged = n, "Session stream lagged");
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                            break;
                        }
                    }
                }
                _ = tokio::time::sleep(Duration::from_secs(30)) => {
                    // Send heartbeat
                    let heartbeat = serde_json::json!({
                        "type": "heartbeat",
                        "session_id": target_session,
                        "ts": chrono::Utc::now().timestamp()
                    });
                    yield Ok(Event::default().data(heartbeat.to_string()));
                }
            }
        }
    };

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}
