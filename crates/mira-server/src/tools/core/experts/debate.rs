// crates/mira-server/src/tools/core/experts/debate.rs
// Expert debate: moderated multi-round discussion between experts

use super::prompts::{CHALLENGER_PROMPT, DEBATE_SYNTHESIS_PROMPT, MODERATOR_PROMPT};
use super::tools::{execute_tool, get_expert_tools};
use super::{
    ToolContext, FOLLOWUP_MAX_ITERATIONS, FOLLOWUP_TIMEOUT, MODERATOR_TIMEOUT,
};
use crate::llm::{LlmClient, Message, Tool, record_llm_usage};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Instant;
use tokio::time::timeout;
use tracing::{debug, info, warn};

// ═══════════════════════════════════════════════════════════════════════════════
// Types
// ═══════════════════════════════════════════════════════════════════════════════

/// A disagreement identified by the moderator
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisagreementFrame {
    pub topic: String,
    pub expert_a: String,
    pub expert_a_position: String,
    pub expert_b: String,
    pub expert_b_position: String,
    pub moderator_question: String,
}

/// Result of the moderation phase
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModerationResult {
    pub disagreements: Vec<DisagreementFrame>,
    pub consensus: Vec<String>,
}

/// Phase timings for output footer
#[derive(Debug, Default)]
struct PhaseTimings {
    moderation_ms: u64,
    followup_ms: u64,
    synthesis_ms: u64,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Public API
// ═══════════════════════════════════════════════════════════════════════════════

/// Run the full debate pipeline after Phase 1 results are available.
/// Returns formatted debate output, or falls back to parallel output on failure.
pub async fn run_debate<C: ToolContext + Clone + 'static>(
    ctx: &C,
    expert_results: &[(String, String)], // (role_key, analysis_text)
) -> Result<String, String> {
    let expert_count = expert_results.len();
    let mut timings = PhaseTimings::default();

    // Phase 2: Moderation — identify disagreements
    let phase2_start = Instant::now();
    let moderation = match run_moderation(ctx, expert_results).await {
        Ok(m) => {
            timings.moderation_ms = phase2_start.elapsed().as_millis() as u64;
            m
        }
        Err(e) => {
            warn!("Debate moderation failed, falling back to parallel output: {}", e);
            return Err(e);
        }
    };

    // If no disagreements, skip Phases 3-4
    if moderation.disagreements.is_empty() {
        info!("Debate moderator found no disagreements — returning consensus output");
        return Ok(format_no_disagreements(expert_results, &moderation.consensus, &timings));
    }

    let disagreement_count = moderation.disagreements.len();
    info!(
        disagreements = disagreement_count,
        consensus_points = moderation.consensus.len(),
        "Debate moderator identified disagreements"
    );

    // Phase 3: Targeted follow-ups
    let phase3_start = Instant::now();
    let followups = match run_followup_round(ctx, expert_results, &moderation.disagreements).await {
        Ok(f) => {
            timings.followup_ms = phase3_start.elapsed().as_millis() as u64;
            f
        }
        Err(e) => {
            warn!("Debate follow-up failed, synthesizing from Phase 1 only: {}", e);
            // Synthesize without follow-up responses
            Vec::new()
        }
    };

    // Phase 4: Final synthesis
    let phase4_start = Instant::now();
    let synthesis = match run_synthesis(ctx, expert_results, &moderation, &followups).await {
        Ok(s) => {
            timings.synthesis_ms = phase4_start.elapsed().as_millis() as u64;
            s
        }
        Err(e) => {
            warn!("Debate synthesis failed, falling back to parallel output: {}", e);
            return Err(e);
        }
    };

    Ok(format_debate_output(
        &synthesis,
        expert_count,
        disagreement_count,
        &timings,
    ))
}

// ═══════════════════════════════════════════════════════════════════════════════
// Phase 2: Moderation
// ═══════════════════════════════════════════════════════════════════════════════

/// Call the moderator LLM to identify disagreements between expert analyses.
async fn run_moderation<C: ToolContext>(
    ctx: &C,
    expert_results: &[(String, String)],
) -> Result<ModerationResult, String> {
    let llm_factory = ctx.llm_factory();

    // Use chat client (not reasoner) — moderator's job is pattern-matching
    let client = llm_factory
        .client_for_role("moderator", ctx.pool())
        .await
        .map_err(|e| format!("No LLM available for moderator: {}", e))?;

    // Build the expert analyses into a single context
    let mut analyses = String::new();
    for (role, analysis) in expert_results {
        analyses.push_str(&format!(
            "=== {} ===\n{}\n\n",
            role, analysis
        ));
    }

    let messages = vec![
        Message::system(MODERATOR_PROMPT.to_string()),
        Message::user(format!(
            "Here are the expert analyses to compare:\n\n{}",
            analyses
        )),
    ];

    let result = timeout(MODERATOR_TIMEOUT, client.chat(messages, None))
        .await
        .map_err(|_| format!("Moderator timed out after {}s", MODERATOR_TIMEOUT.as_secs()))?
        .map_err(|e| format!("Moderator LLM call failed: {}", e))?;

    // Record usage
    record_llm_usage(
        ctx.pool(),
        client.provider_type(),
        &client.model_name(),
        "debate:moderator",
        &result,
        ctx.project_id().await,
        ctx.get_session_id().await,
    )
    .await;

    // Parse the moderator's JSON response
    let content = result
        .content
        .as_deref()
        .ok_or("Moderator returned empty response")?;

    parse_moderation_result(content)
}

/// Parse the moderator's JSON output into a ModerationResult.
fn parse_moderation_result(content: &str) -> Result<ModerationResult, String> {
    // Try to extract JSON from the response (may have markdown fences)
    let json_str = extract_json(content);

    serde_json::from_str::<ModerationResult>(json_str)
        .map_err(|e| format!("Failed to parse moderator JSON: {}. Content: {}", e, &content[..content.len().min(200)]))
}

/// Extract JSON from a string that may contain markdown code fences.
fn extract_json(s: &str) -> &str {
    let trimmed = s.trim();

    // Try stripping ```json ... ``` fences
    if let Some(rest) = trimmed.strip_prefix("```json") {
        if let Some(json) = rest.strip_suffix("```") {
            return json.trim();
        }
    }
    // Try stripping ``` ... ``` fences
    if let Some(rest) = trimmed.strip_prefix("```") {
        if let Some(json) = rest.strip_suffix("```") {
            return json.trim();
        }
    }

    trimmed
}

// ═══════════════════════════════════════════════════════════════════════════════
// Phase 3: Follow-up Round
// ═══════════════════════════════════════════════════════════════════════════════

/// Run targeted follow-up calls for each expert involved in a disagreement.
/// Returns a vec of (role_key, disagreement_topic, response_text).
async fn run_followup_round<C: ToolContext + Clone + 'static>(
    ctx: &C,
    expert_results: &[(String, String)],
    disagreements: &[DisagreementFrame],
) -> Result<Vec<(String, String, String)>, String> {
    let mut futures = Vec::new();

    for disagreement in disagreements {
        // Build follow-up tasks for both experts involved
        for (role_key, opposing_role, opposing_position) in [
            (
                &disagreement.expert_a,
                &disagreement.expert_b,
                &disagreement.expert_b_position,
            ),
            (
                &disagreement.expert_b,
                &disagreement.expert_a,
                &disagreement.expert_a_position,
            ),
        ] {
            // Find the expert's original analysis
            let original_analysis = expert_results
                .iter()
                .find(|(r, _)| r == role_key)
                .map(|(_, a)| a.clone())
                .unwrap_or_default();

            let ctx = ctx.clone();
            let role_key = role_key.clone();
            let topic = disagreement.topic.clone();
            let question = disagreement.moderator_question.clone();
            let opposing_role = opposing_role.clone();
            let opposing_position = opposing_position.clone();

            futures.push(async move {
                let result = run_single_followup(
                    &ctx,
                    &role_key,
                    &original_analysis,
                    &topic,
                    &question,
                    &opposing_role,
                    &opposing_position,
                )
                .await;
                (role_key, topic, result)
            });
        }
    }

    // Run all follow-ups in parallel with a timeout
    let results = timeout(FOLLOWUP_TIMEOUT, futures::future::join_all(futures))
        .await
        .map_err(|_| format!("Follow-up round timed out after {}s", FOLLOWUP_TIMEOUT.as_secs()))?;

    // Collect results, logging any individual failures
    let mut followups = Vec::new();
    for (role_key, topic, result) in results {
        match result {
            Ok(response) => followups.push((role_key, topic, response)),
            Err(e) => {
                warn!(
                    role = %role_key,
                    topic = %topic,
                    "Follow-up failed: {}", e
                );
            }
        }
    }

    Ok(followups)
}

/// Run a single expert follow-up with restricted tools.
async fn run_single_followup<C: ToolContext>(
    ctx: &C,
    role_key: &str,
    original_analysis: &str,
    topic: &str,
    question: &str,
    opposing_role: &str,
    opposing_position: &str,
) -> Result<String, String> {
    let llm_factory = ctx.llm_factory();
    let client = llm_factory
        .client_for_role(role_key, ctx.pool())
        .await
        .map_err(|e| format!("No LLM for follow-up {}: {}", role_key, e))?;

    let user_prompt = format!(
        "## Disagreement: {topic}\n\n\
         **Your original analysis:**\n{original_analysis}\n\n\
         **{opposing_role}'s opposing position:**\n{opposing_position}\n\n\
         **Question to resolve:** {question}\n\n\
         Address this specific tension with evidence from the codebase.",
    );

    let tools = get_followup_tools();
    let mut messages = vec![
        Message::system(CHALLENGER_PROMPT.to_string()),
        Message::user(user_prompt),
    ];

    // Restricted agentic loop (max FOLLOWUP_MAX_ITERATIONS tool calls)
    let mut tool_calls = 0;
    for _ in 0..FOLLOWUP_MAX_ITERATIONS {
        let result = client
            .chat(messages.clone(), Some(tools.clone()))
            .await
            .map_err(|e| format!("Follow-up LLM call failed: {}", e))?;

        // Record usage
        let usage_role = format!("debate:followup:{}", role_key);
        record_llm_usage(
            ctx.pool(),
            client.provider_type(),
            &client.model_name(),
            &usage_role,
            &result,
            ctx.project_id().await,
            ctx.get_session_id().await,
        )
        .await;

        if let Some(ref tc) = result.tool_calls {
            if !tc.is_empty() {
                // Add assistant message with tool calls.
                // Drop reasoning_content to avoid unbounded memory growth.
                let mut assistant_msg =
                    Message::assistant(result.content.clone(), None);
                assistant_msg.tool_calls = Some(tc.clone());
                messages.push(assistant_msg);

                // Execute tools sequentially (restricted set, simple operations)
                for call in tc {
                    tool_calls += 1;
                    let tool_result = execute_tool(ctx, call).await;
                    messages.push(Message::tool_result(&call.id, tool_result));
                }
                continue;
            }
        }

        // No tool calls — we have the response
        debug!(
            role = %role_key,
            topic = %topic,
            tool_calls,
            "Follow-up complete"
        );

        return Ok(result.content.unwrap_or_default());
    }

    // Hit iteration limit — return whatever we have
    Err(format!(
        "Follow-up for {} exceeded {} iterations",
        role_key, FOLLOWUP_MAX_ITERATIONS
    ))
}

/// Get the restricted tool set for follow-up rounds.
/// Only read_file, search_code, and recall — no callers/callees (too slow), no web tools.
fn get_followup_tools() -> Vec<Tool> {
    get_expert_tools()
        .into_iter()
        .filter(|t| {
            matches!(
                t.function.name.as_str(),
                "read_file" | "search_code" | "recall"
            )
        })
        .collect()
}

// ═══════════════════════════════════════════════════════════════════════════════
// Phase 4: Synthesis
// ═══════════════════════════════════════════════════════════════════════════════

/// Run the final synthesis to produce the debate output.
async fn run_synthesis<C: ToolContext>(
    ctx: &C,
    expert_results: &[(String, String)],
    moderation: &ModerationResult,
    followups: &[(String, String, String)], // (role_key, topic, response)
) -> Result<String, String> {
    let llm_factory = ctx.llm_factory();

    // Try to get a reasoner client for synthesis (falls back to chat)
    let (_, reasoner_opt) = llm_factory
        .client_for_role_dual_mode("synthesis", ctx.pool())
        .await
        .map_err(|e| format!("No LLM for synthesis: {}", e))?;

    let client: Arc<dyn LlmClient> = if let Some(reasoner) = reasoner_opt {
        reasoner
    } else {
        llm_factory
            .client_for_role("synthesis", ctx.pool())
            .await
            .map_err(|e| format!("No LLM for synthesis: {}", e))?
    };

    // Build the synthesis context
    let mut context = String::new();

    // Phase 1 results
    context.push_str("# Phase 1: Independent Expert Analyses\n\n");
    for (role, analysis) in expert_results {
        context.push_str(&format!("## {}\n{}\n\n", role, analysis));
    }

    // Phase 2: Moderation results
    context.push_str("# Phase 2: Identified Consensus and Disagreements\n\n");
    context.push_str("## Consensus Points\n");
    for point in &moderation.consensus {
        context.push_str(&format!("- {}\n", point));
    }
    context.push('\n');

    context.push_str("## Disagreements\n");
    for d in &moderation.disagreements {
        context.push_str(&format!(
            "### {}\n- **{}**: {}\n- **{}**: {}\n- Question: {}\n\n",
            d.topic, d.expert_a, d.expert_a_position, d.expert_b, d.expert_b_position, d.moderator_question
        ));
    }

    // Phase 3: Follow-up responses
    if !followups.is_empty() {
        context.push_str("# Phase 3: Expert Responses to Challenges\n\n");
        for (role, topic, response) in followups {
            context.push_str(&format!(
                "## {} on \"{}\"\n{}\n\n",
                role, topic, response
            ));
        }
    }

    let messages = vec![
        Message::system(DEBATE_SYNTHESIS_PROMPT.to_string()),
        Message::user(format!(
            "Synthesize this multi-expert debate into a structured decision document:\n\n{}",
            context
        )),
    ];

    let result = client
        .chat(messages, None)
        .await
        .map_err(|e| format!("Synthesis LLM call failed: {}", e))?;

    // Record usage
    record_llm_usage(
        ctx.pool(),
        client.provider_type(),
        &client.model_name(),
        "debate:synthesis",
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

/// Format the full debate output with header and footer.
fn format_debate_output(
    synthesis: &str,
    expert_count: usize,
    disagreement_count: usize,
    timings: &PhaseTimings,
) -> String {
    let mut output = String::from("## Expert Panel Discussion\n\n");
    output.push_str(synthesis);
    output.push_str(&format!(
        "\n\n---\n*Panel discussion: {} experts, {} disagreement{} identified, 1 follow-up round*\n\
         *Phase timings: Moderation {:.1}s, Follow-up {:.1}s, Synthesis {:.1}s*",
        expert_count,
        disagreement_count,
        if disagreement_count == 1 { "" } else { "s" },
        timings.moderation_ms as f64 / 1000.0,
        timings.followup_ms as f64 / 1000.0,
        timings.synthesis_ms as f64 / 1000.0,
    ));
    output
}

/// Format output when no disagreements were found.
fn format_no_disagreements(
    expert_results: &[(String, String)],
    consensus: &[String],
    timings: &PhaseTimings,
) -> String {
    let mut output = String::from("## Expert Panel Discussion\n\n");

    // Show consensus points if the moderator identified any
    if !consensus.is_empty() {
        output.push_str("### Consensus\n");
        for point in consensus {
            output.push_str(&format!("- {}\n", point));
        }
        output.push('\n');
    }

    // Include the individual analyses
    for (role, analysis) in expert_results {
        output.push_str(&format!("### {} Analysis\n\n{}\n\n", role, analysis));
    }

    output.push_str(&format!(
        "---\n*All experts converged on the same conclusions. No follow-up debate needed.*\n\
         *Moderation: {:.1}s*",
        timings.moderation_ms as f64 / 1000.0,
    ));
    output
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_json_plain() {
        let input = r#"{"disagreements": [], "consensus": ["all good"]}"#;
        assert_eq!(extract_json(input), input);
    }

    #[test]
    fn test_extract_json_with_fences() {
        let input = "```json\n{\"disagreements\": []}\n```";
        assert_eq!(extract_json(input), "{\"disagreements\": []}");
    }

    #[test]
    fn test_extract_json_with_plain_fences() {
        let input = "```\n{\"disagreements\": []}\n```";
        assert_eq!(extract_json(input), "{\"disagreements\": []}");
    }

    #[test]
    fn test_parse_moderation_result_empty_disagreements() {
        let json = r#"{"disagreements": [], "consensus": ["Point A", "Point B"]}"#;
        let result = parse_moderation_result(json).unwrap();
        assert!(result.disagreements.is_empty());
        assert_eq!(result.consensus.len(), 2);
        assert_eq!(result.consensus[0], "Point A");
    }

    #[test]
    fn test_parse_moderation_result_with_disagreement() {
        let json = r#"{
            "disagreements": [{
                "topic": "Error handling",
                "expert_a": "architect",
                "expert_a_position": "Use Result types",
                "expert_b": "code_reviewer",
                "expert_b_position": "Use panics for unrecoverable errors",
                "moderator_question": "When should panics be preferred over Result?"
            }],
            "consensus": ["Code is well-structured"]
        }"#;
        let result = parse_moderation_result(json).unwrap();
        assert_eq!(result.disagreements.len(), 1);
        assert_eq!(result.disagreements[0].topic, "Error handling");
        assert_eq!(result.disagreements[0].expert_a, "architect");
        assert_eq!(result.consensus.len(), 1);
    }

    #[test]
    fn test_parse_moderation_result_with_fences() {
        let json = "```json\n{\"disagreements\": [], \"consensus\": []}\n```";
        let result = parse_moderation_result(json).unwrap();
        assert!(result.disagreements.is_empty());
        assert!(result.consensus.is_empty());
    }

    #[test]
    fn test_parse_moderation_result_invalid() {
        let result = parse_moderation_result("not json at all");
        assert!(result.is_err());
    }

    #[test]
    fn test_get_followup_tools() {
        let tools = get_followup_tools();
        let names: Vec<&str> = tools.iter().map(|t| t.function.name.as_str()).collect();
        assert!(names.contains(&"read_file"));
        assert!(names.contains(&"search_code"));
        assert!(names.contains(&"recall"));
        assert!(!names.contains(&"find_callers"));
        assert!(!names.contains(&"find_callees"));
        assert!(!names.contains(&"get_symbols"));
    }

    #[test]
    fn test_format_debate_output() {
        let timings = PhaseTimings {
            moderation_ms: 8000,
            followup_ms: 30000,
            synthesis_ms: 15000,
        };
        let output = format_debate_output("Synthesis content here", 3, 1, &timings);
        assert!(output.contains("Expert Panel Discussion"));
        assert!(output.contains("Synthesis content here"));
        assert!(output.contains("3 experts"));
        assert!(output.contains("1 disagreement identified"));
        assert!(output.contains("Moderation 8.0s"));
    }

    #[test]
    fn test_format_debate_output_plural_disagreements() {
        let timings = PhaseTimings::default();
        let output = format_debate_output("content", 2, 3, &timings);
        assert!(output.contains("3 disagreements identified"));
    }

    #[test]
    fn test_format_no_disagreements() {
        let results = vec![
            ("architect".to_string(), "Architecture looks good".to_string()),
            ("security".to_string(), "No vulnerabilities found".to_string()),
        ];
        let consensus = vec!["Code is clean".to_string()];
        let timings = PhaseTimings {
            moderation_ms: 5000,
            ..Default::default()
        };
        let output = format_no_disagreements(&results, &consensus, &timings);
        assert!(output.contains("Expert Panel Discussion"));
        assert!(output.contains("Code is clean"));
        assert!(output.contains("All experts converged"));
        assert!(output.contains("architect Analysis"));
        assert!(output.contains("security Analysis"));
    }
}
