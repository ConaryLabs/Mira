// src/web/deepseek.rs
// DeepSeek API client for Reasoner (V3.2) with tool calling support

use anyhow::{anyhow, Result};
use futures::StreamExt;
use reqwest_eventsource::{Event, EventSource};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Instant;
use tracing::{debug, info, warn, error, instrument, Span};
use uuid::Uuid;

const DEEPSEEK_API_URL: &str = "https://api.deepseek.com/chat/completions";

/// Check if content looks like garbage (JSON fragments, brackets, etc.)
/// Returns true if content should be streamed to UI, false if it should be suppressed
fn is_streamable_content(content: &str) -> bool {
    let trimmed = content.trim();

    // Empty or whitespace-only
    if trimmed.is_empty() {
        return false;
    }

    // Single brackets/braces - likely tool call leakage
    if matches!(trimmed, "[" | "]" | "{" | "}" | "[]" | "{}") {
        return false;
    }

    // Just JSON punctuation
    if trimmed.chars().all(|c| matches!(c, '[' | ']' | '{' | '}' | ',' | ':' | '"' | ' ' | '\n' | '\t')) {
        return false;
    }

    // Short content starting with JSON structure - likely tool call JSON fragment
    if (trimmed.starts_with('[') || trimmed.starts_with('{')) && trimmed.len() < 100 {
        // But allow markdown lists that start with [
        if !trimmed.contains("](") {
            return false;
        }
    }

    true
}

// ═══════════════════════════════════════
// API TYPES
// ═══════════════════════════════════════

/// Message in a conversation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String, // "system" | "user" | "assistant" | "tool"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>, // Must preserve for multi-turn!
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>, // For tool responses
}

impl Message {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".into(),
            content: Some(content.into()),
            reasoning_content: None,
            tool_calls: None,
            tool_call_id: None,
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".into(),
            content: Some(content.into()),
            reasoning_content: None,
            tool_calls: None,
            tool_call_id: None,
        }
    }

    pub fn assistant(content: Option<String>, reasoning: Option<String>) -> Self {
        Self {
            role: "assistant".into(),
            content,
            reasoning_content: reasoning,
            tool_calls: None,
            tool_call_id: None,
        }
    }

    pub fn tool_result(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: "tool".into(),
            content: Some(content.into()),
            reasoning_content: None,
            tool_calls: None,
            tool_call_id: Some(tool_call_id.into()),
        }
    }
}

/// Tool call from the model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String, // "function"
    pub function: FunctionCall,
}

/// Function call details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String, // JSON string
}

/// Tool definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    #[serde(rename = "type")]
    pub tool_type: String, // "function"
    pub function: FunctionDef,
}

impl Tool {
    pub fn function(name: impl Into<String>, description: impl Into<String>, parameters: Value) -> Self {
        Self {
            tool_type: "function".into(),
            function: FunctionDef {
                name: name.into(),
                description: description.into(),
                parameters,
            },
        }
    }
}

/// Function definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDef {
    pub name: String,
    pub description: String,
    pub parameters: Value, // JSON Schema
}

/// Chat completion request
#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<Tool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<String>, // "auto" | "required" | "none"
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
}

/// Streaming chunk
#[derive(Debug, Deserialize)]
struct ChatChunk {
    choices: Vec<ChunkChoice>,
    #[serde(default)]
    usage: Option<Usage>,
}

#[derive(Debug, Deserialize)]
struct ChunkChoice {
    delta: ChunkDelta,
}

#[derive(Debug, Deserialize, Default)]
struct ChunkDelta {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    reasoning_content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<ToolCallChunk>>,
}

/// Partial tool call in streaming
#[derive(Debug, Deserialize)]
struct ToolCallChunk {
    index: usize,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    function: Option<FunctionChunk>,
}

#[derive(Debug, Deserialize, Default)]
struct FunctionChunk {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

/// Usage statistics
#[derive(Debug, Clone, Deserialize)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
    #[serde(default)]
    pub prompt_cache_hit_tokens: Option<u32>,
    #[serde(default)]
    pub prompt_cache_miss_tokens: Option<u32>,
}

// ═══════════════════════════════════════
// CLIENT
// ═══════════════════════════════════════

/// DeepSeek API client
pub struct DeepSeekClient {
    api_key: String,
    client: reqwest::Client,
}

/// Result of a chat completion
#[derive(Clone)]
pub struct ChatResult {
    pub request_id: String,
    pub content: Option<String>,
    pub reasoning_content: Option<String>,
    pub tool_calls: Option<Vec<ToolCall>>,
    pub usage: Option<Usage>,
    pub duration_ms: u64,
}

/// Non-streaming response for simple chat
#[derive(Debug, Deserialize)]
struct SimpleChatResponse {
    choices: Vec<SimpleChoice>,
    usage: Option<Usage>,
}

#[derive(Debug, Deserialize)]
struct SimpleChoice {
    message: SimpleMessage,
}

#[derive(Debug, Deserialize)]
struct SimpleMessage {
    content: Option<String>,
}

impl DeepSeekClient {
    /// Create a new DeepSeek client
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            client: reqwest::Client::new(),
        }
    }

    /// Simple non-streaming chat using deepseek-chat model (fast, cheap)
    /// Used for summarization, query generation, etc.
    pub async fn chat_simple(&self, system: &str, user: &str) -> Result<String> {
        let start_time = Instant::now();

        let request = serde_json::json!({
            "model": "deepseek-chat",
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": user}
            ],
            "stream": false,
            "max_tokens": 2048
        });

        debug!("DeepSeek chat_simple request");

        let response = self
            .client
            .post(DEEPSEEK_API_URL)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await
            .map_err(|e| anyhow!("Request failed: {}", e))?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("DeepSeek API error {}: {}", status, body));
        }

        let data: SimpleChatResponse = response
            .json()
            .await
            .map_err(|e| anyhow!("Failed to parse response: {}", e))?;

        let content = data
            .choices
            .first()
            .and_then(|c| c.message.content.clone())
            .unwrap_or_default();

        let duration_ms = start_time.elapsed().as_millis();
        info!(
            duration_ms = duration_ms,
            content_len = content.len(),
            "DeepSeek chat_simple complete"
        );

        Ok(content)
    }

    /// Chat with streaming, returns the complete response
    #[instrument(skip(self, messages, tools), fields(request_id, model = "deepseek-reasoner", message_count = messages.len()))]
    pub async fn chat(
        &self,
        messages: Vec<Message>,
        tools: Option<Vec<Tool>>,
    ) -> Result<ChatResult> {
        let request_id = Uuid::new_v4().to_string();
        let start_time = Instant::now();

        // Record request ID in span
        Span::current().record("request_id", &request_id);

        info!(
            request_id = %request_id,
            message_count = messages.len(),
            tool_count = tools.as_ref().map(|t| t.len()).unwrap_or(0),
            "Starting DeepSeek chat request"
        );

        let request = ChatRequest {
            model: "deepseek-reasoner".into(),
            messages,
            tools,
            tool_choice: Some("auto".into()),
            stream: true,
            max_tokens: Some(8192),
        };

        debug!(request_id = %request_id, "DeepSeek request: {:?}", serde_json::to_string(&request)?);

        let request_builder = self
            .client
            .post(DEEPSEEK_API_URL)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request);

        // Stream the response using EventSource
        let mut es = match EventSource::new(request_builder) {
            Ok(es) => es,
            Err(e) => {
                error!(request_id = %request_id, error = %e, "Failed to create EventSource");
                return Err(anyhow!("Failed to create EventSource: {}", e));
            }
        };

        debug!(request_id = %request_id, "DeepSeek stream opened");

        let mut full_content = String::new();
        let mut full_reasoning = String::new();
        let mut tool_calls: Vec<ToolCall> = Vec::new();
        let mut usage: Option<Usage> = None;
        let mut chunk_count = 0u32;

        while let Some(event) = es.next().await {
            match event {
                Ok(Event::Open) => {
                    debug!(request_id = %request_id, "DeepSeek SSE connection opened");
                }
                Ok(Event::Message(msg)) => {
                    if msg.data == "[DONE]" {
                        debug!(
                            request_id = %request_id,
                            chunks = chunk_count,
                            duration_ms = start_time.elapsed().as_millis(),
                            "DeepSeek stream complete"
                        );
                        break;
                    }

                    chunk_count += 1;
                    let chunk: ChatChunk = match serde_json::from_str(&msg.data) {
                        Ok(c) => c,
                        Err(e) => {
                            warn!(
                                request_id = %request_id,
                                error = %e,
                                chunk = chunk_count,
                                "Failed to parse chunk"
                            );
                            debug!(request_id = %request_id, raw_data = %msg.data, "Raw chunk data");
                            continue;
                        }
                    };

                    // Capture usage from final chunk
                    if let Some(u) = chunk.usage {
                        info!(
                            request_id = %request_id,
                            prompt_tokens = u.prompt_tokens,
                            completion_tokens = u.completion_tokens,
                            cache_hit = ?u.prompt_cache_hit_tokens,
                            cache_miss = ?u.prompt_cache_miss_tokens,
                            "DeepSeek usage stats"
                        );
                        usage = Some(u);
                    }

                    for choice in chunk.choices {
                        // Accumulate reasoning content (don't stream - too many events)
                        if let Some(reasoning) = choice.delta.reasoning_content {
                            if !reasoning.is_empty() {
                                full_reasoning.push_str(&reasoning);
                            }
                        }

                        // Accumulate content
                        if let Some(content) = choice.delta.content {
                            if !content.is_empty() {
                                full_content.push_str(&content);
                            }
                        }

                        // Accumulate tool calls
                        if let Some(tc_chunks) = choice.delta.tool_calls {
                            for tc_chunk in tc_chunks {
                                // Ensure we have space for this tool call
                                while tool_calls.len() <= tc_chunk.index {
                                    tool_calls.push(ToolCall {
                                        id: String::new(),
                                        call_type: "function".into(),
                                        function: FunctionCall {
                                            name: String::new(),
                                            arguments: String::new(),
                                        },
                                    });
                                }

                                let tc = &mut tool_calls[tc_chunk.index];

                                if let Some(id) = tc_chunk.id {
                                    tc.id = id;
                                }
                                if let Some(func) = tc_chunk.function {
                                    if let Some(name) = func.name {
                                        tc.function.name = name;
                                    }
                                    if let Some(args) = func.arguments {
                                        tc.function.arguments.push_str(&args);
                                    }
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    error!(
                        request_id = %request_id,
                        error = ?e,
                        chunks_received = chunk_count,
                        "DeepSeek stream error"
                    );
                    break;
                }
            }
        }

        let duration_ms = start_time.elapsed().as_millis() as u64;

        // Log tool calls if any
        if !tool_calls.is_empty() {
            info!(
                request_id = %request_id,
                tool_count = tool_calls.len(),
                tools = ?tool_calls.iter().map(|tc| &tc.function.name).collect::<Vec<_>>(),
                "DeepSeek requested tool calls"
            );
        }

        // Log tool calls (don't broadcast - causes UI flooding)
        for tc in &tool_calls {
            let args: Value = serde_json::from_str(&tc.function.arguments).unwrap_or(Value::Null);
            debug!(
                request_id = %request_id,
                tool = %tc.function.name,
                call_id = %tc.id,
                args = %args,
                "Tool call"
            );
        }

        info!(
            request_id = %request_id,
            duration_ms = duration_ms,
            content_len = full_content.len(),
            reasoning_len = full_reasoning.len(),
            tool_calls = tool_calls.len(),
            "DeepSeek chat complete"
        );

        Ok(ChatResult {
            request_id,
            content: if full_content.is_empty() {
                None
            } else {
                Some(full_content)
            },
            reasoning_content: if full_reasoning.is_empty() {
                None
            } else {
                Some(full_reasoning)
            },
            tool_calls: if tool_calls.is_empty() {
                None
            } else {
                Some(tool_calls)
            },
            usage,
            duration_ms,
        })
    }

    /// Chat with streaming to a channel (for SSE endpoints)
    /// Sends Delta events to the channel instead of WebSocket broadcast
    pub async fn chat_to_channel(
        &self,
        messages: Vec<Message>,
        tools: Option<Vec<Tool>>,
        tx: tokio::sync::mpsc::Sender<crate::web::chat::stream::ChatEvent>,
    ) -> Result<ChatResult> {
        use crate::web::chat::stream::ChatEvent;

        let request_id = Uuid::new_v4().to_string();
        let start_time = Instant::now();

        let request = ChatRequest {
            model: "deepseek-reasoner".into(),
            messages,
            tools,
            tool_choice: Some("auto".into()),
            stream: true,
            max_tokens: Some(8192),
        };

        let request_builder = self
            .client
            .post(DEEPSEEK_API_URL)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request);

        let mut es = EventSource::new(request_builder)
            .map_err(|e| anyhow!("Failed to create EventSource: {}", e))?;

        let mut full_content = String::new();
        let mut full_reasoning = String::new();
        let mut tool_calls: Vec<ToolCall> = Vec::new();
        let mut usage: Option<Usage> = None;

        while let Some(event) = es.next().await {
            match event {
                Ok(Event::Open) => {}
                Ok(Event::Message(msg)) => {
                    if msg.data == "[DONE]" {
                        break;
                    }

                    let chunk: ChatChunk = match serde_json::from_str(&msg.data) {
                        Ok(c) => c,
                        Err(_) => continue,
                    };

                    if let Some(u) = chunk.usage {
                        usage = Some(u);
                    }

                    for choice in chunk.choices {
                        // Stream reasoning (don't send to channel - too noisy)
                        if let Some(reasoning) = choice.delta.reasoning_content {
                            if !reasoning.is_empty() {
                                full_reasoning.push_str(&reasoning);
                            }
                        }

                        // Stream content deltas to channel
                        if let Some(content) = choice.delta.content {
                            if !content.is_empty() {
                                full_content.push_str(&content);
                                // Filter garbage before sending
                                if is_streamable_content(&content) {
                                    let _ = tx.send(ChatEvent::Delta {
                                        content,
                                    }).await;
                                }
                            }
                        }

                        // Accumulate tool calls
                        if let Some(tc_chunks) = choice.delta.tool_calls {
                            for tc_chunk in tc_chunks {
                                while tool_calls.len() <= tc_chunk.index {
                                    tool_calls.push(ToolCall {
                                        id: String::new(),
                                        call_type: "function".into(),
                                        function: FunctionCall {
                                            name: String::new(),
                                            arguments: String::new(),
                                        },
                                    });
                                }

                                let tc = &mut tool_calls[tc_chunk.index];
                                if let Some(id) = tc_chunk.id {
                                    tc.id = id;
                                }
                                if let Some(func) = tc_chunk.function {
                                    if let Some(name) = func.name {
                                        tc.function.name = name;
                                    }
                                    if let Some(args) = func.arguments {
                                        tc.function.arguments.push_str(&args);
                                    }
                                }
                            }
                        }
                    }
                }
                Err(_) => break,
            }
        }

        let duration_ms = start_time.elapsed().as_millis() as u64;

        Ok(ChatResult {
            request_id,
            content: if full_content.is_empty() { None } else { Some(full_content) },
            reasoning_content: if full_reasoning.is_empty() { None } else { Some(full_reasoning) },
            tool_calls: if tool_calls.is_empty() { None } else { Some(tool_calls) },
            usage,
            duration_ms,
        })
    }
}

// ═══════════════════════════════════════
// TOOL DEFINITIONS
// ═══════════════════════════════════════

/// Get the Mira tools available to DeepSeek
pub fn mira_tools() -> Vec<Tool> {
    vec![
        Tool::function(
            "recall_memories",
            "Search semantic memory for relevant context, past decisions, and project knowledge",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Natural language query to search memories"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of results (default 5)",
                        "default": 5
                    }
                },
                "required": ["query"]
            }),
        ),
        Tool::function(
            "search_code",
            "Semantic code search over the project codebase",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Natural language description of code to find"
                    },
                    "language": {
                        "type": "string",
                        "description": "Filter by programming language (e.g., 'rust', 'python')"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of results (default 10)",
                        "default": 10
                    }
                },
                "required": ["query"]
            }),
        ),
        Tool::function(
            "find_callers",
            "Find all functions that call a specific function. Use this when user asks 'who calls X' or 'callers of X'.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "function_name": {
                        "type": "string",
                        "description": "Name of the function to find callers for"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of results (default 20)",
                        "default": 20
                    }
                },
                "required": ["function_name"]
            }),
        ),
        Tool::function(
            "list_tasks",
            "Get current tasks and their status for the project",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "status": {
                        "type": "string",
                        "enum": ["pending", "in_progress", "completed", "blocked"],
                        "description": "Filter by task status"
                    }
                },
                "required": []
            }),
        ),
        Tool::function(
            "list_goals",
            "Get project goals and their progress",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "status": {
                        "type": "string",
                        "enum": ["planning", "in_progress", "blocked", "completed", "abandoned"],
                        "description": "Filter by goal status"
                    }
                },
                "required": []
            }),
        ),
        Tool::function(
            "claude_task",
            "Send a coding task to Claude Code for the current project. Claude will edit files, run commands, and complete the task. Spawns a new instance if none exists.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "task": {
                        "type": "string",
                        "description": "The coding task for Claude Code to complete"
                    }
                },
                "required": ["task"]
            }),
        ),
        Tool::function(
            "claude_close",
            "Close the current project's Claude Code instance when done with coding tasks.",
            serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        ),
        Tool::function(
            "claude_status",
            "Check if Claude Code is running for the current project.",
            serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        ),
        Tool::function(
            "discuss",
            "Have a real-time conversation with Claude. Send a message and wait for Claude's structured response. Use this for code review, debugging together, getting Claude's expert analysis, or collaborating on complex tasks.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "message": {
                        "type": "string",
                        "description": "What to discuss with Claude"
                    }
                },
                "required": ["message"]
            }),
        ),
        Tool::function(
            "google_search",
            "Search the web using Google Custom Search. Returns titles, URLs, and snippets from search results.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "The search query"
                    },
                    "num_results": {
                        "type": "integer",
                        "description": "Number of results to return (1-10, default 5)",
                        "default": 5
                    }
                },
                "required": ["query"]
            }),
        ),
        Tool::function(
            "web_fetch",
            "Fetch and extract content from a web page. Returns the page title and main text content.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "The URL to fetch"
                    }
                },
                "required": ["url"]
            }),
        ),
        Tool::function(
            "research",
            "Research a topic by searching the web, reading top results, and synthesizing findings into a grounded answer with citations. Use this when you need current information, technical comparisons, or factual verification.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "question": {
                        "type": "string",
                        "description": "The question or topic to research"
                    },
                    "depth": {
                        "type": "string",
                        "enum": ["quick", "thorough"],
                        "description": "Research depth: 'quick' (1 query, 3 pages) or 'thorough' (3 queries, 5 pages)",
                        "default": "quick"
                    }
                },
                "required": ["question"]
            }),
        ),
        Tool::function(
            "bash",
            "Execute shell commands on the system. Use for file operations, git, builds, system tasks, and anything outside of code editing.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The bash command to execute"
                    },
                    "working_directory": {
                        "type": "string",
                        "description": "Working directory for the command (defaults to project root)"
                    },
                    "timeout_seconds": {
                        "type": "integer",
                        "description": "Command timeout in seconds (default 60)",
                        "default": 60
                    }
                },
                "required": ["command"]
            }),
        ),
    ]
}
