// src/web/chat.rs
// Chat API handlers (DeepSeek Reasoner)

use axum::{
    extract::State,
    Json,
};
use mira_types::{ChatRequest, ChatUsage, WsEvent};
use std::time::Instant;
use tracing::{debug, error, info, instrument, warn};

use crate::persona;
use crate::web::deepseek::{self, Message, mira_tools};
use crate::web::state::AppState;

/// Chat with DeepSeek Reasoner
pub async fn chat(
    State(state): State<AppState>,
    Json(req): Json<ChatRequest>,
) -> Json<mira_types::ApiResponse<serde_json::Value>> {
    let deepseek = match &state.deepseek {
        Some(ds) => ds,
        None => {
            return Json(mira_types::ApiResponse::err(
                "DeepSeek not configured. Set DEEPSEEK_API_KEY environment variable.",
            ))
        }
    };

    // Broadcast chat start
    state.broadcast(WsEvent::ChatStart {
        message: req.message.clone(),
    });

    // Store user message in history
    if let Err(e) = state.db.store_chat_message("user", &req.message, None) {
        warn!("Failed to store user message: {}", e);
    }

    // Build messages with system prompt (includes personal context based on user message)
    let mut messages = vec![Message::system(build_system_prompt(&state, &req.message).await)];

    // Add stored conversation history (recent messages from DB)
    // This gives continuity across page refreshes / sessions
    if req.history.is_empty() {
        // No client-side history, load from DB
        if let Ok(recent) = state.db.get_recent_messages(20) {
            for msg in recent {
                messages.push(Message {
                    role: msg.role,
                    content: Some(msg.content),
                    reasoning_content: msg.reasoning_content,
                    tool_calls: None,
                    tool_call_id: None,
                });
            }
        }
    } else {
        // Use client-provided history
        for msg in &req.history {
            let role = match msg.role {
                mira_types::ChatRole::System => "system",
                mira_types::ChatRole::User => "user",
                mira_types::ChatRole::Assistant => "assistant",
                mira_types::ChatRole::Tool => "tool",
            };
            messages.push(Message {
                role: role.to_string(),
                content: msg.content.clone(),
                reasoning_content: msg.reasoning_content.clone(),
                tool_calls: None,
                tool_call_id: msg.tool_call_id.clone(),
            });
        }
    }

    // Add user message
    messages.push(Message::user(&req.message));

    // Get tools
    let tools = mira_tools();

    // Call DeepSeek
    match deepseek.chat(messages, Some(tools)).await {
        Ok(result) => {
            // Handle tool calls if present
            if let Some(tool_calls) = &result.tool_calls {
                // Execute tools and continue conversation
                let _tool_results = execute_tools(&state, tool_calls).await;

                // For now, return the partial result - full tool loop TBD
                info!("Chat completed with {} tool calls", tool_calls.len());
            }

            // Broadcast completion
            let usage = result.usage.map(|u| ChatUsage {
                prompt_tokens: u.prompt_tokens,
                completion_tokens: u.completion_tokens,
                total_tokens: u.total_tokens,
                cache_hit_tokens: u.prompt_cache_hit_tokens,
                cache_miss_tokens: u.prompt_cache_miss_tokens,
            });

            let response_content = result.content.clone().unwrap_or_default();

            state.broadcast(WsEvent::ChatComplete {
                content: response_content.clone(),
                model: "deepseek-reasoner".to_string(),
                usage,
            });

            // Store assistant response in history
            // Use content if available, fall back to reasoning_content
            let assistant_content = if !response_content.is_empty() {
                response_content.clone()
            } else {
                result.reasoning_content.clone().unwrap_or_default()
            };
            if !assistant_content.is_empty() {
                if let Err(e) = state.db.store_chat_message(
                    "assistant",
                    &assistant_content,
                    result.reasoning_content.as_deref(),
                ) {
                    warn!("Failed to store assistant message: {}", e);
                }

                // Spawn background tasks (non-blocking)
                spawn_fact_extraction(
                    state.clone(),
                    req.message.clone(),
                    assistant_content,
                );

                // Check if we need to roll up older messages into summaries
                maybe_spawn_summarization(state.clone());
            }

            Json(mira_types::ApiResponse::ok(serde_json::json!({
                "content": result.content,
                "reasoning_content": result.reasoning_content,
                "tool_calls": result.tool_calls,
            })))
        }
        Err(e) => {
            state.broadcast(WsEvent::ChatError {
                message: e.to_string(),
            });
            Json(mira_types::ApiResponse::err(e.to_string()))
        }
    }
}

/// Test chat endpoint - returns detailed JSON for debugging
/// Used by `mira test-chat` CLI and for programmatic testing
#[instrument(skip(state, req), fields(message_len = req.message.len()))]
pub async fn test_chat(
    State(state): State<AppState>,
    Json(req): Json<ChatRequest>,
) -> Json<mira_types::ApiResponse<serde_json::Value>> {
    let start_time = Instant::now();

    let deepseek = match &state.deepseek {
        Some(ds) => ds,
        None => {
            return Json(mira_types::ApiResponse::err(
                "DeepSeek not configured. Set DEEPSEEK_API_KEY environment variable.",
            ))
        }
    };

    info!(message = %req.message, "Test chat request received");

    // Store user message in history
    if let Err(e) = state.db.store_chat_message("user", &req.message, None) {
        warn!("Failed to store user message: {}", e);
    }

    // Build messages (includes personal context based on user message)
    let mut messages = vec![Message::system(build_system_prompt(&state, &req.message).await)];

    // Add stored conversation history (recent messages from DB)
    if req.history.is_empty() {
        if let Ok(recent) = state.db.get_recent_messages(20) {
            for msg in recent {
                messages.push(Message {
                    role: msg.role,
                    content: Some(msg.content),
                    reasoning_content: msg.reasoning_content,
                    tool_calls: None,
                    tool_call_id: None,
                });
            }
        }
    } else {
        for msg in &req.history {
            let role = match msg.role {
                mira_types::ChatRole::System => "system",
                mira_types::ChatRole::User => "user",
                mira_types::ChatRole::Assistant => "assistant",
                mira_types::ChatRole::Tool => "tool",
            };
            messages.push(Message {
                role: role.to_string(),
                content: msg.content.clone(),
                reasoning_content: msg.reasoning_content.clone(),
                tool_calls: None,
                tool_call_id: msg.tool_call_id.clone(),
            });
        }
    }

    messages.push(Message::user(&req.message));

    let tools = mira_tools();
    let tool_names: Vec<String> = tools.iter().map(|t| t.function.name.clone()).collect();

    // Call DeepSeek
    match deepseek.chat(messages, Some(tools)).await {
        Ok(result) => {
            let duration_ms = start_time.elapsed().as_millis() as u64;

            // Execute tools if requested
            let mut tool_results = Vec::new();
            if let Some(ref tool_calls) = result.tool_calls {
                let results = execute_tools(&state, tool_calls).await;
                for (id, res) in results {
                    tool_results.push(serde_json::json!({
                        "call_id": id,
                        "result": res,
                    }));
                }
            }

            let response = serde_json::json!({
                "success": true,
                "request_id": result.request_id,
                "duration_ms": duration_ms,
                "deepseek_duration_ms": result.duration_ms,
                "content": result.content,
                "reasoning_content": result.reasoning_content,
                "tool_calls": result.tool_calls,
                "tool_results": tool_results,
                "usage": result.usage.map(|u| serde_json::json!({
                    "prompt_tokens": u.prompt_tokens,
                    "completion_tokens": u.completion_tokens,
                    "total_tokens": u.total_tokens,
                    "cache_hit_tokens": u.prompt_cache_hit_tokens,
                    "cache_miss_tokens": u.prompt_cache_miss_tokens,
                })),
                "available_tools": tool_names,
            });

            info!(
                request_id = %result.request_id,
                duration_ms = duration_ms,
                "Test chat complete"
            );

            // Store assistant response and spawn background tasks
            // Use content if available, fall back to reasoning_content (reasoner quirk)
            let assistant_content = result.content.clone()
                .or_else(|| result.reasoning_content.clone())
                .unwrap_or_default();
            if !assistant_content.is_empty() {
                if let Err(e) = state.db.store_chat_message(
                    "assistant",
                    &assistant_content,
                    result.reasoning_content.as_deref(),
                ) {
                    warn!("Failed to store assistant message: {}", e);
                }

                // Spawn background tasks (non-blocking)
                spawn_fact_extraction(
                    state.clone(),
                    req.message.clone(),
                    assistant_content,
                );

                // Check if we need to roll up older messages into summaries
                maybe_spawn_summarization(state.clone());
            }

            Json(mira_types::ApiResponse::ok(response))
        }
        Err(e) => {
            error!(error = %e, "Test chat failed");
            Json(mira_types::ApiResponse::err(e.to_string()))
        }
    }
}

/// Build system prompt with persona overlays and personal context
/// KV-cache optimized ordering (static → semi-static → dynamic → volatile)
/// Layers: base persona -> capabilities -> profile -> project -> summaries -> semantic recall
async fn build_system_prompt(state: &AppState, user_message: &str) -> String {
    let project_id = state.project_id().await;
    let session_persona = state.get_session_persona().await;

    // Get base persona stack (includes persona, project context, session, capabilities)
    let mut prompt = persona::build_system_prompt_with_persona(
        &state.db,
        project_id,
        session_persona.as_deref(),
    );

    // Add conversation summaries (semi-dynamic, changes less frequently)
    let summary_context = get_summary_context(&state.db, 5);
    if !summary_context.is_empty() {
        prompt.push_str(&format!("\n\n=== CONVERSATION HISTORY ===\n{}", summary_context));
    }

    // Inject personal context (global memories) - most dynamic part of system prompt
    let personal_context = build_personal_context(state, user_message).await;
    if !personal_context.is_empty() {
        prompt.push_str(&format!("\n\n=== ABOUT THE USER ===\n{}", personal_context));
    }

    prompt
}

/// Build personal context from global memories
/// Combines user profile (always present) with semantic recall based on current message
async fn build_personal_context(state: &AppState, user_message: &str) -> String {
    let mut context_parts = Vec::new();

    // 1. Get user profile (core facts - always included)
    if let Ok(profile) = state.db.get_user_profile() {
        if !profile.is_empty() {
            let profile_text: Vec<String> = profile
                .iter()
                .map(|m| format!("- {}", m.content))
                .collect();
            context_parts.push(format!("Profile:\n{}", profile_text.join("\n")));
        }
    }

    // 2. Semantic recall based on current message (if embeddings available)
    if let Some(ref embeddings) = state.embeddings {
        if let Ok(query_embedding) = embeddings.embed(user_message).await {
            if let Ok(memories) = state.db.recall_global_semantic(&query_embedding, 5) {
                if !memories.is_empty() {
                    let relevant: Vec<String> = memories
                        .iter()
                        .filter(|(_, _, distance)| *distance < 0.5) // Only include similar
                        .map(|(_, content, _)| format!("- {}", content))
                        .collect();
                    if !relevant.is_empty() {
                        context_parts.push(format!("Relevant context:\n{}", relevant.join("\n")));
                    }
                }
            }
        }
    }

    context_parts.join("\n\n")
}

/// Claude Code usage guide - injected when spawn_claude is first used
/// Provides DeepSeek with expert knowledge on how to use the Claude instance effectively
const CLAUDE_CODE_GUIDE: &str = r#"## Claude Code Instance Guide (v2.0.76)

You now have a Claude Code instance running. Use `send_to_claude` with this instance_id for follow-up.

### What Claude Code Can Do
- **Read/Write/Edit files** with surgical precision (AST-aware)
- **Run terminal commands** (bash, git, npm, cargo, etc.)
- **Multi-file changes** atomically coordinated
- **Web search/fetch** for documentation lookups

### Effective Follow-ups via send_to_claude
Be specific in your messages:
- "Run the tests and fix any failures"
- "Commit the changes with message 'feat: add X'"
- "Also update the related tests in tests/unit/"
- "Show me the git diff of your changes"

### Claude's Available Tools
- `Read`, `Write`, `Edit`, `Glob`, `Grep` - file operations
- `Bash` - terminal commands (supports background execution)
- `WebFetch`, `WebSearch` - web access
- `Task` - spawn subagents for parallel work

### Tips
- Claude maintains full conversation context
- Output streams to UI in real-time
- Instance persists until killed or task complete
- Multiple instances can run in parallel
"#;

/// Execute tool calls and return results
#[instrument(skip(state, tool_calls), fields(tool_count = tool_calls.len()))]
pub(crate) async fn execute_tools(
    state: &AppState,
    tool_calls: &[deepseek::ToolCall],
) -> Vec<(String, String)> {
    let mut results = Vec::new();

    for tc in tool_calls {
        let start_time = Instant::now();
        let args: serde_json::Value =
            serde_json::from_str(&tc.function.arguments).unwrap_or(serde_json::Value::Null);

        debug!(
            tool = %tc.function.name,
            call_id = %tc.id,
            args = %args,
            "Executing tool"
        );

        let result = match tc.function.name.as_str() {
            "recall_memories" => {
                let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("");
                let limit = args.get("limit").and_then(|v| v.as_i64()).unwrap_or(5);

                match execute_recall(state, query, limit).await {
                    Ok(r) => r,
                    Err(e) => format!("Error: {}", e),
                }
            }
            "search_code" => {
                let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("");
                let limit = args.get("limit").and_then(|v| v.as_i64()).unwrap_or(10);

                match execute_code_search(state, query, limit).await {
                    Ok(r) => r,
                    Err(e) => format!("Error: {}", e),
                }
            }
            "list_tasks" => {
                match execute_list_tasks(state).await {
                    Ok(r) => r,
                    Err(e) => format!("Error: {}", e),
                }
            }
            "list_goals" => {
                match execute_list_goals(state).await {
                    Ok(r) => r,
                    Err(e) => format!("Error: {}", e),
                }
            }
            "spawn_claude" => {
                let initial_prompt = args
                    .get("initial_prompt")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let working_dir = args
                    .get("working_directory")
                    .and_then(|v| v.as_str())
                    .map(String::from)
                    .or_else(|| {
                        // Use project path if available
                        futures::executor::block_on(state.get_project())
                            .map(|p| p.path)
                    })
                    .unwrap_or_else(|| ".".to_string());

                match state
                    .claude_manager
                    .spawn(working_dir, Some(initial_prompt.to_string()))
                    .await
                {
                    Ok(id) => format!(
                        "Claude instance started with ID: {}\n\n{}",
                        id, CLAUDE_CODE_GUIDE
                    ),
                    Err(e) => format!("Error spawning Claude: {}", e),
                }
            }
            "send_to_claude" => {
                let instance_id = args
                    .get("instance_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let message = args.get("message").and_then(|v| v.as_str()).unwrap_or("");

                match state.claude_manager.send_input(instance_id, message).await {
                    Ok(_) => "Message sent to Claude".to_string(),
                    Err(e) => format!("Error: {}", e),
                }
            }
            _ => {
                warn!(tool = %tc.function.name, "Unknown tool requested");
                format!("Unknown tool: {}", tc.function.name)
            }
        };

        let duration_ms = start_time.elapsed().as_millis() as u64;
        let success = !result.starts_with("Error");

        if success {
            info!(
                tool = %tc.function.name,
                call_id = %tc.id,
                duration_ms = duration_ms,
                result_len = result.len(),
                "Tool executed successfully"
            );
        } else {
            error!(
                tool = %tc.function.name,
                call_id = %tc.id,
                duration_ms = duration_ms,
                result = %result,
                "Tool execution failed"
            );
        }

        // Broadcast tool result
        state.broadcast(WsEvent::ToolResult {
            tool_name: tc.function.name.clone(),
            result: result.clone(),
            success,
            call_id: tc.id.clone(),
            duration_ms,
        });

        results.push((tc.id.clone(), result));
    }

    results
}

async fn execute_recall(state: &AppState, query: &str, limit: i64) -> anyhow::Result<String> {
    let project_id = state.project_id().await;
    let project = state.get_project().await;

    // Add project context header if project is set
    let context_header = match &project {
        Some(p) => format!(
            "[Project: {} @ {}]\n\n",
            p.name.as_deref().unwrap_or("Unknown"),
            p.path
        ),
        None => String::new(),
    };

    if let Some(ref embeddings) = state.embeddings {
        if let Ok(query_embedding) = embeddings.embed(query).await {
            let conn = state.db.conn();

            let embedding_bytes: Vec<u8> = query_embedding
                .iter()
                .flat_map(|f| f.to_le_bytes())
                .collect();

            let mut stmt = conn.prepare(
                "SELECT f.content FROM memory_facts f
                 JOIN vec_memory v ON f.id = v.fact_id
                 WHERE (f.project_id = ?1 OR ?1 IS NULL)
                 ORDER BY vec_distance_cosine(v.embedding, ?2)
                 LIMIT ?3",
            )?;

            let memories: Vec<String> = stmt
                .query_map(rusqlite::params![project_id, embedding_bytes, limit], |row| {
                    row.get(0)
                })?
                .filter_map(|r| r.ok())
                .collect();

            if !memories.is_empty() {
                return Ok(format!(
                    "{}Found {} memories:\n{}",
                    context_header,
                    memories.len(),
                    memories.join("\n---\n")
                ));
            }
        }
    }

    Ok(format!("{}No memories found", context_header))
}

async fn execute_code_search(state: &AppState, query: &str, limit: i64) -> anyhow::Result<String> {
    let project_id = state.project_id().await;
    let project = state.get_project().await;

    // Add project context header if project is set
    let context_header = match &project {
        Some(p) => format!(
            "[Project: {} @ {}]\n\n",
            p.name.as_deref().unwrap_or("Unknown"),
            p.path
        ),
        None => String::new(),
    };

    if let Some(ref embeddings) = state.embeddings {
        if let Ok(query_embedding) = embeddings.embed(query).await {
            let conn = state.db.conn();

            let embedding_bytes: Vec<u8> = query_embedding
                .iter()
                .flat_map(|f| f.to_le_bytes())
                .collect();

            let mut stmt = conn.prepare(
                "SELECT file_path, chunk_content FROM vec_code
                 WHERE project_id = ?1 OR ?1 IS NULL
                 ORDER BY vec_distance_cosine(embedding, ?2)
                 LIMIT ?3",
            )?;

            let results: Vec<String> = stmt
                .query_map(rusqlite::params![project_id, embedding_bytes, limit], |row| {
                    let path: String = row.get(0)?;
                    let content: String = row.get(1)?;
                    Ok(format!("## {}\n```\n{}\n```", path, content))
                })?
                .filter_map(|r| r.ok())
                .collect();

            if !results.is_empty() {
                return Ok(format!(
                    "{}Found {} code matches:\n{}",
                    context_header,
                    results.len(),
                    results.join("\n\n")
                ));
            }
        }
    }

    Ok(format!("{}No code matches found", context_header))
}

async fn execute_list_tasks(state: &AppState) -> anyhow::Result<String> {
    let project_id = state.project_id().await;
    let conn = state.db.conn();

    let mut stmt = conn.prepare(
        "SELECT title, status, priority FROM tasks
         WHERE project_id = ?1 OR ?1 IS NULL
         ORDER BY created_at DESC LIMIT 20",
    )?;

    let tasks: Vec<String> = stmt
        .query_map([project_id], |row| {
            let title: String = row.get(0)?;
            let status: String = row.get(1)?;
            let priority: String = row.get(2)?;
            Ok(format!("- [{}] {} ({})", status, title, priority))
        })?
        .filter_map(|r| r.ok())
        .collect();

    if tasks.is_empty() {
        Ok("No tasks found".to_string())
    } else {
        Ok(format!("Tasks:\n{}", tasks.join("\n")))
    }
}

async fn execute_list_goals(state: &AppState) -> anyhow::Result<String> {
    let project_id = state.project_id().await;
    let conn = state.db.conn();

    let mut stmt = conn.prepare(
        "SELECT title, status, progress_percent FROM goals
         WHERE project_id = ?1 OR ?1 IS NULL
         ORDER BY created_at DESC LIMIT 10",
    )?;

    let goals: Vec<String> = stmt
        .query_map([project_id], |row| {
            let title: String = row.get(0)?;
            let status: String = row.get(1)?;
            let progress: i32 = row.get(2)?;
            Ok(format!("- [{}] {} ({}%)", status, title, progress))
        })?
        .filter_map(|r| r.ok())
        .collect();

    if goals.is_empty() {
        Ok("No goals found".to_string())
    } else {
        Ok(format!("Goals:\n{}", goals.join("\n")))
    }
}

// ═══════════════════════════════════════
// FACT EXTRACTION (Background)
// ═══════════════════════════════════════

/// Prompt for extracting facts from conversation exchanges
const FACT_EXTRACTION_PROMPT: &str = r#"Extract any new facts about the user from this conversation exchange. Focus on:
- Personal details (name, family members, interests, life events)
- Preferences (communication style, likes/dislikes)
- Work context (role, projects, technology preferences)

Return ONLY a JSON array of facts. Each fact should have:
- "content": the fact itself (clear, concise statement)
- "category": one of "profile", "personal", "preferences", "work"
- "key": optional unique identifier for deduplication (e.g. "user_name", "daughter_name")

Example output:
[
  {"content": "User's name is Peter", "category": "profile", "key": "user_name"},
  {"content": "Has a daughter named Emma who is 5 years old", "category": "personal", "key": "daughter"}
]

If no new facts worth remembering, return: []

Respond with ONLY the JSON array, no other text."#;

/// Extract facts from a conversation exchange and store as global memories
/// Runs in background to not block the response
pub fn spawn_fact_extraction(
    state: AppState,
    user_message: String,
    assistant_response: String,
) {
    tokio::spawn(async move {
        if let Err(e) = extract_and_store_facts(&state, &user_message, &assistant_response).await {
            warn!("Fact extraction failed: {}", e);
        }
    });
}

/// Actually perform the extraction
async fn extract_and_store_facts(
    state: &AppState,
    user_message: &str,
    assistant_response: &str,
) -> anyhow::Result<()> {
    let deepseek = state.deepseek.as_ref()
        .ok_or_else(|| anyhow::anyhow!("DeepSeek not configured"))?;

    // Build extraction prompt
    let exchange = format!(
        "User: {}\n\nAssistant: {}",
        user_message,
        assistant_response
    );

    let messages = vec![
        Message::system(FACT_EXTRACTION_PROMPT.to_string()),
        Message::user(exchange),
    ];

    // Call DeepSeek (no tools needed for extraction)
    let result = deepseek.chat(messages, None).await?;

    let content = result.content
        .ok_or_else(|| anyhow::anyhow!("No content in extraction response"))?;

    // Parse JSON array of facts
    let facts: Vec<ExtractedFact> = match serde_json::from_str(&content) {
        Ok(f) => f,
        Err(e) => {
            debug!("Failed to parse extraction response as JSON: {} - content: {}", e, content);
            return Ok(()); // Not an error, just no facts extracted
        }
    };

    if facts.is_empty() {
        debug!("No facts extracted from exchange");
        return Ok(());
    }

    info!("Extracted {} facts from conversation", facts.len());

    // Store each fact as global memory
    for fact in facts {
        let id = state.db.store_global_memory(
            &fact.content,
            &fact.category,
            fact.key.as_deref(),
            Some(0.9), // Slightly lower confidence for auto-extracted facts
        )?;

        // Also store embedding if available
        if let Some(ref embeddings) = state.embeddings {
            if let Ok(embedding) = embeddings.embed(&fact.content).await {
                let conn = state.db.conn();
                let embedding_bytes: Vec<u8> = embedding
                    .iter()
                    .flat_map(|f| f.to_le_bytes())
                    .collect();

                let _ = conn.execute(
                    "INSERT OR REPLACE INTO vec_memory (rowid, embedding, fact_id, content) VALUES (?, ?, ?, ?)",
                    rusqlite::params![id, embedding_bytes, id, &fact.content],
                );
            }
        }

        debug!("Stored fact: {} (category: {}, key: {:?})", fact.content, fact.category, fact.key);
    }

    Ok(())
}

/// A fact extracted from conversation
#[derive(Debug, serde::Deserialize)]
struct ExtractedFact {
    content: String,
    category: String,
    key: Option<String>,
}

// ═══════════════════════════════════════
// ROLLING SUMMARIZATION
// ═══════════════════════════════════════

/// Configuration for rolling summaries
const SUMMARY_WINDOW_SIZE: usize = 20;  // Keep this many recent messages unsummarized
const SUMMARY_BATCH_SIZE: usize = 10;   // Summarize this many messages at a time
const SUMMARY_THRESHOLD: usize = 30;    // Trigger summarization when this many unsummarized

// Multi-level summary promotion thresholds
const L1_PROMOTION_THRESHOLD: usize = 10; // Promote L1→L2 when this many session summaries
const L2_PROMOTION_THRESHOLD: usize = 7;  // Promote L2→L3 when this many daily summaries
const L1_PROMOTION_BATCH: usize = 5;      // Combine this many L1 summaries into one L2
const L2_PROMOTION_BATCH: usize = 5;      // Combine this many L2 summaries into one L3

/// Prompt for summarizing conversation chunks (L1 - session)
const SUMMARIZATION_PROMPT: &str = r#"Summarize this conversation segment concisely. Focus on:
- Key topics discussed
- Decisions made or preferences expressed
- Important context for future conversations
- Any action items or follow-ups mentioned

Keep it brief (2-4 sentences) but preserve important details.
Write in third person (e.g., "User discussed...", "They decided...")

Respond with ONLY the summary text, no preamble."#;

/// Prompt for combining summaries into higher-level summaries (L2/L3)
const PROMOTION_PROMPT: &str = r#"Combine these conversation summaries into a single higher-level summary.
Focus on the most important themes, decisions, and context that would be valuable long-term.
Be concise (2-3 sentences) but preserve key information.

Respond with ONLY the combined summary text, no preamble."#;

/// Check if we need to summarize and spawn background task if so
pub fn maybe_spawn_summarization(state: AppState) {
    tokio::spawn(async move {
        // Check message count for L1 summarization
        let count = match state.db.count_unsummarized_messages() {
            Ok(c) => c as usize,
            Err(_) => return,
        };

        if count >= SUMMARY_THRESHOLD {
            info!("Triggering rolling summarization: {} unsummarized messages", count);
            if let Err(e) = perform_rolling_summarization(&state).await {
                warn!("Rolling summarization failed: {}", e);
            }
        }

        // Check for L1→L2 promotion
        let l1_count = state.db.count_summaries_at_level(1).unwrap_or(0) as usize;
        if l1_count >= L1_PROMOTION_THRESHOLD {
            info!("Triggering L1→L2 promotion: {} session summaries", l1_count);
            if let Err(e) = promote_summaries(&state, 1, 2, L1_PROMOTION_BATCH).await {
                warn!("L1→L2 promotion failed: {}", e);
            }
        }

        // Check for L2→L3 promotion
        let l2_count = state.db.count_summaries_at_level(2).unwrap_or(0) as usize;
        if l2_count >= L2_PROMOTION_THRESHOLD {
            info!("Triggering L2→L3 promotion: {} daily summaries", l2_count);
            if let Err(e) = promote_summaries(&state, 2, 3, L2_PROMOTION_BATCH).await {
                warn!("L2→L3 promotion failed: {}", e);
            }
        }
    });
}

/// Perform rolling summarization of older messages
async fn perform_rolling_summarization(state: &AppState) -> anyhow::Result<()> {
    let deepseek = state.deepseek.as_ref()
        .ok_or_else(|| anyhow::anyhow!("DeepSeek not configured"))?;

    // Get the oldest unsummarized messages (beyond our window)
    // First, get recent messages to find the cutoff point
    let recent = state.db.get_recent_messages(SUMMARY_WINDOW_SIZE)?;

    if recent.is_empty() {
        return Ok(());
    }

    // Get oldest message ID in our "keep" window
    let oldest_kept_id = recent.first().map(|m| m.id).unwrap_or(0);

    // Get messages before that for summarization
    let to_summarize = state.db.get_messages_before(oldest_kept_id, SUMMARY_BATCH_SIZE)?;

    if to_summarize.is_empty() {
        return Ok(());
    }

    let start_id = to_summarize.first().unwrap().id;
    let end_id = to_summarize.last().unwrap().id;

    info!(
        "Summarizing messages {} to {} ({} messages)",
        start_id, end_id, to_summarize.len()
    );

    // Format messages for summarization
    let conversation_text: String = to_summarize
        .iter()
        .map(|m| format!("{}: {}", m.role.to_uppercase(), m.content))
        .collect::<Vec<_>>()
        .join("\n\n");

    // Call DeepSeek to summarize
    let messages = vec![
        Message::system(SUMMARIZATION_PROMPT.to_string()),
        Message::user(conversation_text),
    ];

    let result = deepseek.chat(messages, None).await?;

    let summary = result.content
        .or(result.reasoning_content)
        .ok_or_else(|| anyhow::anyhow!("No summary generated"))?;

    // Store summary
    let summary_id = state.db.store_chat_summary(&summary, start_id, end_id, 1)?;
    info!("Stored summary {} covering messages {}-{}", summary_id, start_id, end_id);

    // Mark messages as summarized
    let marked = state.db.mark_messages_summarized(start_id, end_id)?;
    info!("Marked {} messages as summarized", marked);

    Ok(())
}

/// Promote summaries from one level to the next
async fn promote_summaries(
    state: &AppState,
    from_level: i32,
    to_level: i32,
    batch_size: usize,
) -> anyhow::Result<()> {
    let deepseek = state.deepseek.as_ref()
        .ok_or_else(|| anyhow::anyhow!("DeepSeek not configured"))?;

    // Get oldest summaries at the source level
    let summaries = state.db.get_oldest_summaries(from_level, batch_size)?;

    if summaries.is_empty() {
        return Ok(());
    }

    let ids: Vec<i64> = summaries.iter().map(|s| s.id).collect();
    let range_start = summaries.first().unwrap().message_range_start;
    let range_end = summaries.last().unwrap().message_range_end;

    info!(
        "Promoting {} L{} summaries to L{} (covering {}-{})",
        summaries.len(), from_level, to_level, range_start, range_end
    );

    // Combine summaries for the LLM
    let combined_text: String = summaries
        .iter()
        .map(|s| format!("- {}", s.summary))
        .collect::<Vec<_>>()
        .join("\n");

    // Call DeepSeek to create higher-level summary
    let messages = vec![
        Message::system(PROMOTION_PROMPT.to_string()),
        Message::user(combined_text),
    ];

    let result = deepseek.chat(messages, None).await?;

    let new_summary = result.content
        .or(result.reasoning_content)
        .ok_or_else(|| anyhow::anyhow!("No promoted summary generated"))?;

    // Store the new higher-level summary
    let new_id = state.db.store_chat_summary(&new_summary, range_start, range_end, to_level)?;
    info!("Created L{} summary {} from {} L{} summaries", to_level, new_id, ids.len(), from_level);

    // Delete the old summaries
    let deleted = state.db.delete_summaries(&ids)?;
    info!("Deleted {} promoted L{} summaries", deleted, from_level);

    Ok(())
}

/// Get recent summaries for context injection (all levels)
pub fn get_summary_context(db: &crate::db::Database, limit: usize) -> String {
    let mut parts = Vec::new();

    // L3 - Weekly summaries (oldest context, most compressed)
    if let Ok(summaries) = db.get_recent_summaries(3, 2) {
        if !summaries.is_empty() {
            parts.push("Long-term context:".to_string());
            for s in summaries {
                parts.push(format!("  - {}", s.summary));
            }
        }
    }

    // L2 - Daily summaries
    if let Ok(summaries) = db.get_recent_summaries(2, 3) {
        if !summaries.is_empty() {
            parts.push("Recent days:".to_string());
            for s in summaries {
                parts.push(format!("  - {}", s.summary));
            }
        }
    }

    // L1 - Session summaries (most recent compressed context)
    if let Ok(summaries) = db.get_recent_summaries(1, limit) {
        if !summaries.is_empty() {
            parts.push("Earlier today:".to_string());
            for s in summaries {
                parts.push(format!("  - {}", s.summary));
            }
        }
    }

    parts.join("\n")
}
