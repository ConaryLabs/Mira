// crates/mira-server/src/background/proactive.rs
// Background proactive suggestion processing
//
// Two-tier frequency:
// - Pattern mining: Every 3rd cycle (~15 minutes) - SQL only, no LLM
// - LLM enhancement: Every 10th cycle (~50 minutes) - generates contextual suggestions

use crate::db::pool::DatabasePool;
use crate::llm::{LlmClient, PromptBuilder, record_llm_usage};
use crate::proactive::patterns::{
    BehaviorPattern, PatternData, get_high_confidence_patterns, run_pattern_mining,
};
use crate::utils::ResultExt;
use rusqlite::params;
use std::sync::Arc;

/// Process proactive suggestions in background
///
/// - Every 3rd cycle: Mine patterns from behavior logs (SQL only, fast)
/// - Every 10th cycle: Enhance high-confidence patterns with LLM-generated suggestions
pub async fn process_proactive(
    pool: &Arc<DatabasePool>,
    client: &Arc<dyn LlmClient>,
    cycle_count: u64,
) -> Result<usize, String> {
    let mut processed = 0;

    // Pattern mining every 3rd cycle (fast, SQL only)
    if cycle_count.is_multiple_of(3) {
        processed += mine_patterns(pool).await?;
    }

    // LLM enhancement every 10th cycle (expensive)
    if cycle_count.is_multiple_of(10) {
        processed += enhance_suggestions(pool, client).await?;
    }

    Ok(processed)
}

/// Mine patterns from behavior logs - SQL only, no LLM
async fn mine_patterns(pool: &Arc<DatabasePool>) -> Result<usize, String> {
    // Get all projects with recent activity
    let projects = pool
        .interact(|conn| {
            let mut stmt = conn
                .prepare(
                    r#"
                    SELECT DISTINCT p.id
                    FROM projects p
                    JOIN sessions s ON s.project_id = p.id
                    WHERE s.last_activity > datetime('now', '-24 hours')
                "#,
                )
                .map_err(|e| anyhow::anyhow!("Failed to prepare: {}", e))?;

            let rows = stmt
                .query_map([], |row| row.get::<_, i64>(0))
                .map_err(|e| anyhow::anyhow!("Failed to query: {}", e))?;

            rows.collect::<Result<Vec<_>, _>>()
                .map_err(|e| anyhow::anyhow!("Failed to collect: {}", e))
        })
        .await
        .str_err()?;

    let mut total_patterns = 0;

    for project_id in projects {
        let pool_clone = pool.clone();
        let patterns_stored = pool_clone
            .interact(move |conn| {
                run_pattern_mining(conn, project_id)
                    .map_err(|e| anyhow::anyhow!("Mining failed: {}", e))
            })
            .await
            .str_err()?;

        if patterns_stored > 0 {
            tracing::debug!(
                "Proactive: mined {} patterns for project {}",
                patterns_stored,
                project_id
            );
            total_patterns += patterns_stored;
        }
    }

    if total_patterns > 0 {
        tracing::info!("Proactive: mined {} total patterns", total_patterns);
    }

    Ok(total_patterns)
}

/// Enhance high-confidence patterns with LLM-generated suggestions
async fn enhance_suggestions(
    pool: &Arc<DatabasePool>,
    client: &Arc<dyn LlmClient>,
) -> Result<usize, String> {
    // Get projects with high-confidence patterns that don't have recent suggestions
    let projects_with_patterns = pool
        .interact(|conn| {
            let mut stmt = conn
                .prepare(
                    r#"
                    SELECT DISTINCT bp.project_id
                    FROM behavior_patterns bp
                    WHERE bp.confidence >= 0.7
                      AND bp.pattern_type IN ('file_sequence', 'tool_chain')
                      AND bp.occurrence_count >= 3
                      -- Only process if no recent suggestions for this project
                      AND NOT EXISTS (
                          SELECT 1 FROM proactive_suggestions ps
                          WHERE ps.project_id = bp.project_id
                            AND ps.created_at > datetime('now', '-1 day')
                      )
                    LIMIT 5
                "#,
                )
                .map_err(|e| anyhow::anyhow!("Failed to prepare: {}", e))?;

            let rows = stmt
                .query_map([], |row| row.get::<_, i64>(0))
                .map_err(|e| anyhow::anyhow!("Failed to query: {}", e))?;

            rows.collect::<Result<Vec<_>, _>>()
                .map_err(|e| anyhow::anyhow!("Failed to collect: {}", e))
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

/// A pre-generated suggestion ready for storage
#[derive(Debug)]
struct PreGeneratedSuggestion {
    pattern_id: Option<i64>,
    trigger_key: String,
    suggestion_text: String,
    confidence: f64,
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

    match client.chat(messages, None).await {
        Ok(result) => {
            // Record usage
            record_llm_usage(
                pool,
                client.provider_type(),
                &client.model_name(),
                "background:proactive",
                &result,
                Some(project_id),
                None,
            )
            .await;

            let content = result.content.as_deref().unwrap_or("[]");
            parse_suggestions(content, patterns)
        }
        Err(e) => {
            tracing::warn!("Failed to generate proactive suggestions: {}", e);
            Ok(vec![])
        }
    }
}

/// Parse LLM response into suggestions
fn parse_suggestions(
    content: &str,
    patterns: &[BehaviorPattern],
) -> Result<Vec<PreGeneratedSuggestion>, String> {
    // Extract JSON from markdown code block if present
    let json_str = if let Some(start) = content.find("```json") {
        let start = start + 7;
        if let Some(end) = content[start..].find("```") {
            &content[start..start + end]
        } else {
            content
        }
    } else if let Some(start) = content.find('[') {
        if let Some(end) = content.rfind(']') {
            &content[start..=end]
        } else {
            content
        }
    } else {
        content
    };

    #[derive(serde::Deserialize)]
    struct LlmSuggestion {
        trigger: String,
        hint: String,
    }

    let parsed: Vec<LlmSuggestion> = serde_json::from_str(json_str.trim()).map_err(|e| {
        tracing::debug!("Failed to parse suggestions JSON: {} from: {}", e, json_str);
        format!("JSON parse error: {}", e)
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

/// Store suggestions in the database
async fn store_suggestions(
    pool: &Arc<DatabasePool>,
    project_id: i64,
    suggestions: &[PreGeneratedSuggestion],
) -> Result<usize, String> {
    if suggestions.is_empty() {
        return Ok(0);
    }

    let suggestions_clone: Vec<_> = suggestions
        .iter()
        .map(|s| PreGeneratedSuggestion {
            pattern_id: s.pattern_id,
            trigger_key: s.trigger_key.clone(),
            suggestion_text: s.suggestion_text.clone(),
            confidence: s.confidence,
        })
        .collect();

    pool.interact(move |conn| {
        let mut stored = 0;

        for suggestion in &suggestions_clone {
            // Upsert suggestion - replace if trigger_key exists
            let result = conn.execute(
                r#"
                INSERT INTO proactive_suggestions
                    (project_id, pattern_id, trigger_key, suggestion_text, confidence, expires_at)
                VALUES (?, ?, ?, ?, ?, datetime('now', '+7 days'))
                ON CONFLICT(project_id, trigger_key) DO UPDATE SET
                    pattern_id = excluded.pattern_id,
                    suggestion_text = excluded.suggestion_text,
                    confidence = excluded.confidence,
                    expires_at = datetime('now', '+7 days')
                "#,
                params![
                    project_id,
                    suggestion.pattern_id,
                    suggestion.trigger_key,
                    suggestion.suggestion_text,
                    suggestion.confidence,
                ],
            );

            match result {
                Ok(_) => stored += 1,
                Err(e) => tracing::warn!("Failed to store suggestion: {}", e),
            }
        }

        Ok::<usize, anyhow::Error>(stored)
    })
    .await
    .map_err(|e| e.to_string())
}

/// Get pre-generated suggestions for a trigger key (fast O(1) lookup)
pub fn get_pre_generated_suggestions(
    conn: &rusqlite::Connection,
    project_id: i64,
    trigger_key: &str,
) -> Result<Vec<(String, f64)>, rusqlite::Error> {
    let mut stmt = conn.prepare_cached(
        r#"
        SELECT suggestion_text, confidence
        FROM proactive_suggestions
        WHERE project_id = ?
          AND trigger_key = ?
          AND (expires_at IS NULL OR expires_at > datetime('now'))
          AND created_at > datetime('now', '-4 hours')
        ORDER BY confidence DESC
        LIMIT 3
    "#,
    )?;

    let rows = stmt.query_map(params![project_id, trigger_key], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
    })?;

    rows.collect()
}

/// Mark a suggestion as shown (for feedback tracking)
pub fn mark_suggestion_shown(
    conn: &rusqlite::Connection,
    project_id: i64,
    trigger_key: &str,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        r#"
        UPDATE proactive_suggestions
        SET shown_count = shown_count + 1
        WHERE project_id = ? AND trigger_key = ?
    "#,
        params![project_id, trigger_key],
    )?;
    Ok(())
}

/// Mark a suggestion as accepted (for feedback tracking)
pub fn mark_suggestion_accepted(
    conn: &rusqlite::Connection,
    project_id: i64,
    trigger_key: &str,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        r#"
        UPDATE proactive_suggestions
        SET accepted_count = accepted_count + 1
        WHERE project_id = ? AND trigger_key = ?
    "#,
        params![project_id, trigger_key],
    )?;
    Ok(())
}

/// Clean up expired suggestions
pub async fn cleanup_expired_suggestions(pool: &Arc<DatabasePool>) -> Result<usize, String> {
    pool.interact(|conn| {
        let deleted = conn
            .execute(
                "DELETE FROM proactive_suggestions WHERE expires_at < datetime('now')",
                [],
            )
            .map_err(|e| anyhow::anyhow!("Failed to cleanup: {}", e))?;
        Ok(deleted)
    })
    .await
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proactive::PatternType;

    #[test]
    fn test_parse_suggestions_json() {
        let patterns = vec![BehaviorPattern {
            id: Some(1),
            project_id: 1,
            pattern_type: PatternType::FileSequence,
            pattern_key: "test".to_string(),
            pattern_data: PatternData::FileSequence {
                files: vec!["src/main.rs".to_string()],
                transitions: vec![("src/main.rs".to_string(), "src/lib.rs".to_string())],
            },
            confidence: 0.8,
            occurrence_count: 5,
        }];

        let json = r#"[{"trigger": "src/main.rs", "hint": "Check lib.rs for related code"}]"#;
        let suggestions = parse_suggestions(json, &patterns).unwrap();

        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].trigger_key, "src/main.rs");
        assert!(suggestions[0].suggestion_text.contains("lib.rs"));
    }

    #[test]
    fn test_parse_suggestions_markdown() {
        let patterns = vec![];
        let markdown = r#"Here are suggestions:
```json
[{"trigger": "grep", "hint": "Consider semantic search"}]
```"#;
        let suggestions = parse_suggestions(markdown, &patterns).unwrap();
        assert_eq!(suggestions.len(), 1);
    }
}
