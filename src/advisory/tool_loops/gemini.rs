//! Gemini 3 Pro tool loop with thought signature preservation

use anyhow::Result;
use std::time::Duration;
use tokio::time::timeout;

use crate::advisory::{
    providers::{
        GeminiProvider, GeminiContent, GeminiPart,
        GeminiTextPart, GeminiFunctionCallPart, GeminiFunctionCall,
        GeminiFunctionResponsePart, GeminiFunctionResponse,
    },
    tool_bridge, AdvisoryResponse,
};

/// Gemini 3 Pro tool loop with thought signature preservation
///
/// Has an overall timeout of 2 minutes to prevent runaway tool loops.
pub async fn ask_with_tools_gemini(
    message: &str,
    system: Option<String>,
    ctx: &mut tool_bridge::ToolContext,
) -> Result<AdvisoryResponse> {
    // Overall timeout for the entire tool loop (2 minutes)
    const TOOL_LOOP_TIMEOUT_SECS: u64 = 120;

    timeout(
        Duration::from_secs(TOOL_LOOP_TIMEOUT_SECS),
        ask_with_tools_gemini_inner(message, system, ctx),
    )
    .await
    .map_err(|_| anyhow::anyhow!("Gemini tool loop timed out after {} seconds", TOOL_LOOP_TIMEOUT_SECS))?
}

/// Inner implementation of Gemini tool loop
async fn ask_with_tools_gemini_inner(
    message: &str,
    system: Option<String>,
    ctx: &mut tool_bridge::ToolContext,
) -> Result<AdvisoryResponse> {
    let gemini = GeminiProvider::from_env()?;

    const MAX_TOOL_ROUNDS: usize = 5;
    let mut contents: Vec<GeminiContent> = vec![];
    let mut total_tool_calls = 0;

    tracing::info!("Starting Gemini tool loop for: {}...", &message[..message.len().min(50)]);

    // Start with user message
    contents.push(GeminiContent {
        role: "user".to_string(),
        parts: vec![GeminiPart::Text(GeminiTextPart { text: message.to_string() })],
    });

    for round in 0..MAX_TOOL_ROUNDS {
        ctx.tracker.new_call();

        let round_start = std::time::Instant::now();
        tracing::info!("Gemini tool loop round {} starting...", round + 1);

        let (response, raw_parts) = gemini.complete_with_contents(
            contents.clone(),
            system.clone(),
            true,
        ).await?;

        let elapsed = round_start.elapsed();
        tracing::info!("Gemini round {} API call took {:?}", round + 1, elapsed);

        // If no tool calls, we're done
        if response.tool_calls.is_empty() {
            tracing::info!("Gemini tool loop complete after {} rounds, {} tool calls", round + 1, total_tool_calls);
            return Ok(response);
        }

        tracing::info!(
            "Round {}: Gemini requested {} tool calls: {:?}",
            round + 1,
            response.tool_calls.len(),
            response.tool_calls.iter().map(|c| &c.name).collect::<Vec<_>>()
        );

        // Build model response with function calls (preserving thought signatures)
        let mut model_parts: Vec<GeminiPart> = vec![];

        // Map tool call IDs to thought signatures from raw_parts
        let mut thought_sigs: std::collections::HashMap<String, Option<String>> = std::collections::HashMap::new();
        for (idx, part) in raw_parts.iter().enumerate() {
            if part.function_call.is_some() {
                let call_id = format!("gemini_{}", idx);
                thought_sigs.insert(call_id, part.thought_signature.clone());
            }
        }

        // Add function call parts with thought signatures
        for call in &response.tool_calls {
            let thought_sig = thought_sigs.get(&call.id).cloned().flatten();
            model_parts.push(GeminiPart::FunctionCall(GeminiFunctionCallPart {
                function_call: GeminiFunctionCall {
                    name: call.name.clone(),
                    args: call.arguments.clone(),
                },
                thought_signature: thought_sig,
            }));
        }

        // Add model's function call response
        contents.push(GeminiContent {
            role: "model".to_string(),
            parts: model_parts,
        });

        // Execute tools and build function responses
        let mut response_parts: Vec<GeminiPart> = vec![];
        for call in &response.tool_calls {
            let tool_call = tool_bridge::ToolCall {
                id: call.id.clone(),
                name: call.name.clone(),
                arguments: call.arguments.clone(),
            };
            let result = tool_bridge::execute_tool(ctx, &tool_call).await;
            total_tool_calls += 1;

            response_parts.push(GeminiPart::FunctionResponse(GeminiFunctionResponsePart {
                function_response: GeminiFunctionResponse {
                    name: call.name.clone(),
                    response: serde_json::json!({ "result": result.content }),
                },
            }));
        }

        // Add user turn with function responses
        contents.push(GeminiContent {
            role: "user".to_string(),
            parts: response_parts,
        });

        // Check if we've hit budget limits
        if !ctx.tracker.can_call(&ctx.budget) {
            tracing::warn!("Tool budget exhausted after {} calls", total_tool_calls);
            // Do one more call without tools to get final response
            let (final_response, _) = gemini.complete_with_contents(
                contents,
                system,
                false,
            ).await?;
            return Ok(final_response);
        }
    }

    // If we hit max rounds, do a final call without tools
    tracing::warn!("Hit max tool rounds ({}), forcing final response", MAX_TOOL_ROUNDS);
    let (final_response, _) = gemini.complete_with_contents(
        contents,
        system,
        false,
    ).await?;

    Ok(final_response)
}
