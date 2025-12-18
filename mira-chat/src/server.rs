//! HTTP server for Studio integration
//!
//! Exposes mira-chat functionality via REST/SSE endpoints:
//! - GET /api/status - Health check
//! - POST /api/chat/stream - SSE streaming chat
//! - GET /api/messages - Paginated message history

use anyhow::Result;
use axum::{
    extract::{Query, State},
    http::{header, Method, StatusCode},
    response::{
        sse::{Event, KeepAlive, Sse},
        Json,
    },
    routing::{get, post},
    Router,
};
use futures::stream::Stream;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::SqlitePool;
use std::{collections::HashMap, convert::Infallible, path::PathBuf, sync::Arc};
use tokio::sync::mpsc;
use tower_http::cors::{Any, CorsLayer};
use uuid::Uuid;

use crate::{
    context::{build_system_prompt, MiraContext},
    reasoning::classify,
    responses::{Client as GptClient, StreamEvent},
    semantic::SemanticSearch,
    session::SessionManager,
    tools::{get_tools, DiffInfo, ToolExecutor},
};

// ============================================================================
// SSE Event Types
// ============================================================================

/// Events sent to the frontend via SSE
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum ChatEvent {
    /// Streaming text from assistant
    #[serde(rename = "text_delta")]
    TextDelta { delta: String },

    /// Tool call started - show in UI immediately
    #[serde(rename = "tool_call_start")]
    ToolCallStart {
        call_id: String,
        name: String,
        arguments: Value,
    },

    /// Tool result (may include diff for file operations)
    #[serde(rename = "tool_call_result")]
    ToolCallResult {
        call_id: String,
        name: String,
        success: bool,
        output: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        diff: Option<DiffInfo>,
    },

    /// Reasoning summary (when effort > none)
    #[serde(rename = "reasoning")]
    Reasoning {
        effort: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        summary: Option<String>,
    },

    /// Token usage at end
    #[serde(rename = "usage")]
    Usage {
        input_tokens: u32,
        output_tokens: u32,
        reasoning_tokens: u32,
        cached_tokens: u32,
    },

    /// Stream complete
    #[serde(rename = "done")]
    Done,

    /// Error
    #[serde(rename = "error")]
    Error { message: String },
}


// ============================================================================
// Request/Response Types
// ============================================================================

/// Chat request from frontend
#[derive(Debug, Deserialize)]
pub struct ChatRequest {
    pub message: String,
    pub project_path: String,
    #[serde(default)]
    pub reasoning_effort: Option<String>,
}

/// Message in the endless chat
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub role: String,
    pub blocks: Vec<MessageBlock>,
    pub created_at: i64,
}

/// Message with optional usage info (for API response)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageWithUsage {
    pub id: String,
    pub role: String,
    pub blocks: Vec<MessageBlock>,
    pub created_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<UsageInfo>,
}

/// Token usage info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageInfo {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub reasoning_tokens: u32,
    pub cached_tokens: u32,
}

/// Block within a message
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum MessageBlock {
    #[serde(rename = "text")]
    Text { content: String },
    #[serde(rename = "tool_call")]
    ToolCall {
        call_id: String,
        name: String,
        arguments: Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        result: Option<ToolCallResult>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallResult {
    pub success: bool,
    pub output: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff: Option<DiffInfo>,
}

/// Pagination query params
#[derive(Debug, Deserialize)]
pub struct MessagesQuery {
    #[serde(default = "default_limit")]
    pub limit: i32,
    pub before: Option<i64>, // created_at timestamp for cursor pagination
}

fn default_limit() -> i32 {
    50
}

// ============================================================================
// Server State
// ============================================================================

#[derive(Clone)]
pub struct AppState {
    pub db: Option<SqlitePool>,
    pub semantic: Arc<SemanticSearch>,
    pub api_key: String,
    pub default_reasoning_effort: String,
}

// ============================================================================
// Routes
// ============================================================================

/// Create the router with all endpoints
pub fn create_router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION]);

    Router::new()
        .route("/api/status", get(status_handler))
        .route("/api/chat/stream", post(chat_stream_handler))
        .route("/api/messages", get(messages_handler))
        .layer(cors)
        .with_state(state)
}

/// Run the HTTP server
pub async fn run(
    port: u16,
    api_key: String,
    db: Option<SqlitePool>,
    semantic: Arc<SemanticSearch>,
    reasoning_effort: String,
) -> Result<()> {
    let state = AppState {
        db,
        semantic,
        api_key,
        default_reasoning_effort: reasoning_effort,
    };

    let app = create_router(state);
    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));

    println!("Server listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

// ============================================================================
// Handlers
// ============================================================================

async fn status_handler(State(state): State<AppState>) -> Json<Value> {
    Json(json!({
        "status": "ok",
        "semantic_search": state.semantic.is_available(),
        "database": state.db.is_some(),
        "model": "gpt-5.2",
        "default_reasoning_effort": state.default_reasoning_effort,
    }))
}

async fn messages_handler(
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

async fn chat_stream_handler(
    State(state): State<AppState>,
    Json(request): Json<ChatRequest>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let (tx, rx) = mpsc::channel::<ChatEvent>(100);

    // Spawn the chat processing task
    tokio::spawn(async move {
        if let Err(e) = process_chat(state, request, tx.clone()).await {
            let _ = tx
                .send(ChatEvent::Error {
                    message: e.to_string(),
                })
                .await;
        }
        let _ = tx.send(ChatEvent::Done).await;
    });

    // Convert channel to SSE stream
    let stream = async_stream::stream! {
        let mut rx = rx;
        while let Some(event) = rx.recv().await {
            let data = serde_json::to_string(&event).unwrap_or_default();
            yield Ok(Event::default().data(data));
        }
    };

    Sse::new(stream).keep_alive(KeepAlive::default())
}

// ============================================================================
// Chat Processing
// ============================================================================

async fn process_chat(
    state: AppState,
    request: ChatRequest,
    tx: mpsc::Sender<ChatEvent>,
) -> Result<()> {
    let project_path = PathBuf::from(&request.project_path);

    // Always gpt-5.2, effort based on task complexity
    const MODEL: &str = "gpt-5.2";
    let effort = classify(&request.message);
    let reasoning_effort = request
        .reasoning_effort
        .unwrap_or_else(|| effort.effort_for_model().to_string());

    // Tool continuations: no reasoning needed
    const CONTINUATION_EFFORT: &str = "none";

    // Save user message
    let user_msg_id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();
    let user_blocks = vec![MessageBlock::Text {
        content: request.message.clone(),
    }];

    if let Some(db) = &state.db {
        let _ = sqlx::query(
            r#"
            INSERT INTO chat_messages (id, role, blocks, created_at)
            VALUES ($1, 'user', $2, $3)
            "#,
        )
        .bind(&user_msg_id)
        .bind(serde_json::to_string(&user_blocks)?)
        .bind(now)
        .execute(db)
        .await;
    }

    // Create session manager for full context assembly
    let session = if let Some(db) = &state.db {
        match SessionManager::new(db.clone(), state.semantic.clone(), request.project_path.clone()).await {
            Ok(s) => Some(Arc::new(s)),
            Err(e) => {
                tracing::warn!("Failed to create session manager: {}", e);
                None
            }
        }
    } else {
        None
    };

    // Assemble system prompt.
    // CHEAP MODE: until token usage is under control, we do NOT inject the
    // full assembled context blob (summaries/semantic/code index/recent msgs).
    // We rely on server-side continuity via previous_response_id.
    // Keep only persona + guidelines + small Mira context.
    let base_prompt = if let Some(db) = &state.db {
        let context = MiraContext::load(db, &request.project_path)
            .await
            .unwrap_or_default();
        build_system_prompt(&context)
    } else {
        build_system_prompt(&MiraContext::default())
    };

    // Check for handoff context (after a smooth reset)
    let handoff = if let Some(ref session) = session {
        match session.consume_handoff().await {
            Ok(h) => h,
            Err(e) => {
                tracing::warn!("Failed to consume handoff (continuity may be lost): {}", e);
                None
            }
        }
    } else {
        None
    };

    // If we have a handoff, append it to the system prompt for this turn only
    let system_prompt = if let Some(ref handoff_blob) = handoff {
        tracing::info!("Applying handoff context for smooth continuity");
        format!("{}\n\n{}", base_prompt, handoff_blob)
    } else {
        base_prompt
    };

    // Create GPT client
    let client = GptClient::new(state.api_key.clone());
    let tools = get_tools();

    // Create tool executor with session for file tracking
    let mut executor = ToolExecutor::new();
    executor.cwd = project_path.clone();
    if let Some(db) = &state.db {
        executor = executor.with_db(db.clone());
    }
    executor = executor.with_semantic(state.semantic.clone());
    if let Some(ref s) = session {
        executor = executor.with_session(s.clone());
    }

    // Get previous response ID for continuity from session
    // Note: if handoff was consumed, this should be None (starting fresh)
    let previous_response_id = if let Some(ref session) = session {
        session.get_response_id().await.unwrap_or(None)
    } else {
        get_last_response_id(&state.db).await
    };

    // Agentic loop
    let mut response_id: Option<String> = None;
    let mut assistant_blocks: Vec<MessageBlock> = Vec::new();
    let mut accumulated_text = String::new();
    // Accumulate usage across all iterations
    let mut total_input_tokens: u32 = 0;
    let mut total_output_tokens: u32 = 0;
    let mut total_reasoning_tokens: u32 = 0;
    let mut total_cached_tokens: u32 = 0;

    for iteration in 0..10 {
        // Stream the response
        let mut rx = if iteration == 0 {
            client
                .create_stream(
                    &request.message,
                    &system_prompt,
                    previous_response_id.as_deref(),
                    &reasoning_effort,
                    MODEL,
                    &tools,
                )
                .await?
        } else {
            // Continue with tool results - same model, low reasoning
            let tool_results: Vec<(String, String)> = assistant_blocks
                .iter()
                .filter_map(|b| match b {
                    MessageBlock::ToolCall {
                        call_id, result, ..
                    } => result.as_ref().map(|r| (call_id.clone(), r.output.clone())),
                    _ => None,
                })
                .collect();

            client
                .continue_with_tool_results_stream(
                    response_id.as_ref().unwrap(),
                    tool_results,
                    &system_prompt,
                    CONTINUATION_EFFORT,
                    MODEL,
                    &tools,
                )
                .await?
        };

        // Collect function calls from this iteration
        let mut pending_calls: HashMap<String, (String, String)> = HashMap::new(); // call_id -> (name, args)
        let mut has_function_calls = false;

        while let Some(event) = rx.recv().await {
            match event {
                StreamEvent::TextDelta(delta) => {
                    accumulated_text.push_str(&delta);
                    tx.send(ChatEvent::TextDelta { delta }).await?;
                }
                StreamEvent::FunctionCallStart { name, call_id } => {
                    has_function_calls = true;
                    pending_calls.insert(call_id.clone(), (name.clone(), String::new()));
                    tx.send(ChatEvent::ToolCallStart {
                        call_id,
                        name,
                        arguments: json!({}),
                    })
                    .await?;
                }
                StreamEvent::FunctionCallDelta {
                    call_id,
                    arguments_delta,
                } => {
                    if let Some((_, args)) = pending_calls.get_mut(&call_id) {
                        args.push_str(&arguments_delta);
                    }
                }
                StreamEvent::FunctionCallDone {
                    name,
                    call_id,
                    arguments,
                } => {
                    // Execute the tool with rich result (includes diff for file ops)
                    let rich_result = executor.execute_rich(&name, &arguments).await;
                    let (success, output, diff) = match rich_result {
                        Ok(r) => (r.success, r.output, r.diff),
                        Err(e) => (false, e.to_string(), None),
                    };

                    let tool_result = ToolCallResult {
                        success,
                        output: output.clone(),
                        diff: diff.clone(),
                    };

                    // Parse arguments for storage
                    let args_value: Value =
                        serde_json::from_str(&arguments).unwrap_or(json!({}));

                    // Add to blocks
                    assistant_blocks.push(MessageBlock::ToolCall {
                        call_id: call_id.clone(),
                        name: name.clone(),
                        arguments: args_value.clone(),
                        result: Some(tool_result),
                    });

                    // Send result event
                    tx.send(ChatEvent::ToolCallResult {
                        call_id,
                        name,
                        success,
                        output,
                        diff,
                    })
                    .await?;
                }
                StreamEvent::Done(response) => {
                    response_id = Some(response.id.clone());

                    // Accumulate and send usage
                    if let Some(usage) = response.usage {
                        total_input_tokens += usage.input_tokens;
                        total_output_tokens += usage.output_tokens;
                        total_reasoning_tokens += usage.reasoning_tokens();
                        total_cached_tokens += usage.cached_tokens();
                        tx.send(ChatEvent::Usage {
                            input_tokens: usage.input_tokens,
                            output_tokens: usage.output_tokens,
                            reasoning_tokens: usage.reasoning_tokens(),
                            cached_tokens: usage.cached_tokens(),
                        })
                        .await?;
                    }
                }
                StreamEvent::Error(e) => {
                    tx.send(ChatEvent::Error { message: e }).await?;
                    break;
                }
            }
        }

        // If there were no function calls, we're done
        if !has_function_calls {
            break;
        }
    }

    // Add accumulated text as a block
    if !accumulated_text.is_empty() {
        assistant_blocks.insert(
            0,
            MessageBlock::Text {
                content: accumulated_text,
            },
        );
    }

    // Save assistant message
    let assistant_msg_id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();

    if let Some(db) = &state.db {
        let _ = sqlx::query(
            r#"
            INSERT INTO chat_messages (id, role, blocks, created_at)
            VALUES ($1, 'assistant', $2, $3)
            "#,
        )
        .bind(&assistant_msg_id)
        .bind(serde_json::to_string(&assistant_blocks)?)
        .bind(now)
        .execute(db)
        .await;

        // Store token usage for this message
        if total_input_tokens > 0 || total_output_tokens > 0 {
            let usage_id = Uuid::new_v4().to_string();
            let _ = sqlx::query(
                r#"
                INSERT INTO chat_usage (id, message_id, input_tokens, output_tokens, reasoning_tokens, cached_tokens, model, reasoning_effort, created_at)
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
                "#,
            )
            .bind(&usage_id)
            .bind(&assistant_msg_id)
            .bind(total_input_tokens as i32)
            .bind(total_output_tokens as i32)
            .bind(total_reasoning_tokens as i32)
            .bind(total_cached_tokens as i32)
            .bind(MODEL)
            .bind(&reasoning_effort)
            .bind(now)
            .execute(db)
            .await;
        }

        // Save response ID for next request (prefer session, fallback to direct)
        if let Some(rid) = &response_id {
            if let Some(ref session) = session {
                let _ = session.set_response_id(rid).await;
            } else {
                let _ = sqlx::query(
                    r#"
                    INSERT OR REPLACE INTO chat_state (key, value)
                    VALUES ('last_response_id', $1)
                    "#,
                )
                .bind(rid)
                .execute(db)
                .await;
            }
        }

        // SMOOTH RESET: If input tokens exceeded threshold, prepare handoff for next turn
        // This prevents runaway token accumulation while preserving continuity
        const AUTO_RESET_THRESHOLD: u32 = 100_000;
        if total_input_tokens > AUTO_RESET_THRESHOLD {
            tracing::info!(
                "Preparing smooth reset: {} tokens exceeded {}k limit",
                total_input_tokens, AUTO_RESET_THRESHOLD / 1000
            );
            if let Some(ref session) = session {
                let _ = session.clear_response_id_with_handoff().await;
            } else {
                // Fallback for non-session mode: hard reset
                let _ = sqlx::query("DELETE FROM chat_state WHERE key = 'last_response_id'")
                    .execute(db)
                    .await;
            }
        }
    }

    Ok(())
}

async fn get_last_response_id(db: &Option<SqlitePool>) -> Option<String> {
    let Some(db) = db else {
        return None;
    };

    sqlx::query_scalar::<_, String>(
        r#"SELECT value FROM chat_state WHERE key = 'last_response_id'"#,
    )
    .fetch_optional(db)
    .await
    .ok()
    .flatten()
}


