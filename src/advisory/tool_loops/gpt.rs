//! GPT-5.2 tool loop using Responses API

use anyhow::Result;

use crate::advisory::{
    providers::{GptProvider, ResponsesInputItem},
    tool_bridge, AdvisoryResponse,
};

/// GPT-5.2 tool loop using Responses API
pub async fn ask_with_tools_gpt(
    message: &str,
    system: Option<String>,
    ctx: &mut tool_bridge::ToolContext,
) -> Result<AdvisoryResponse> {
    let gpt = GptProvider::from_env()?;

    const MAX_TOOL_ROUNDS: usize = 5;
    let mut items: Vec<ResponsesInputItem> = vec![];
    let mut total_tool_calls = 0;

    // Start with user message
    items.push(ResponsesInputItem::Message {
        role: "user".to_string(),
        content: message.to_string(),
    });

    for round in 0..MAX_TOOL_ROUNDS {
        ctx.tracker.new_call();

        let response = gpt.complete_with_items(
            items.clone(),
            system.clone(),
            true,
        ).await?;

        // If no tool calls, we're done
        if response.tool_calls.is_empty() {
            return Ok(response);
        }

        tracing::debug!(
            "Round {}: GPT requested {} tool calls",
            round + 1,
            response.tool_calls.len()
        );

        // Add function_call items and execute tools
        for call in &response.tool_calls {
            // Add the function_call item (required by Responses API)
            items.push(ResponsesInputItem::FunctionCall {
                call_id: call.id.clone(),
                name: call.name.clone(),
                arguments: serde_json::to_string(&call.arguments)
                    .unwrap_or_else(|_| "{}".to_string()),
            });

            // Execute the tool
            let tool_call = tool_bridge::ToolCall {
                id: call.id.clone(),
                name: call.name.clone(),
                arguments: call.arguments.clone(),
            };
            let result = tool_bridge::execute_tool(ctx, &tool_call).await;
            total_tool_calls += 1;

            // Add function_call_output item
            items.push(ResponsesInputItem::FunctionCallOutput {
                call_id: call.id.clone(),
                output: result.content,
            });
        }

        // Check if we've hit budget limits
        if !ctx.tracker.can_call(&ctx.budget) {
            tracing::warn!("Tool budget exhausted after {} calls", total_tool_calls);
            // Do one more call without tools to get final response
            let final_response = gpt.complete_with_items(
                items,
                system,
                false,
            ).await?;
            return Ok(final_response);
        }
    }

    // If we hit max rounds, do a final call without tools
    tracing::warn!("Hit max tool rounds ({}), forcing final response", MAX_TOOL_ROUNDS);
    let final_response = gpt.complete_with_items(
        items,
        system,
        false,
    ).await?;

    Ok(final_response)
}
