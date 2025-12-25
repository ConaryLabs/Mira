//! Opus 4.5 tool loop using Anthropic Messages API

use anyhow::Result;
use std::time::Duration;
use tokio::time::timeout;

use crate::advisory::{
    providers::{OpusProvider, OpusInputItem, OpusToolUse},
    tool_bridge, AdvisoryResponse,
};

/// Opus 4.5 tool loop using Anthropic Messages API
///
/// Has an overall timeout of 2 minutes to prevent runaway tool loops.
/// Uses extended thinking which is compatible with tools when tool_choice is auto.
pub async fn ask_with_tools_opus(
    message: &str,
    system: Option<String>,
    ctx: &mut tool_bridge::ToolContext,
) -> Result<AdvisoryResponse> {
    // Overall timeout for the entire tool loop (2 minutes)
    const TOOL_LOOP_TIMEOUT_SECS: u64 = 120;

    timeout(
        Duration::from_secs(TOOL_LOOP_TIMEOUT_SECS),
        ask_with_tools_opus_inner(message, system, ctx),
    )
    .await
    .map_err(|_| anyhow::anyhow!("Opus tool loop timed out after {} seconds", TOOL_LOOP_TIMEOUT_SECS))?
}

/// Inner implementation of Opus 4.5 tool loop
async fn ask_with_tools_opus_inner(
    message: &str,
    system: Option<String>,
    ctx: &mut tool_bridge::ToolContext,
) -> Result<AdvisoryResponse> {
    let opus = OpusProvider::from_env()?;

    const MAX_TOOL_ROUNDS: usize = 5;
    let mut items: Vec<OpusInputItem> = vec![];
    let mut total_tool_calls = 0;

    tracing::info!("Starting Opus tool loop for: {}...", &message[..message.len().min(50)]);

    // Start with user message
    items.push(OpusInputItem::UserMessage(message.to_string()));

    for round in 0..MAX_TOOL_ROUNDS {
        ctx.tracker.new_call();

        let round_start = std::time::Instant::now();
        tracing::info!("Opus tool loop round {} starting...", round + 1);

        let (response, raw_blocks) = opus.complete_with_items(
            items.clone(),
            system.clone(),
            true,
        ).await?;

        let elapsed = round_start.elapsed();
        tracing::info!("Opus round {} API call took {:?}", round + 1, elapsed);

        // If no tool calls, we're done
        if response.tool_calls.is_empty() {
            tracing::info!("Opus tool loop complete after {} rounds, {} tool calls", round + 1, total_tool_calls);
            return Ok(response);
        }

        tracing::info!(
            "Round {}: Opus requested {} tool calls: {:?}",
            round + 1,
            response.tool_calls.len(),
            response.tool_calls.iter().map(|c| &c.name).collect::<Vec<_>>()
        );

        // Extract thinking block with signature from raw_blocks (required for multi-turn with extended thinking)
        // The signature is required by the Anthropic API for validating thinking blocks in multi-turn
        let (thinking, thinking_signature) = raw_blocks.iter()
            .find_map(|block| block.get_thinking_with_signature())
            .map(|(t, s)| (Some(t.to_string()), s.map(|s| s.to_string())))
            .unwrap_or((None, None));

        // Add assistant message with tool uses (including thinking + signature for multi-turn)
        let tool_uses: Vec<OpusToolUse> = response.tool_calls.iter().map(|call| {
            OpusToolUse {
                id: call.id.clone(),
                name: call.name.clone(),
                input: call.arguments.clone(),
            }
        }).collect();

        items.push(OpusInputItem::AssistantMessage {
            thinking,
            thinking_signature,
            text: if response.text.is_empty() { None } else { Some(response.text.clone()) },
            tool_uses,
        });

        // Execute tools and add results
        for call in &response.tool_calls {
            let tool_call = tool_bridge::ToolCall {
                id: call.id.clone(),
                name: call.name.clone(),
                arguments: call.arguments.clone(),
            };
            let result = tool_bridge::execute_tool(ctx, &tool_call).await;
            total_tool_calls += 1;

            items.push(OpusInputItem::ToolResult {
                tool_use_id: call.id.clone(),
                content: result.content,
                is_error: result.is_error,
            });
        }

        // Check if we've hit budget limits
        if !ctx.tracker.can_call(&ctx.budget) {
            tracing::warn!("Tool budget exhausted after {} calls", total_tool_calls);
            // Do one more call without tools to get final response
            let (final_response, _) = opus.complete_with_items(
                items,
                system,
                false,
            ).await?;
            return Ok(final_response);
        }
    }

    // If we hit max rounds, do a final call without tools
    tracing::warn!("Hit max tool rounds ({}), forcing final response", MAX_TOOL_ROUNDS);
    let (final_response, _) = opus.complete_with_items(
        items,
        system,
        false,
    ).await?;

    Ok(final_response)
}
