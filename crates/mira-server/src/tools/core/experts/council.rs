// crates/mira-server/src/tools/core/experts/council.rs
// Council loop: coordinator-driven multi-expert consultation

use super::context::{build_user_prompt, get_patterns_context};
use super::findings::{CouncilFinding, FindingsStore};
use super::plan::{parse_json_with_retry, ResearchPlan, ReviewResult};
use super::prompts::*;
use super::role::ExpertRole;
use super::tools::{
    execute_tool_with_findings, get_expert_tools, store_finding_tool, web_fetch_tool,
    web_search_tool,
};
use super::{
    EXPERT_TIMEOUT, LLM_CALL_TIMEOUT, MAX_CONCURRENT_EXPERTS, MAX_ITERATIONS, ToolContext,
};
use crate::llm::{Message, Tool, record_llm_usage};
use mira_types::{CouncilEvent, WsEvent};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::timeout;
use tracing::{debug, info, warn};

// ═══════════════════════════════════════════════════════════════════════════════
// Constants
// ═══════════════════════════════════════════════════════════════════════════════

/// Maximum council rounds: 1 plan + up to 2 delta rounds, then force synthesis.
const MAX_COUNCIL_ROUNDS: usize = 3;

/// Timeout for coordinator LLM calls (planning + review).
const COORDINATOR_TIMEOUT: Duration = Duration::from_secs(120);

/// Timeout for the entire council execution phase.
const COUNCIL_EXECUTE_TIMEOUT: Duration = Duration::from_secs(900);

// ═══════════════════════════════════════════════════════════════════════════════
// Public API
// ═══════════════════════════════════════════════════════════════════════════════

/// Run a full council consultation with coordinator-driven planning.
///
/// Flow: Plan → Execute → Review → (optional Delta rounds) → Synthesize
pub async fn run_council<C: ToolContext + Clone + 'static>(
    ctx: &C,
    roles: Vec<ExpertRole>,
    context: String,
    question: Option<String>,
) -> Result<String, String> {
    let role_keys: Vec<String> = roles.iter().map(|r| r.db_key()).collect();
    info!(roles = ?role_keys, "Council consultation starting");

    let findings_store = Arc::new(FindingsStore::new());

    // Phase 1: Coordinator creates a research plan
    let plan = plan_phase(ctx, &roles, &context, question.as_deref()).await?;

    ctx.broadcast(WsEvent::Council(CouncilEvent::PlanCreated {
        task_count: plan.tasks.len(),
        roles: plan.tasks.iter().map(|t| t.role.clone()).collect(),
    }));

    info!(
        goal = %plan.goal,
        tasks = plan.tasks.len(),
        excluded = plan.excluded_roles.len(),
        "Council plan created"
    );

    // Phase 2: Execute — experts run their assigned tasks in parallel
    execute_phase(ctx, &plan, &findings_store, &context, question.as_deref()).await?;

    let mut rounds = 1;

    // Phase 3: Review + optional delta rounds
    for round in 0..MAX_COUNCIL_ROUNDS - 1 {
        let review = review_phase(ctx, &findings_store).await?;

        ctx.broadcast(WsEvent::Council(CouncilEvent::ReviewComplete {
            consensus_count: review.consensus.len(),
            conflict_count: review.conflicts.len(),
        }));

        if !review.needs_followup || review.delta_questions.is_empty() {
            info!(round = round + 1, "Council review: consensus reached");
            break;
        }

        rounds += 1;
        info!(
            round = rounds,
            delta_questions = review.delta_questions.len(),
            "Council review: delta round needed"
        );

        ctx.broadcast(WsEvent::Council(CouncilEvent::DeltaRoundStarted {
            round: rounds,
            question_count: review.delta_questions.len(),
        }));

        // Run delta round: targeted follow-up questions
        delta_round(ctx, &review, &findings_store, &context).await?;
    }

    // Phase 4: Synthesize
    ctx.broadcast(WsEvent::Council(CouncilEvent::SynthesisStarted));
    let synthesis = synthesize_phase(ctx, &findings_store).await?;

    let total_findings = findings_store.count();
    ctx.broadcast(WsEvent::Council(CouncilEvent::Complete {
        total_findings,
        rounds,
    }));

    // Format final output
    Ok(format_council_output(
        &synthesis,
        total_findings,
        rounds,
        &role_keys,
    ))
}

// ═══════════════════════════════════════════════════════════════════════════════
// Phase 1: Plan
// ═══════════════════════════════════════════════════════════════════════════════

async fn plan_phase<C: ToolContext>(
    ctx: &C,
    roles: &[ExpertRole],
    context: &str,
    question: Option<&str>,
) -> Result<ResearchPlan, String> {
    let llm_factory = ctx.llm_factory();
    let client = llm_factory
        .client_for_role("coordinator", ctx.pool())
        .await
        .map_err(|e| format!("No LLM available for coordinator: {}", e))?;

    let role_descriptions: Vec<String> = roles
        .iter()
        .map(|r| format!("- {}: {}", r.db_key(), r.name()))
        .collect();

    let user_prompt = format!(
        "## Consultation Request\n\n{}{}\n\n## Available Experts\n\n{}\n\n\
         Create a research plan assigning focused tasks to the most relevant experts.",
        if let Some(q) = question {
            format!("**Question:** {}\n\n", q)
        } else {
            String::new()
        },
        context,
        role_descriptions.join("\n")
    );

    let messages = vec![
        Message::system(COORDINATOR_PLAN_PROMPT.to_string()),
        Message::user(user_prompt),
    ];

    let result = timeout(COORDINATOR_TIMEOUT, client.chat(messages, None))
        .await
        .map_err(|_| format!("Coordinator plan timed out after {}s", COORDINATOR_TIMEOUT.as_secs()))?
        .map_err(|e| format!("Coordinator plan LLM call failed: {}", e))?;

    record_llm_usage(
        ctx.pool(),
        client.provider_type(),
        &client.model_name(),
        "council:coordinator:plan",
        &result,
        ctx.project_id().await,
        ctx.get_session_id().await,
    )
    .await;

    let content = result
        .content
        .as_deref()
        .ok_or("Coordinator returned empty plan")?;

    parse_json_with_retry::<ResearchPlan>(content, &client, "ResearchPlan with goal, tasks array").await
}

// ═══════════════════════════════════════════════════════════════════════════════
// Phase 2: Execute
// ═══════════════════════════════════════════════════════════════════════════════

async fn execute_phase<C: ToolContext + Clone + 'static>(
    ctx: &C,
    plan: &ResearchPlan,
    findings_store: &Arc<FindingsStore>,
    original_context: &str,
    question: Option<&str>,
) -> Result<(), String> {
    use futures::stream::{self, StreamExt};

    let tasks: Vec<_> = plan.tasks.iter().enumerate().map(|(i, task)| {
        let ctx = ctx.clone();
        let store = Arc::clone(findings_store);
        let role_key = task.role.clone();
        let task_desc = task.task.clone();
        let focus_areas = task.focus_areas.clone();
        let context = original_context.to_string();
        let question = question.map(String::from);

        async move {
            let result = run_expert_task(
                &ctx,
                &role_key,
                &task_desc,
                &focus_areas,
                &store,
                &context,
                question.as_deref(),
            )
            .await;
            (i, role_key, result)
        }
    }).collect();

    let results = timeout(
        COUNCIL_EXECUTE_TIMEOUT,
        stream::iter(tasks)
            .buffer_unordered(MAX_CONCURRENT_EXPERTS)
            .collect::<Vec<_>>(),
    )
    .await
    .map_err(|_| format!(
        "Council execution timed out after {}s",
        COUNCIL_EXECUTE_TIMEOUT.as_secs()
    ))?;

    // Log results — don't fail the council if some experts fail
    let mut successes = 0;
    let mut failures = 0;
    for (_i, role_key, result) in &results {
        match result {
            Ok(()) => {
                successes += 1;
                ctx.broadcast(WsEvent::Council(CouncilEvent::ExpertComplete {
                    role: role_key.clone(),
                    finding_count: findings_store.by_role(role_key).len(),
                }));
            }
            Err(e) => {
                failures += 1;
                warn!(role = %role_key, error = %e, "Expert task failed");
            }
        }
    }

    info!(successes, failures, "Council execution phase complete");

    if successes == 0 {
        return Err("All expert tasks failed".to_string());
    }

    Ok(())
}

/// Run a single expert on a focused task, emitting findings to the store.
async fn run_expert_task<C: ToolContext>(
    ctx: &C,
    role_key: &str,
    task_description: &str,
    focus_areas: &[String],
    findings_store: &Arc<FindingsStore>,
    original_context: &str,
    question: Option<&str>,
) -> Result<(), String> {
    let expert = ExpertRole::from_db_key(role_key)
        .ok_or_else(|| format!("Unknown role: {}", role_key))?;

    ctx.broadcast(WsEvent::Council(CouncilEvent::ExpertStarted {
        role: role_key.to_string(),
        task: task_description.to_string(),
    }));

    // Get LLM client
    let llm_factory = ctx.llm_factory();
    let strategy = llm_factory
        .strategy_for_role(role_key, ctx.pool())
        .await
        .map_err(|e| e.to_string())?;

    let chat_client = strategy.actor().clone();

    // Build system prompt: base role prompt + council task scoping
    let base_prompt = expert.system_prompt(ctx).await;
    let focus_str = if focus_areas.is_empty() {
        String::new()
    } else {
        focus_areas.join(", ")
    };
    let task_prompt = COUNCIL_EXPERT_TASK_PROMPT
        .replace("{task}", task_description)
        .replace("{focus_areas}", &focus_str);
    let system_prompt = format!("{}\n\n{}", base_prompt, task_prompt);

    // Inject learned patterns for code reviewer and security experts
    let patterns_context =
        if matches!(expert, ExpertRole::CodeReviewer | ExpertRole::Security) {
            get_patterns_context(ctx, role_key).await
        } else {
            String::new()
        };

    let enriched_context = if patterns_context.is_empty() {
        original_context.to_string()
    } else {
        format!("{}\n{}", original_context, patterns_context)
    };

    let user_prompt = build_user_prompt(&enriched_context, question);

    // Build tool list: standard tools + store_finding + web + MCP
    let mut tools = get_expert_tools();
    tools.push(store_finding_tool());
    tools.push(web_fetch_tool());
    if std::env::var("BRAVE_API_KEY")
        .ok()
        .filter(|k| !k.trim().is_empty())
        .is_some()
    {
        tools.push(web_search_tool());
    }
    let mcp_tools = ctx.mcp_expert_tools().await;
    if !mcp_tools.is_empty() {
        tools.extend(mcp_tools);
    }

    let mut messages = vec![Message::system(system_prompt), Message::user(user_prompt)];

    let mut total_tool_calls = 0;
    let mut iterations = 0;
    let mut previous_response_id: Option<String> = None;

    // Agentic loop
    let _result = timeout(EXPERT_TIMEOUT, async {
        loop {
            iterations += 1;
            if iterations > MAX_ITERATIONS {
                return Err(format!(
                    "Expert {} exceeded maximum iterations",
                    role_key
                ));
            }

            let messages_to_send =
                if previous_response_id.is_some() && chat_client.supports_stateful() {
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
                    messages.clone()
                };

            let result = timeout(
                LLM_CALL_TIMEOUT,
                chat_client.chat_stateful(
                    messages_to_send,
                    Some(tools.clone()),
                    previous_response_id.as_deref(),
                ),
            )
            .await
            .map_err(|_| format!("LLM call timed out for {}", role_key))?
            .map_err(|e| format!("Expert {} LLM call failed: {}", role_key, e))?;

            // Record usage
            let usage_role = format!("council:expert:{}", role_key);
            record_llm_usage(
                ctx.pool(),
                chat_client.provider_type(),
                &chat_client.model_name(),
                &usage_role,
                &result,
                ctx.project_id().await,
                ctx.get_session_id().await,
            )
            .await;

            previous_response_id = Some(result.request_id.clone());

            if let Some(ref tool_calls) = result.tool_calls {
                if !tool_calls.is_empty() {
                    let mut assistant_msg =
                        Message::assistant(result.content.clone(), None);
                    assistant_msg.tool_calls = Some(tool_calls.clone());
                    messages.push(assistant_msg);

                    for tc in tool_calls {
                        total_tool_calls += 1;

                        // Use council-aware executor that handles store_finding
                        let tool_result =
                            execute_tool_with_findings(ctx, tc, findings_store).await;

                        // If this was store_finding, tag the finding with the role
                        if tc.function.name == "store_finding" {
                            // The finding was added without a role; patch it
                            let findings = findings_store.all();
                            if let Some(last) = findings.last() {
                                if last.role.is_empty() {
                                    // We need to fix the role — get mutable access
                                    // Since FindingsStore uses Mutex, we re-add with correct role
                                    // Actually, let's fix this differently: patch in-place
                                    patch_last_finding_role(findings_store, role_key);
                                }
                            }

                            ctx.broadcast(WsEvent::Council(CouncilEvent::FindingAdded {
                                role: role_key.to_string(),
                                topic: serde_json::from_str::<serde_json::Value>(
                                    &tc.function.arguments,
                                )
                                .ok()
                                .and_then(|v| v["topic"].as_str().map(String::from))
                                .unwrap_or_default(),
                            }));
                        }

                        messages.push(Message::tool_result(&tc.id, tool_result));
                    }

                    continue;
                }
            }

            // No tool calls — expert is done
            // If decoupled strategy, run thinker for synthesis
            if strategy.is_decoupled() {
                let thinker = strategy.thinker();
                let assistant_msg =
                    Message::assistant(result.content.clone(), result.reasoning_content.clone());
                messages.push(assistant_msg);
                messages.push(Message::user(
                    "Based on the tool results above, provide your final expert analysis. \
                     Synthesize the findings into a clear, actionable response."
                        .to_string(),
                ));

                let final_result = thinker
                    .chat_stateful(messages, None::<Vec<Tool>>, None::<&str>)
                    .await
                    .map_err(|e| format!("Thinker synthesis failed for {}: {}", role_key, e))?;

                let usage_role = format!("council:expert:{}:reasoner", role_key);
                record_llm_usage(
                    ctx.pool(),
                    thinker.provider_type(),
                    &thinker.model_name(),
                    &usage_role,
                    &final_result,
                    ctx.project_id().await,
                    ctx.get_session_id().await,
                )
                .await;

                // Parse any remaining findings from the final response
                if let Some(ref content) = final_result.content {
                    parse_response_as_findings(content, role_key, findings_store);
                }
            } else {
                // Parse findings from the response text as fallback
                if let Some(ref content) = result.content {
                    parse_response_as_findings(content, role_key, findings_store);
                }
            }

            debug!(
                role = %role_key,
                iterations,
                tool_calls = total_tool_calls,
                findings = findings_store.by_role(role_key).len(),
                "Expert task complete"
            );

            return Ok(());
        }
    })
    .await
    .map_err(|_| format!("{} task timed out", role_key))??;

    Ok(())
}

/// Patch the role field of the last finding in the store.
fn patch_last_finding_role(store: &Arc<FindingsStore>, role: &str) {
    let mut findings = store.all();
    if let Some(last) = findings.last_mut() {
        if last.role.is_empty() {
            // We need to rebuild — FindingsStore uses interior mutability
            // The simplest approach: the store exposes a method for this
            store.patch_last_role(role);
        }
    }
}

/// Parse the expert's final response for any findings not captured via store_finding.
fn parse_response_as_findings(content: &str, role_key: &str, store: &Arc<FindingsStore>) {
    use super::findings::parse_expert_findings;

    let parsed = parse_expert_findings(content, role_key);
    for finding in parsed {
        if finding.content.len() < 20 {
            continue;
        }
        store.add(CouncilFinding {
            role: role_key.to_string(),
            topic: finding.finding_type.clone(),
            content: finding.content,
            evidence: finding
                .file_path
                .into_iter()
                .chain(finding.code_snippet.into_iter())
                .collect(),
            severity: finding.severity,
            recommendation: finding.suggestion,
        });
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Phase 3: Review
// ═══════════════════════════════════════════════════════════════════════════════

async fn review_phase<C: ToolContext>(
    ctx: &C,
    findings_store: &Arc<FindingsStore>,
) -> Result<ReviewResult, String> {
    let llm_factory = ctx.llm_factory();
    let client = llm_factory
        .client_for_role("coordinator", ctx.pool())
        .await
        .map_err(|e| format!("No LLM for coordinator review: {}", e))?;

    let findings_summary = findings_store.format_for_synthesis();

    let user_prompt = format!(
        "## Expert Findings to Review\n\n{}\n\n\
         Analyze these findings for consensus, conflicts, and gaps.",
        findings_summary
    );

    let messages = vec![
        Message::system(COORDINATOR_REVIEW_PROMPT.to_string()),
        Message::user(user_prompt),
    ];

    let result = timeout(COORDINATOR_TIMEOUT, client.chat(messages, None))
        .await
        .map_err(|_| format!("Coordinator review timed out after {}s", COORDINATOR_TIMEOUT.as_secs()))?
        .map_err(|e| format!("Coordinator review LLM call failed: {}", e))?;

    record_llm_usage(
        ctx.pool(),
        client.provider_type(),
        &client.model_name(),
        "council:coordinator:review",
        &result,
        ctx.project_id().await,
        ctx.get_session_id().await,
    )
    .await;

    let content = result
        .content
        .as_deref()
        .ok_or("Coordinator returned empty review")?;

    parse_json_with_retry::<ReviewResult>(content, &client, "ReviewResult with needs_followup, delta_questions, consensus, conflicts").await
}

// ═══════════════════════════════════════════════════════════════════════════════
// Delta Rounds
// ═══════════════════════════════════════════════════════════════════════════════

async fn delta_round<C: ToolContext + Clone + 'static>(
    ctx: &C,
    review: &ReviewResult,
    findings_store: &Arc<FindingsStore>,
    original_context: &str,
) -> Result<(), String> {
    use futures::stream::{self, StreamExt};

    let tasks: Vec<_> = review.delta_questions.iter().map(|dq| {
        let ctx = ctx.clone();
        let store = Arc::clone(findings_store);
        let role_key = dq.role.clone();
        let question = dq.question.clone();
        let dq_context = dq.context.clone();
        let original_context = original_context.to_string();

        async move {
            let result = run_expert_task(
                &ctx,
                &role_key,
                &format!("Follow-up: {}", question),
                &[dq_context],
                &store,
                &original_context,
                Some(&question),
            )
            .await;
            (role_key, result)
        }
    }).collect();

    let results = timeout(
        Duration::from_secs(300), // 5 min for delta rounds
        stream::iter(tasks)
            .buffer_unordered(MAX_CONCURRENT_EXPERTS)
            .collect::<Vec<_>>(),
    )
    .await
    .map_err(|_| "Delta round timed out".to_string())?;

    for (role_key, result) in &results {
        if let Err(e) = result {
            warn!(role = %role_key, error = %e, "Delta round expert failed");
        }
    }

    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════════
// Phase 4: Synthesize
// ═══════════════════════════════════════════════════════════════════════════════

async fn synthesize_phase<C: ToolContext>(
    ctx: &C,
    findings_store: &Arc<FindingsStore>,
) -> Result<String, String> {
    let llm_factory = ctx.llm_factory();

    // Use thinker (reasoner) for synthesis if available
    let strategy = llm_factory
        .strategy_for_role("synthesis", ctx.pool())
        .await
        .map_err(|e| format!("No LLM for synthesis: {}", e))?;

    let client = strategy.thinker().clone();

    let findings_summary = findings_store.format_for_synthesis();

    let user_prompt = format!(
        "## Expert Council Findings\n\n{}\n\n\
         Synthesize these findings into a structured decision document.",
        findings_summary
    );

    let messages = vec![
        Message::system(COUNCIL_SYNTHESIS_PROMPT.to_string()),
        Message::user(user_prompt),
    ];

    let result = client
        .chat(messages, None)
        .await
        .map_err(|e| format!("Synthesis LLM call failed: {}", e))?;

    record_llm_usage(
        ctx.pool(),
        client.provider_type(),
        &client.model_name(),
        "council:synthesis",
        &result,
        ctx.project_id().await,
        ctx.get_session_id().await,
    )
    .await;

    result
        .content
        .ok_or_else(|| "Synthesis returned empty response".to_string())
}

// ═══════════════════════════════════════════════════════════════════════════════
// Output Formatting
// ═══════════════════════════════════════════════════════════════════════════════

fn format_council_output(
    synthesis: &str,
    total_findings: usize,
    rounds: usize,
    roles: &[String],
) -> String {
    let mut output = String::from("## Expert Council Discussion\n\n");
    output.push_str(synthesis);
    output.push_str(&format!(
        "\n\n---\n*Council: {} experts ({}), {} findings, {} round{}*",
        roles.len(),
        roles.join(", "),
        total_findings,
        rounds,
        if rounds == 1 { "" } else { "s" },
    ));
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_council_output() {
        let output = format_council_output(
            "Synthesis here",
            5,
            2,
            &["architect".to_string(), "security".to_string()],
        );
        assert!(output.contains("Expert Council Discussion"));
        assert!(output.contains("Synthesis here"));
        assert!(output.contains("2 experts"));
        assert!(output.contains("5 findings"));
        assert!(output.contains("2 rounds"));
    }

    #[test]
    fn test_format_council_output_single_round() {
        let output = format_council_output("content", 3, 1, &["architect".to_string()]);
        assert!(output.contains("1 round*")); // singular, no "s"
    }

    #[test]
    fn test_max_council_rounds() {
        assert_eq!(MAX_COUNCIL_ROUNDS, 3);
    }
}
