// background/proactive/llm.rs
// LLM-enhanced suggestion generation

use super::storage::store_suggestions;
use super::PreGeneratedSuggestion;
use crate::db::get_projects_needing_suggestions_sync;
use crate::db::pool::DatabasePool;
use crate::llm::{LlmClient, PromptBuilder, chat_with_usage};
use crate::proactive::patterns::{BehaviorPattern, PatternData, get_high_confidence_patterns};
use crate::utils::ResultExt;
use crate::utils::json::parse_json_hardened;
use std::sync::Arc;

/// Enhance high-confidence patterns with LLM-generated suggestions
pub(super) async fn enhance_suggestions(
    pool: &Arc<DatabasePool>,
    client: &Arc<dyn LlmClient>,
) -> Result<usize, String> {
    // Get projects with high-confidence patterns that don't have recent suggestions
    let projects_with_patterns = pool
        .interact(|conn| {
            get_projects_needing_suggestions_sync(conn)
                .map_err(|e| anyhow::anyhow!("Failed to get projects: {}", e))
        })
        .await
        .str_err()?;

    let mut total_suggestions = 0;

    for project_id in projects_with_patterns {
        // Get patterns for this project
        let patterns: Vec<BehaviorPattern> = {
            let pool_clone = pool.clone();
            pool_clone
                .interact(move |conn| {
                    get_high_confidence_patterns(conn, project_id, 0.7)
                        .map_err(|e| anyhow::anyhow!("Failed to get patterns: {}", e))
                })
                .await
                .str_err()?
        };

        if patterns.is_empty() {
            continue;
        }

        // Generate suggestions for patterns
        let suggestions =
            generate_suggestions_for_patterns(pool, project_id, &patterns, client).await?;

        // Store suggestions
        let stored = store_suggestions(pool, project_id, &suggestions).await?;
        total_suggestions += stored;
    }

    if total_suggestions > 0 {
        tracing::info!("Proactive: generated {} LLM suggestions", total_suggestions);
    }

    Ok(total_suggestions)
}

/// Generate contextual suggestions for patterns using LLM
async fn generate_suggestions_for_patterns(
    pool: &Arc<DatabasePool>,
    project_id: i64,
    patterns: &[BehaviorPattern],
    client: &Arc<dyn LlmClient>,
) -> Result<Vec<PreGeneratedSuggestion>, String> {
    // Build a summary of patterns for the LLM
    let pattern_summaries: Vec<String> = patterns
        .iter()
        .take(10) // Limit to avoid huge prompts
        .filter_map(|p| match &p.pattern_data {
            PatternData::FileSequence { transitions, .. } => {
                if transitions.is_empty() {
                    None
                } else {
                    Some(format!(
                        "File sequence (confidence: {:.0}%): {} -> {}",
                        p.confidence * 100.0,
                        transitions[0].0,
                        transitions[0].1
                    ))
                }
            }
            PatternData::ToolChain { tools, .. } => {
                if tools.len() < 2 {
                    None
                } else {
                    Some(format!(
                        "Tool chain (confidence: {:.0}%): {} -> {}",
                        p.confidence * 100.0,
                        tools[0],
                        tools[1]
                    ))
                }
            }
            _ => None,
        })
        .collect();

    if pattern_summaries.is_empty() {
        return Ok(vec![]);
    }

    let prompt = format!(
        r#"You are helping a developer by providing brief, contextual hints based on observed workflow patterns.

## Observed Patterns
{patterns}

## Task
For each pattern, generate a short, helpful hint (max 50 characters) that can be shown to the developer when they're working with the first item in the pattern. The hint should suggest what they might want to look at next.

Respond in JSON format:
```json
[
  {{
    "trigger": "the file path or tool name that triggers this hint",
    "hint": "Brief suggestion (max 50 chars)"
  }}
]
```

Focus on actionable, specific suggestions. Skip patterns that are too generic to be useful."#,
        patterns = pattern_summaries.join("\n"),
    );

    let messages = PromptBuilder::for_background().build_messages(prompt);

    match chat_with_usage(
        &**client,
        pool,
        messages,
        "background:proactive",
        Some(project_id),
        None,
    )
    .await
    {
        Ok(content) => parse_suggestions(&content, patterns),
        Err(e) => {
            tracing::warn!("Failed to generate proactive suggestions: {}", e);
            Ok(vec![])
        }
    }
}

/// Parse LLM response into suggestions
pub(super) fn parse_suggestions(
    content: &str,
    patterns: &[BehaviorPattern],
) -> Result<Vec<PreGeneratedSuggestion>, String> {
    #[derive(serde::Deserialize)]
    struct LlmSuggestion {
        trigger: String,
        hint: String,
    }

    let parsed: Vec<LlmSuggestion> = parse_json_hardened(content).map_err(|e| {
        tracing::debug!("Failed to parse suggestions JSON: {}", e);
        e
    })?;

    // Match suggestions back to patterns for confidence scoring
    let suggestions: Vec<PreGeneratedSuggestion> = parsed
        .into_iter()
        .map(|s| {
            // Find matching pattern to get confidence
            let (pattern_id, confidence) = patterns
                .iter()
                .find(|p| match &p.pattern_data {
                    PatternData::FileSequence { transitions, .. } => transitions
                        .iter()
                        .any(|(from, _)| from.contains(&s.trigger) || s.trigger.contains(from)),
                    PatternData::ToolChain { tools, .. } => tools
                        .first()
                        .is_some_and(|t| t == &s.trigger || s.trigger.contains(t)),
                    _ => false,
                })
                .map(|p| (p.id, p.confidence))
                .unwrap_or((None, 0.7));

            PreGeneratedSuggestion {
                pattern_id,
                trigger_key: s.trigger,
                suggestion_text: s.hint,
                confidence,
            }
        })
        .collect();

    Ok(suggestions)
}
