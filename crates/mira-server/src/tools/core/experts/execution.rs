// crates/mira-server/src/tools/core/experts/execution.rs
// Core expert consultation logic

use super::context::{build_user_prompt, format_expert_response, get_patterns_context};
use super::findings::{parse_expert_findings, store_findings};
use super::role::ExpertRole;
use super::tools::{execute_tool, get_expert_tools, web_fetch_tool, web_search_tool};
use super::{
    EXPERT_TIMEOUT, LLM_CALL_TIMEOUT, MAX_CONCURRENT_EXPERTS, MAX_ITERATIONS,
    PARALLEL_EXPERT_TIMEOUT, ToolContext,
};
use crate::llm::{Message, record_llm_usage};
use std::sync::Arc;
use tokio::time::timeout;
use tracing::debug;

/// Core function to consult an expert with agentic tool access
pub async fn consult_expert<C: ToolContext>(
    ctx: &C,
    expert: ExpertRole,
    context: String,
    question: Option<String>,
) -> Result<String, String> {
    let expert_key = expert.db_key();

    // Get reasoning strategy: Single (one model) or Decoupled (chat + reasoner)
    let llm_factory = ctx.llm_factory();
    let strategy = llm_factory
        .strategy_for_role(expert_key.as_str(), ctx.pool())
        .await
        .map_err(|e| e.to_string())?;

    let chat_client = strategy.actor().clone();
    let provider = chat_client.provider_type();
    tracing::info!(expert = %expert_key, provider = %provider, "Expert consultation starting");

    // Get system prompt (async to avoid blocking!)
    let system_prompt = expert.system_prompt(ctx).await;

    // Inject learned patterns for code reviewer and security experts (async to avoid blocking!)
    let patterns_context = if matches!(expert, ExpertRole::CodeReviewer | ExpertRole::Security) {
        get_patterns_context(ctx, expert_key.as_str()).await
    } else {
        String::new()
    };

    // Build user prompt with injected patterns
    let enriched_context = if patterns_context.is_empty() {
        context.clone()
    } else {
        format!("{}\n{}", context, patterns_context)
    };

    let user_prompt = build_user_prompt(&enriched_context, question.as_deref());

    // Build dynamic tool list: built-in + web + MCP tools
    let mut tools = get_expert_tools();

    // Always add web_fetch
    tools.push(web_fetch_tool());

    // Add web_search if BRAVE_API_KEY is configured
    if std::env::var("BRAVE_API_KEY")
        .ok()
        .filter(|k| !k.trim().is_empty())
        .is_some()
    {
        tools.push(web_search_tool());
    }

    // Add MCP tools from configured external servers (with full schemas)
    let mcp_tools = ctx.mcp_expert_tools().await;
    if !mcp_tools.is_empty() {
        debug!(
            mcp_tool_count = mcp_tools.len(),
            "Adding MCP tools to expert tool set"
        );
        tools.extend(mcp_tools);
    }

    debug!(total_tools = tools.len(), "Expert tool set built");

    let mut messages = vec![Message::system(system_prompt), Message::user(user_prompt)];

    let mut total_tool_calls = 0;
    let mut iterations = 0;
    // Track previous response ID for stateful providers
    // This preserves reasoning context across tool-calling turns
    let mut previous_response_id: Option<String> = None;

    // Agentic loop with overall timeout
    let result = timeout(EXPERT_TIMEOUT, async {
        loop {
            iterations += 1;
            if iterations > MAX_ITERATIONS {
                return Err(format!(
                    "Expert exceeded maximum iterations ({}). Partial analysis may be available.",
                    MAX_ITERATIONS
                ));
            }

            // For stateful providers, only send new messages after
            // the first call. The previous_response_id preserves context server-side.
            // For non-stateful providers (DeepSeek, Gemini), always send full history.
            let messages_to_send =
                if previous_response_id.is_some() && chat_client.supports_stateful() {
                    // Only send tool messages (results from current iteration)
                    // These are at the end of the messages vec after the last assistant message
                    messages
                        .iter()
                        .rev()
                        .take_while(|m| m.role == "tool")
                        .cloned()
                        .collect::<Vec<_>>()
                        .into_iter()
                        .rev()
                        .collect()
                } else {
                    // First call OR non-stateful provider - send all messages
                    messages.clone()
                };

            // Call LLM with tools using chat client during tool-gathering phase
            let result = timeout(
                LLM_CALL_TIMEOUT,
                chat_client.chat_stateful(
                    messages_to_send,
                    Some(tools.clone()),
                    previous_response_id.as_deref(),
                ),
            )
            .await
            .map_err(|_| format!("LLM call timed out after {}s", LLM_CALL_TIMEOUT.as_secs()))?
            .map_err(|e| format!("Expert consultation failed: {}", e))?;

            // Record usage for this LLM call
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

            // Store response ID for next iteration (enables reasoning context preservation)
            previous_response_id = Some(result.request_id.clone());

            // Check if the model wants to call tools
            if let Some(ref tool_calls) = result.tool_calls {
                if !tool_calls.is_empty() {
                    // Add assistant message with tool calls.
                    // Drop reasoning_content — intermediate reasoning chains aren't
                    // needed for tool-loop context and cause unbounded memory growth.
                    let mut assistant_msg = Message::assistant(result.content.clone(), None);
                    assistant_msg.tool_calls = Some(tool_calls.clone());
                    messages.push(assistant_msg);

                    // Execute tools in parallel for better performance
                    let tool_futures = tool_calls.iter().map(|tc| {
                        let tc = tc.clone();
                        async move {
                            let result = execute_tool(ctx, &tc).await;
                            (tc.id.clone(), result)
                        }
                    });

                    let tool_results = futures::future::join_all(tool_futures).await;

                    for (id, result) in tool_results {
                        total_tool_calls += 1;
                        messages.push(Message::tool_result(&id, result));
                    }

                    // Continue the loop to get the next response
                    continue;
                }
            }

            // No tool calls - we have a preliminary response from chat client
            // For decoupled strategy, now use thinker for final synthesis
            if strategy.is_decoupled() {
                let thinker = strategy.thinker();
                tracing::debug!(
                    expert = %expert_key,
                    iterations,
                    tool_calls = total_tool_calls,
                    "Tool gathering complete, switching to thinker for synthesis"
                );

                // Add chat client's response as context for thinker
                let assistant_msg =
                    Message::assistant(result.content.clone(), result.reasoning_content.clone());
                messages.push(assistant_msg);

                // Create synthesis prompt for thinker
                let synthesis_prompt = Message::user(String::from(
                    "Based on the tool results above, provide your final expert analysis. \
                    Synthesize the findings into a clear, actionable response.",
                ));
                messages.push(synthesis_prompt);

                // Call thinker without tools for final synthesis (no timeout, it can be slow)
                let final_result = thinker
                    .chat_stateful(messages, None::<Vec<crate::llm::Tool>>, None::<&str>)
                    .await
                    .map_err(|e| format!("Thinker synthesis failed: {}", e))?;

                // Record usage for thinker synthesis call
                let role_for_usage = format!("expert:{}:reasoner", expert_key);
                record_llm_usage(
                    ctx.pool(),
                    thinker.provider_type(),
                    &thinker.model_name(),
                    &role_for_usage,
                    &final_result,
                    ctx.project_id().await,
                    ctx.get_session_id().await,
                )
                .await;

                return Ok((final_result, total_tool_calls, iterations));
            }

            // No reasoner client (non-DeepSeek) - return chat client result directly
            return Ok((result, total_tool_calls, iterations));
        }
    })
    .await
    .map_err(|_| {
        format!(
            "{} consultation timed out after {}s",
            expert.name(),
            EXPERT_TIMEOUT.as_secs()
        )
    })??;

    let (final_result, tool_calls, iters) = result;

    // Parse and store findings for code reviewer and security experts
    if matches!(expert, ExpertRole::CodeReviewer | ExpertRole::Security) {
        if let Some(ref content) = final_result.content {
            let findings = parse_expert_findings(content, expert_key.as_str());
            if !findings.is_empty() {
                let stored = store_findings(ctx, &findings, expert_key.as_str()).await;
                tracing::debug!(
                    expert = %expert_key,
                    parsed = findings.len(),
                    stored,
                    "Parsed and stored review findings"
                );
            }
        }
    }

    Ok(format_expert_response(
        expert,
        final_result,
        tool_calls,
        iters,
    ))
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
            expert_roles.into_iter().next().unwrap(),
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
