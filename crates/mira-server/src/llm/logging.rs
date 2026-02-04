// crates/mira-server/src/llm/logging.rs
// Shared LLM logging helpers to reduce duplication across provider clients

use super::types::{ToolCall, Usage};
use tracing::{debug, info};

/// Log usage statistics for an LLM call.
/// Provider-specific fields (e.g. DeepSeek cache stats) remain inline in the caller.
pub fn log_usage(request_id: &str, provider: &str, usage: &Usage) {
    info!(
        request_id = %request_id,
        prompt_tokens = usage.prompt_tokens,
        completion_tokens = usage.completion_tokens,
        total_tokens = usage.total_tokens,
        "{} usage stats", provider
    );
}

/// Log tool calls requested by the LLM.
pub fn log_tool_calls(request_id: &str, provider: &str, tool_calls: &[ToolCall]) {
    info!(
        request_id = %request_id,
        tool_count = tool_calls.len(),
        tools = ?tool_calls.iter().map(|tc| &tc.function.name).collect::<Vec<_>>(),
        "{} requested tool calls", provider
    );
    for tc in tool_calls {
        let args: serde_json::Value =
            serde_json::from_str(&tc.function.arguments).unwrap_or(serde_json::Value::Null);
        debug!(
            request_id = %request_id,
            tool = %tc.function.name,
            call_id = %tc.id,
            args = %args,
            "Tool call"
        );
    }
}

/// Log completion summary for an LLM call.
pub fn log_completion(
    request_id: &str,
    provider: &str,
    duration_ms: u64,
    content_len: usize,
    reasoning_len: usize,
    tool_call_count: usize,
) {
    info!(
        request_id = %request_id,
        duration_ms = duration_ms,
        content_len = content_len,
        reasoning_len = reasoning_len,
        tool_calls = tool_call_count,
        "{} chat complete", provider
    );
}
