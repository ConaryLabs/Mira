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
    context::MiraContext,
    reasoning::classify,
    responses::{Client as GptClient, StreamEvent},
    semantic::SemanticSearch,
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
    }))
}

async fn messages_handler(
    State(state): State<AppState>,
    Query(params): Query<MessagesQuery>,
) -> Result<Json<Vec<Message>>, (StatusCode, String)> {
    let Some(db) = &state.db else {
        return Ok(Json(vec![]));
    };

    let messages: Vec<(String, String, String, i64)> = if let Some(before) = params.before {
        sqlx::query_as(
            r#"
            SELECT id, role, blocks, created_at
            FROM chat_messages
            WHERE created_at < $1
            ORDER BY created_at DESC
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
            SELECT id, role, blocks, created_at
            FROM chat_messages
            ORDER BY created_at DESC
            LIMIT $1
            "#,
        )
        .bind(params.limit)
        .fetch_all(db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    };

    let result: Vec<Message> = messages
        .into_iter()
        .map(|(id, role, blocks_json, created_at)| {
            let blocks: Vec<MessageBlock> =
                serde_json::from_str(&blocks_json).unwrap_or_default();
            Message {
                id,
                role,
                blocks,
                created_at,
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

    // Classify task complexity for model routing
    let effort = classify(&request.message);
    let reasoning_effort = request
        .reasoning_effort
        .unwrap_or_else(|| effort.as_str().to_string());
    let model = effort.model();

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

    // Load Mira context for this project
    let context = if let Some(db) = &state.db {
        MiraContext::load(db, &request.project_path)
            .await
            .unwrap_or_default()
    } else {
        MiraContext::default()
    };

    // Build system prompt
    let system_prompt = build_system_prompt(&context, &request.project_path);

    // Create GPT client
    let client = GptClient::new(state.api_key.clone());
    let tools = get_tools();

    // Create tool executor
    let mut executor = ToolExecutor::new();
    executor.cwd = project_path.clone();
    if let Some(db) = &state.db {
        executor = executor.with_db(db.clone());
    }
    executor = executor.with_semantic(state.semantic.clone());

    // Get previous response ID for continuity
    let previous_response_id = get_last_response_id(&state.db).await;

    // Agentic loop
    let mut response_id: Option<String> = None;
    let mut assistant_blocks: Vec<MessageBlock> = Vec::new();
    let mut accumulated_text = String::new();

    for iteration in 0..10 {
        // Stream the response
        let mut rx = if iteration == 0 {
            client
                .create_stream(
                    &request.message,
                    &system_prompt,
                    previous_response_id.as_deref(),
                    &reasoning_effort,
                    model,
                    &tools,
                )
                .await?
        } else {
            // Continue with tool results
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
                    &reasoning_effort,
                    model,
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

                    // Send usage
                    if let Some(usage) = response.usage {
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

        // Save response ID for next request
        if let Some(rid) = &response_id {
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

fn build_system_prompt(context: &MiraContext, project_path: &str) -> String {
    let mut prompt = format!(
        r#"You are a helpful coding assistant. You are working in the project at: {}

You have access to tools for file operations, shell commands, web search, and memory.
Use tools to help the user with their coding tasks.
"#,
        project_path
    );

    // Add corrections
    if !context.corrections.is_empty() {
        prompt.push_str("\n## User Corrections (follow these):\n");
        for c in &context.corrections {
            prompt.push_str(&format!("- {}: {}\n", c.what_was_wrong, c.what_is_right));
        }
    }

    // Add active goals
    if !context.goals.is_empty() {
        prompt.push_str("\n## Active Goals:\n");
        for g in &context.goals {
            prompt.push_str(&format!("- {} ({})\n", g.title, g.status));
        }
    }

    // Add memories
    if !context.memories.is_empty() {
        prompt.push_str("\n## Remembered Context:\n");
        for m in &context.memories {
            prompt.push_str(&format!("- {}\n", m.content));
        }
    }

    prompt
}

