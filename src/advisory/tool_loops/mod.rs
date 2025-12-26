//! Tool loop wrappers for each advisory provider
//!
//! These functions wrap the generic tool_loop::run_tool_loop with
//! provider-specific initialization, maintaining backward compatibility.

use anyhow::Result;

use crate::advisory::{
    providers::{GptProvider, GeminiProvider, OpusProvider, ReasonerProvider},
    tool_bridge::ToolContext,
    tool_loop::{run_tool_loop, ToolLoopConfig},
    AdvisoryResponse,
};

/// GPT-5.2 tool loop using Responses API
pub async fn ask_with_tools_gpt(
    message: &str,
    system: Option<String>,
    ctx: &mut ToolContext,
) -> Result<AdvisoryResponse> {
    let provider = GptProvider::from_env()?;
    let result = run_tool_loop(&provider, message, system, ctx, ToolLoopConfig::default()).await?;
    Ok(result.response)
}

/// Gemini 3 Pro tool loop with thought signature support
pub async fn ask_with_tools_gemini(
    message: &str,
    system: Option<String>,
    ctx: &mut ToolContext,
) -> Result<AdvisoryResponse> {
    let provider = GeminiProvider::from_env()?;
    let result = run_tool_loop(&provider, message, system, ctx, ToolLoopConfig::default()).await?;
    Ok(result.response)
}

/// Opus 4.5 tool loop with extended thinking support
pub async fn ask_with_tools_opus(
    message: &str,
    system: Option<String>,
    ctx: &mut ToolContext,
) -> Result<AdvisoryResponse> {
    let provider = OpusProvider::from_env()?;
    let result = run_tool_loop(&provider, message, system, ctx, ToolLoopConfig::default()).await?;
    Ok(result.response)
}

/// DeepSeek Reasoner tool loop with retry logic
pub async fn ask_with_tools_deepseek(
    message: &str,
    system: Option<String>,
    ctx: &mut ToolContext,
) -> Result<AdvisoryResponse> {
    let provider = ReasonerProvider::from_env()?;
    // DeepSeek Reasoner needs longer timeout
    let result = run_tool_loop(&provider, message, system, ctx, ToolLoopConfig::for_reasoner()).await?;
    Ok(result.response)
}
