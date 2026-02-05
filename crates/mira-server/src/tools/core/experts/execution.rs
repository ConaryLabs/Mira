// crates/mira-server/src/tools/core/experts/execution.rs
// Core expert consultation logic

use super::agentic::{AgenticLoopConfig, ToolHandler, run_agentic_loop};
use super::context::{build_user_prompt, format_expert_response, get_patterns_context};
use super::findings::{parse_expert_findings, store_findings};
use super::role::ExpertRole;
use super::tools::{build_expert_toolset, execute_tool};
use super::{
    EXPERT_TIMEOUT, LLM_CALL_TIMEOUT, MAX_CONCURRENT_EXPERTS, MAX_ITERATIONS,
    PARALLEL_EXPERT_TIMEOUT, ToolContext,
};
use crate::llm::{
    DeepSeekClient, GeminiClient, LlmClient, Message, Provider, ToolCall, record_llm_usage,
};
use crate::utils::ResultExt;
use async_trait::async_trait;
use std::sync::Arc;
use tokio::time::timeout;

/// Tool handler for single-expert consultations (parallel tool execution).
struct ExpertToolHandler<'a, C: ToolContext> {
    ctx: &'a C,
}

#[async_trait]
impl<C: ToolContext> ToolHandler for ExpertToolHandler<'_, C> {
    async fn handle_tool_call(&self, tool_call: &ToolCall) -> String {
        execute_tool(self.ctx, tool_call).await
    }
}

/// Enrich context with learned patterns for code_reviewer and security roles.
/// Returns the original context unchanged for other roles.
pub async fn enrich_context_for_role<C: ToolContext>(
    ctx: &C,
    expert: &ExpertRole,
    role_key: &str,
    context: &str,
) -> String {
    let patterns_context = if matches!(expert, ExpertRole::CodeReviewer | ExpertRole::Security) {
        get_patterns_context(ctx, role_key).await
    } else {
        String::new()
    };

    if patterns_context.is_empty() {
        context.to_string()
    } else {
        format!("{}\n{}", context, patterns_context)
    }
}

/// Parse and store review findings for code_reviewer and security experts.
/// No-op for other roles.
async fn maybe_store_findings<C: ToolContext>(
    ctx: &C,
    expert: &ExpertRole,
    expert_key: &str,
    result: &crate::llm::ChatResult,
) {
    if !matches!(expert, ExpertRole::CodeReviewer | ExpertRole::Security) {
        return;
    }
    if let Some(ref content) = result.content {
        let findings = parse_expert_findings(content, expert_key);
        if !findings.is_empty() {
            let parsed = findings.len();
            let stored = store_findings(ctx, findings, expert_key).await;
            tracing::debug!(
                expert = %expert_key,
                parsed,
                stored,
                "Parsed and stored review findings"
            );
        }
    }
}

/// Core function to consult an expert with agentic tool access
pub async fn consult_expert<C: ToolContext>(
    ctx: &C,
    expert: ExpertRole,
    context: String,
    question: Option<String>,
) -> Result<String, String> {
    let expert_key = expert.db_key();

    // Expert consultation fundamentally requires LLM reasoning — no heuristic fallback
    let llm_factory = ctx.llm_factory();

    // Check for real API-keyed providers (not just sampling)
    let has_real_providers = !llm_factory.available_providers().is_empty();

    if !has_real_providers {
        // Try elicitation to get an API key before falling back to sampling or error
        if let Some(one_shot) = try_elicit_api_key(ctx).await {
            tracing::info!(expert = %expert_key, "Using elicitated one-shot client");
            return consult_expert_one_shot(ctx, expert, &expert_key, context, question, one_shot)
                .await;
        }

        // No elicitation result — fall through to sampling or error
        if !llm_factory.has_providers() {
            return Err(format!(
                "Expert consultation ({}) requires an LLM provider. This tool uses AI models \
                 to reason about code. Set DEEPSEEK_API_KEY or GEMINI_API_KEY in ~/.mira/.env, \
                 or unset MIRA_DISABLE_LLM to enable expert consultation.",
                expert.name()
            ));
        }
    }

    // Get reasoning strategy: Single (one model) or Decoupled (chat + reasoner)
    let strategy = llm_factory
        .strategy_for_role(expert_key.as_str(), ctx.pool())
        .await
        .str_err()?;

    let chat_client = strategy.actor().clone();
    let provider = chat_client.provider_type();
    tracing::info!(expert = %expert_key, provider = %provider, "Expert consultation starting");

    // Get system prompt (async to avoid blocking!)
    let system_prompt = expert.system_prompt(ctx).await;

    // Inject learned patterns for code reviewer and security experts
    let enriched_context = enrich_context_for_role(ctx, &expert, &expert_key, &context).await;

    let user_prompt = build_user_prompt(&enriched_context, question.as_deref());

    // Sampling fallback: single-shot via MCP host (no tools, no agentic loop)
    if provider == Provider::Sampling {
        return consult_expert_via_sampling(
            ctx,
            expert,
            &expert_key,
            system_prompt,
            user_prompt,
            &chat_client,
        )
        .await;
    }

    // Build dynamic tool list: built-in + web + MCP tools
    let tools = build_expert_toolset(ctx, false).await;

    let mut messages = vec![Message::system(system_prompt), Message::user(user_prompt)];

    let handler = ExpertToolHandler { ctx };
    let config = AgenticLoopConfig {
        max_turns: MAX_ITERATIONS,
        timeout: EXPERT_TIMEOUT,
        llm_call_timeout: LLM_CALL_TIMEOUT,
        usage_role: format!("expert:{}", expert_key),
    };

    let loop_result =
        run_agentic_loop(ctx, &strategy, &mut messages, tools, &config, &handler).await?;

    let final_result = loop_result.result;
    let tool_calls = loop_result.total_tool_calls;
    let iters = loop_result.iterations;

    maybe_store_findings(ctx, &expert, &expert_key, &final_result).await;

    Ok(format_expert_response(
        expert,
        final_result,
        tool_calls,
        iters,
    ))
}

/// Single-shot expert consultation via MCP sampling (no tools, no agentic loop).
///
/// Used as a zero-key fallback when no DeepSeek/Gemini API keys are configured.
/// The MCP host (Claude Code) handles the actual LLM call.
async fn consult_expert_via_sampling<C: ToolContext>(
    ctx: &C,
    expert: ExpertRole,
    expert_key: &str,
    system_prompt: String,
    user_prompt: String,
    chat_client: &Arc<dyn crate::llm::LlmClient>,
) -> Result<String, String> {
    tracing::info!(expert = %expert_key, "Using MCP sampling for expert consultation (single-shot)");

    let messages = vec![Message::system(system_prompt), Message::user(user_prompt)];

    // Single LLM call — no tools, no iteration
    let result = timeout(LLM_CALL_TIMEOUT, chat_client.chat(messages, None))
        .await
        .map_err(|_| {
            format!(
                "MCP sampling timed out after {}s",
                LLM_CALL_TIMEOUT.as_secs()
            )
        })?
        .map_err(|e| format!("MCP sampling failed: {}", e))?;

    // Record usage
    let role_for_usage = format!("expert:{}", expert_key);
    record_llm_usage(
        ctx.pool(),
        chat_client.provider_type(),
        &chat_client.model_name(),
        &role_for_usage,
        &result,
        ctx.project_id().await,
        ctx.get_session_id().await,
    )
    .await;

    maybe_store_findings(ctx, &expert, expert_key, &result).await;

    Ok(format_expert_response(expert, result, 0, 1))
}

/// Consult multiple experts, with optional council/debate mode.
///
/// - Single expert: delegates directly to `consult_expert()` (no council overhead).
/// - Multiple experts with mode "debate" or "council": runs the council pipeline.
/// - Multiple experts without mode: runs in parallel and concatenates results.
pub async fn consult_experts<C: ToolContext + Clone + 'static>(
    ctx: &C,
    roles: Vec<String>,
    context: String,
    question: Option<String>,
    mode: Option<String>,
) -> Result<String, String> {
    use futures::stream::{self, StreamExt};

    if roles.is_empty() {
        return Err("No expert roles specified".to_string());
    }

    // "debate" is now an alias for "council"
    let is_council = matches!(mode.as_deref(), Some("debate") | Some("council"));

    // Parse and validate all roles first
    let parsed_roles: Result<Vec<ExpertRole>, String> = roles
        .iter()
        .map(|r| {
            ExpertRole::from_db_key(r)
                .ok_or_else(|| format!("Unknown expert role: '{}'. Valid roles: architect, plan_reviewer, scope_analyst, code_reviewer, security", r))
        })
        .collect();

    let expert_roles = parsed_roles?;

    // Single expert bypass: skip council entirely
    if expert_roles.len() == 1 {
        return consult_expert(
            ctx,
            expert_roles.into_iter().next().expect("len == 1"),
            context,
            question,
        )
        .await;
    }

    // Council mode: coordinator-driven multi-expert consultation
    if is_council && expert_roles.len() >= 2 {
        match super::council::run_council(
            ctx,
            expert_roles.clone(),
            context.clone(),
            question.clone(),
        )
        .await
        {
            Ok(council_output) => return Ok(council_output),
            Err(e) => {
                tracing::warn!("Council pipeline failed, falling back to parallel: {}", e);
                // Fall through to standard parallel output
            }
        }
    }

    // Standard parallel mode (also used as council fallback)
    // Use Arc for efficient sharing across concurrent tasks
    let context: Arc<str> = Arc::from(context);
    let question: Option<Arc<str>> = question.map(Arc::from);

    let consultation_future = stream::iter(expert_roles)
        .map(|role| {
            let ctx = ctx.clone();
            let context = Arc::clone(&context);
            let question = question.clone();
            let role_clone = role.clone();
            async move {
                let result = consult_expert(
                    &ctx,
                    role,
                    context.to_string(),
                    question.map(|q| q.to_string()),
                )
                .await;
                (role_clone, result)
            }
        })
        .buffer_unordered(MAX_CONCURRENT_EXPERTS)
        .collect::<Vec<_>>();

    let results = match timeout(PARALLEL_EXPERT_TIMEOUT, consultation_future).await {
        Ok(results) => results,
        Err(_) => {
            return Err(format!(
                "Parallel expert consultation timed out after {} seconds",
                PARALLEL_EXPERT_TIMEOUT.as_secs()
            ));
        }
    };

    // Collect and format results
    let mut output = String::new();
    let mut successes = 0;
    let mut failures = 0;

    for (role, result) in results {
        match result {
            Ok(response) => {
                successes += 1;
                output.push_str(&response);
                output.push_str("\n\n---\n\n");
            }
            Err(e) => {
                failures += 1;
                output.push_str(&format!(
                    "## {} (Failed)\n\nError: {}\n\n---\n\n",
                    role.name(),
                    e
                ));
            }
        }
    }

    // Add summary
    if failures > 0 {
        output.push_str(&format!(
            "*Consulted {} experts: {} succeeded, {} failed*",
            successes + failures,
            successes,
            failures
        ));
    } else {
        output.push_str(&format!("*Consulted {} experts in parallel*", successes));
    }

    Ok(output)
}

/// Try to get an API key from the user via MCP elicitation.
///
/// Returns a one-shot `Arc<dyn LlmClient>` if the user provides a valid key,
/// `None` if elicitation is unavailable, unsupported, or the user declines.
async fn try_elicit_api_key<C: ToolContext>(ctx: &C) -> Option<Arc<dyn LlmClient>> {
    let elicit = ctx.elicitation_client()?;
    if !elicit.is_available().await {
        return None;
    }

    let (provider, key, persist) = crate::mcp::elicitation::request_api_key(&elicit).await?;

    let client: Arc<dyn LlmClient> = match provider {
        // Use deepseek-chat (tool-capable) rather than deepseek-reasoner for one-shot
        Provider::DeepSeek => Arc::new(DeepSeekClient::with_model(
            key.clone(),
            "deepseek-chat".into(),
        )),
        Provider::Gemini => Arc::new(GeminiClient::new(key.clone())),
        _ => return None,
    };

    if persist {
        crate::mcp::elicitation::persist_api_key(provider.api_key_env_var(), &key);
    }

    tracing::info!(provider = %provider, "[expert] Using elicitated API key (one-shot)");
    Some(client)
}

/// Single-shot expert consultation using a one-shot LLM client (from elicitation).
///
/// Similar to `consult_expert_via_sampling` but uses a real provider client,
/// so it supports tools and the agentic loop.
async fn consult_expert_one_shot<C: ToolContext>(
    ctx: &C,
    expert: ExpertRole,
    expert_key: &str,
    context: String,
    question: Option<String>,
    chat_client: Arc<dyn LlmClient>,
) -> Result<String, String> {
    let provider = chat_client.provider_type();
    tracing::info!(expert = %expert_key, provider = %provider, "One-shot expert consultation starting");

    // Get system prompt
    let system_prompt = expert.system_prompt(ctx).await;

    // Inject learned patterns for code reviewer and security experts
    let enriched_context = enrich_context_for_role(ctx, &expert, expert_key, &context).await;

    let user_prompt = build_user_prompt(&enriched_context, question.as_deref());
    let messages = vec![Message::system(system_prompt), Message::user(user_prompt)];

    // Single LLM call with tools (includes MCP tools, fixing prior omission)
    let tools = build_expert_toolset(ctx, false).await;

    let result = timeout(LLM_CALL_TIMEOUT, chat_client.chat(messages, Some(tools)))
        .await
        .map_err(|_| {
            format!(
                "One-shot expert timed out after {}s",
                LLM_CALL_TIMEOUT.as_secs()
            )
        })?
        .map_err(|e| format!("One-shot expert consultation failed: {}", e))?;

    // Record usage
    let role_for_usage = format!("expert:{}", expert_key);
    record_llm_usage(
        ctx.pool(),
        provider,
        &chat_client.model_name(),
        &role_for_usage,
        &result,
        ctx.project_id().await,
        ctx.get_session_id().await,
    )
    .await;

    maybe_store_findings(ctx, &expert, expert_key, &result).await;

    Ok(format_expert_response(expert, result, 0, 1))
}
