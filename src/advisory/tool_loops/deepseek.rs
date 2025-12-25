//! DeepSeek Reasoner tool loop using Chat Completions API

use anyhow::Result;
use std::time::Duration;
use tokio::time::timeout;

use crate::advisory::{
    providers::{ReasonerProvider, AdvisoryRequest, AdvisoryProvider},
    tool_bridge, AdvisoryResponse, AdvisoryMessage, AdvisoryRole,
};

/// DeepSeek Reasoner tool loop using Chat Completions API
///
/// Has an overall timeout of 3 minutes (reasoner is slower due to reasoning).
pub async fn ask_with_tools_deepseek(
    message: &str,
    system: Option<String>,
    ctx: &mut tool_bridge::ToolContext,
) -> Result<AdvisoryResponse> {
    // Overall timeout for the entire tool loop (3 minutes for reasoner)
    const TOOL_LOOP_TIMEOUT_SECS: u64 = 180;

    timeout(
        Duration::from_secs(TOOL_LOOP_TIMEOUT_SECS),
        ask_with_tools_deepseek_inner(message, system, ctx),
    )
    .await
    .map_err(|_| anyhow::anyhow!("DeepSeek tool loop timed out after {} seconds", TOOL_LOOP_TIMEOUT_SECS))?
}

/// Inner implementation of DeepSeek Reasoner tool loop
async fn ask_with_tools_deepseek_inner(
    message: &str,
    system: Option<String>,
    ctx: &mut tool_bridge::ToolContext,
) -> Result<AdvisoryResponse> {
    let reasoner = ReasonerProvider::from_env()?;

    const MAX_TOOL_ROUNDS: usize = 5;
    let mut total_tool_calls = 0;

    // Build initial request with tools enabled
    let mut request = AdvisoryRequest::with_tools(message.to_string());
    request.system = system.clone();

    tracing::info!("Starting DeepSeek tool loop for: {}...", &message[..message.len().min(50)]);

    for round in 0..MAX_TOOL_ROUNDS {
        ctx.tracker.new_call();

        let round_start = std::time::Instant::now();
        tracing::info!("DeepSeek tool loop round {} starting...", round + 1);

        let response = reasoner.complete(request.clone()).await?;

        let elapsed = round_start.elapsed();
        tracing::info!("DeepSeek round {} API call took {:?}", round + 1, elapsed);

        // If no tool calls, we're done
        if response.tool_calls.is_empty() {
            tracing::info!("DeepSeek tool loop complete after {} rounds, {} tool calls", round + 1, total_tool_calls);
            return Ok(response);
        }

        tracing::info!(
            "Round {}: DeepSeek requested {} tool calls: {:?}",
            round + 1,
            response.tool_calls.len(),
            response.tool_calls.iter().map(|c| &c.name).collect::<Vec<_>>()
        );

        // Execute tools and collect results
        let mut tool_results = String::new();
        for call in &response.tool_calls {
            let tool_call = tool_bridge::ToolCall {
                id: call.id.clone(),
                name: call.name.clone(),
                arguments: call.arguments.clone(),
            };
            let result = tool_bridge::execute_tool(ctx, &tool_call).await;
            total_tool_calls += 1;

            tool_results.push_str(&format!("\n[Tool: {}]\n{}\n", call.name, result.content));
        }

        // Add the assistant response and tool results to history
        let assistant_msg = AdvisoryMessage {
            role: AdvisoryRole::Assistant,
            content: if response.text.is_empty() {
                format!("[Called tools: {}]", response.tool_calls.iter().map(|c| c.name.as_str()).collect::<Vec<_>>().join(", "))
            } else {
                response.text.clone()
            },
        };
        request.history.push(assistant_msg);

        // Add tool results as user message
        let tool_msg = AdvisoryMessage {
            role: AdvisoryRole::User,
            content: format!("Tool results:{}\n\nNow provide your response based on the above information.", tool_results),
        };
        request.history.push(tool_msg);

        // Check if we've hit budget limits
        if !ctx.tracker.can_call(&ctx.budget) {
            tracing::warn!("Tool budget exhausted after {} calls", total_tool_calls);
            // Do one more call without tools to get final response
            request.enable_tools = false;
            let final_response = reasoner.complete(request).await?;
            return Ok(final_response);
        }
    }

    // If we hit max rounds, do a final call without tools
    tracing::warn!("Hit max tool rounds ({}), forcing final response", MAX_TOOL_ROUNDS);
    request.enable_tools = false;
    let final_response = reasoner.complete(request).await?;

    Ok(final_response)
}
