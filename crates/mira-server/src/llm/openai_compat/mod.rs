// crates/mira-server/src/llm/openai_compat/mod.rs
// Shared OpenAI-compatible request/response handling for DeepSeek, Ollama, etc.

mod request;
mod response;

pub use request::ChatRequest;
pub use response::{ChatResponse, ResponseChoice, parse_chat_response};

use crate::llm::{ChatResult, Message, Tool, truncate_messages_to_default_budget};
use anyhow::Result;
use std::future::Future;
use std::time::Instant;
use tracing::{Span, debug, info};
use uuid::Uuid;

/// Configuration for an OpenAI-compatible chat request
pub struct CompatChatConfig {
    /// Provider name for logging (e.g. "DeepSeek", "Ollama")
    pub provider_name: &'static str,
    /// Model name
    pub model: String,
    /// Whether context budget truncation is supported
    pub supports_budget: bool,
    /// Optional max_tokens to set on the request
    pub max_tokens: Option<u32>,
}

/// Execute an OpenAI-compatible chat request with shared boilerplate.
///
/// Handles: UUID generation, span recording, budget truncation, request building,
/// serialization, response parsing, tool call logging, and completion logging.
///
/// The caller provides `execute_http`, a closure that takes `(request_id, body)`
/// and returns the raw response body string.
pub async fn execute_openai_compat_chat<F, Fut>(
    config: CompatChatConfig,
    messages: Vec<Message>,
    tools: Option<Vec<Tool>>,
    execute_http: F,
) -> Result<ChatResult>
where
    F: FnOnce(String, String) -> Fut,
    Fut: Future<Output = Result<String>>,
{
    let request_id = Uuid::new_v4().to_string();
    let start_time = Instant::now();

    Span::current().record("request_id", &request_id);

    // Apply budget-aware truncation if enabled
    let messages = if config.supports_budget {
        let original_count = messages.len();
        let messages = truncate_messages_to_default_budget(messages);
        if messages.len() != original_count {
            info!(
                request_id = %request_id,
                original_messages = original_count,
                truncated_messages = messages.len(),
                "Applied context budget truncation"
            );
        }
        messages
    } else {
        messages
    };

    info!(
        request_id = %request_id,
        message_count = messages.len(),
        tool_count = tools.as_ref().map(|t| t.len()).unwrap_or(0),
        model = %config.model,
        "Starting {} chat request", config.provider_name
    );

    // Build request using shared ChatRequest
    let mut request = ChatRequest::new(&config.model, messages).with_tools(tools);
    if let Some(max) = config.max_tokens {
        request = request.with_max_tokens(max);
    }

    let body = serde_json::to_string(&request)?;
    debug!(request_id = %request_id, "{} request: {}", config.provider_name, body);

    let response_body = execute_http(request_id.clone(), body).await?;

    let duration_ms = start_time.elapsed().as_millis() as u64;

    // Parse response using shared parser
    let result = parse_chat_response(&response_body, &request_id, duration_ms)?;

    // Log tool calls if any
    if let Some(ref tcs) = result.tool_calls {
        crate::llm::logging::log_tool_calls(&request_id, config.provider_name, tcs);
    }

    crate::llm::logging::log_completion(
        &request_id,
        config.provider_name,
        duration_ms,
        result.content.as_ref().map(|c| c.len()).unwrap_or(0),
        result
            .reasoning_content
            .as_ref()
            .map(|r| r.len())
            .unwrap_or(0),
        result.tool_calls.as_ref().map(|t| t.len()).unwrap_or(0),
    );

    Ok(result)
}
