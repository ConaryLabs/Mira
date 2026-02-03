// crates/mira-server/src/background/pondering.rs
// Active Reasoning Loops - "Nightly Pondering"
//
// Analyzes tool usage and memory patterns during idle time to discover
// insights about the developer's workflow and codebase.

use crate::db::pool::DatabasePool;
use crate::llm::{LlmClient, PromptBuilder, record_llm_usage};
use crate::utils::ResultExt;
use crate::utils::json::parse_json_hardened;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Arc;

/// Minimum number of tool calls needed before pondering
const MIN_TOOL_CALLS: usize = 10;

/// Maximum tool history entries to analyze per batch
const MAX_HISTORY_ENTRIES: usize = 100;

/// Hours to look back for recent activity
const LOOKBACK_HOURS: i64 = 24;

#[derive(Debug, Serialize, Deserialize)]
struct ToolUsageEntry {
    tool_name: String,
    arguments_summary: String,
    success: bool,
    timestamp: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct MemoryEntry {
    content: String,
    fact_type: String,
    category: Option<String>,
    status: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct PonderingInsight {
    pattern_type: String,
    description: String,
    confidence: f64,
    evidence: Vec<String>,
}

/// Process pondering - analyze recent activity for patterns
pub async fn process_pondering(
    pool: &Arc<DatabasePool>,
    client: Option<&Arc<dyn LlmClient>>,
) -> Result<usize, String> {
    // Get all projects with recent activity
    let projects = pool
        .interact(|conn| {
            let mut stmt = conn
                .prepare(
                    r#"
                    SELECT DISTINCT p.id, p.name, p.path
                    FROM projects p
                    JOIN sessions s ON s.project_id = p.id
                    WHERE s.last_activity > datetime('now', '-24 hours')
                "#,
                )
                .map_err(|e| anyhow::anyhow!("Failed to prepare: {}", e))?;

            let rows = stmt
                .query_map([], |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                })
                .map_err(|e| anyhow::anyhow!("Failed to query: {}", e))?;

            rows.collect::<Result<Vec<_>, _>>()
                .map_err(|e| anyhow::anyhow!("Failed to collect: {}", e))
        })
        .await
        .str_err()?;

    let mut processed = 0;

    for (project_id, project_name, _project_path) in projects {
        let name = project_name.unwrap_or_else(|| "Unknown".to_string());

        // Check if we should ponder this project
        let should_ponder = should_ponder_project(pool, project_id).await?;
        if !should_ponder {
            continue;
        }

        // Gather data for pondering
        let tool_history = get_recent_tool_history(pool, project_id).await?;
        let memories = get_recent_memories(pool, project_id).await?;

        if tool_history.len() < MIN_TOOL_CALLS {
            tracing::debug!(
                "Project {} has insufficient activity ({} calls), skipping pondering",
                name,
                tool_history.len()
            );
            continue;
        }

        // Generate insights
        let insights = match client {
            Some(c) => {
                generate_insights(pool, project_id, &name, &tool_history, &memories, c).await?
            }
            None => generate_insights_heuristic(&tool_history, &memories),
        };

        // Store insights as behavior patterns
        let stored = store_insights(pool, project_id, &insights).await?;
        if stored > 0 {
            tracing::info!(
                "Pondering: generated {} insights for project {}",
                stored,
                name
            );
            processed += stored;

            // Update last pondering timestamp
            update_last_pondering(pool, project_id).await?;
        }
    }

    Ok(processed)
}

/// Check if enough time has passed since last pondering
async fn should_ponder_project(pool: &Arc<DatabasePool>, project_id: i64) -> Result<bool, String> {
    pool.interact(move |conn| {
        // Check server_state for last pondering time
        let last_pondering: Option<String> = conn
            .query_row(
                "SELECT value FROM server_state WHERE key = ?",
                params![format!("last_pondering_{}", project_id)],
                |row| row.get(0),
            )
            .ok();

        match last_pondering {
            Some(timestamp) => {
                // Only ponder if >6 hours since last time
                let should = conn
                    .query_row(
                        "SELECT datetime(?) < datetime('now', '-6 hours')",
                        params![timestamp],
                        |row| row.get::<_, bool>(0),
                    )
                    .unwrap_or(true);
                Ok(should)
            }
            None => Ok(true), // Never pondered before
        }
    })
    .await
    .str_err()
}

/// Update last pondering timestamp
async fn update_last_pondering(pool: &Arc<DatabasePool>, project_id: i64) -> Result<(), String> {
    pool.interact(move |conn| {
        conn.execute(
            r#"
            INSERT INTO server_state (key, value, updated_at)
            VALUES (?, datetime('now'), datetime('now'))
            ON CONFLICT(key) DO UPDATE SET value = datetime('now'), updated_at = datetime('now')
            "#,
            params![format!("last_pondering_{}", project_id)],
        )
        .map_err(|e| anyhow::anyhow!("Failed to update: {}", e))?;
        Ok(())
    })
    .await
    .str_err()
}

/// Get recent tool usage history for a project
async fn get_recent_tool_history(
    pool: &Arc<DatabasePool>,
    project_id: i64,
) -> Result<Vec<ToolUsageEntry>, String> {
    pool.interact(move |conn| {
        let mut stmt = conn
            .prepare(
                r#"
                SELECT th.tool_name, th.arguments, th.success, th.created_at
                FROM tool_history th
                JOIN sessions s ON s.id = th.session_id
                WHERE s.project_id = ?
                  AND th.created_at > datetime('now', '-' || ? || ' hours')
                ORDER BY th.created_at DESC
                LIMIT ?
            "#,
            )
            .map_err(|e| anyhow::anyhow!("Failed to prepare: {}", e))?;

        let rows = stmt
            .query_map(
                params![project_id, LOOKBACK_HOURS, MAX_HISTORY_ENTRIES],
                |row| {
                    let args: Option<String> = row.get(1)?;
                    Ok(ToolUsageEntry {
                        tool_name: row.get(0)?,
                        arguments_summary: summarize_arguments(&args.unwrap_or_default()),
                        success: row.get::<_, i32>(2)? == 1,
                        timestamp: row.get(3)?,
                    })
                },
            )
            .map_err(|e| anyhow::anyhow!("Failed to query: {}", e))?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| anyhow::anyhow!("Failed to collect: {}", e))
    })
    .await
    .str_err()
}

/// Summarize tool arguments to avoid leaking sensitive data
fn summarize_arguments(args: &str) -> String {
    // Parse JSON and extract just the keys/structure
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(args) {
        if let Some(obj) = value.as_object() {
            let keys: Vec<&str> = obj.keys().map(|s| s.as_str()).collect();
            return format!("keys: {}", keys.join(", "));
        }
    }
    // Fallback: truncate
    if args.len() > 50 {
        format!("{}...", &args[..50])
    } else {
        args.to_string()
    }
}

/// Get recent memories for a project
async fn get_recent_memories(
    pool: &Arc<DatabasePool>,
    project_id: i64,
) -> Result<Vec<MemoryEntry>, String> {
    pool.interact(move |conn| {
        let mut stmt = conn
            .prepare(
                r#"
                SELECT content, fact_type, category, status
                FROM memory_facts
                WHERE project_id = ?
                  AND updated_at > datetime('now', '-7 days')
                ORDER BY updated_at DESC
                LIMIT 50
            "#,
            )
            .map_err(|e| anyhow::anyhow!("Failed to prepare: {}", e))?;

        let rows = stmt
            .query_map(params![project_id], |row| {
                Ok(MemoryEntry {
                    content: row.get(0)?,
                    fact_type: row.get(1)?,
                    category: row.get(2)?,
                    status: row.get(3)?,
                })
            })
            .map_err(|e| anyhow::anyhow!("Failed to query: {}", e))?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| anyhow::anyhow!("Failed to collect: {}", e))
    })
    .await
    .str_err()
}

/// Generate insights using LLM
async fn generate_insights(
    pool: &Arc<DatabasePool>,
    project_id: i64,
    project_name: &str,
    tool_history: &[ToolUsageEntry],
    memories: &[MemoryEntry],
    client: &Arc<dyn LlmClient>,
) -> Result<Vec<PonderingInsight>, String> {
    // Build tool usage summary
    let mut tool_counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    let mut failure_counts: std::collections::HashMap<&str, usize> =
        std::collections::HashMap::new();

    for entry in tool_history {
        *tool_counts.entry(&entry.tool_name).or_default() += 1;
        if !entry.success {
            *failure_counts.entry(&entry.tool_name).or_default() += 1;
        }
    }

    let tool_summary: Vec<String> = tool_counts
        .iter()
        .map(|(tool, count)| {
            let failures = failure_counts.get(tool).unwrap_or(&0);
            if *failures > 0 {
                format!("{}: {} calls ({} failed)", tool, count, failures)
            } else {
                format!("{}: {} calls", tool, count)
            }
        })
        .collect();

    // Build memory summary
    let memory_summary: Vec<String> = memories
        .iter()
        .take(20)
        .map(|m| {
            let cat = m.category.as_deref().unwrap_or("general");
            format!("[{}] {}: {}", m.status, cat, truncate(&m.content, 100))
        })
        .collect();

    let prompt = format!(
        r#"You are analyzing developer activity patterns for project "{project_name}".

## Recent Tool Usage (last {hours} hours)
{tool_summary}

## Recent Tool Sequence (newest first)
{tool_sequence}

## Recent Memories
{memory_summary}

## Task
Identify 1-3 meaningful patterns or insights about this developer's workflow. Focus on:
1. Repeated tool sequences that could be automated
2. Failure patterns that suggest friction points
3. Memory themes that reveal project focus areas
4. Potential workflow improvements

Respond in JSON format:
```json
[
  {{
    "pattern_type": "insight_tool_chain|insight_workflow|insight_friction|insight_focus_area",
    "description": "Brief description of the pattern",
    "confidence": 0.0-1.0,
    "evidence": ["specific observation 1", "specific observation 2"]
  }}
]
```

Only include high-value insights. If nothing notable, return an empty array []."#,
        project_name = project_name,
        hours = LOOKBACK_HOURS,
        tool_summary = tool_summary.join("\n"),
        tool_sequence = tool_history
            .iter()
            .take(20)
            .map(|e| format!(
                "- {} ({})",
                e.tool_name,
                if e.success { "ok" } else { "fail" }
            ))
            .collect::<Vec<_>>()
            .join("\n"),
        memory_summary = memory_summary.join("\n"),
    );

    let messages = PromptBuilder::for_background().build_messages(prompt);

    match client.chat(messages, None).await {
        Ok(result) => {
            // Record usage
            record_llm_usage(
                pool,
                client.provider_type(),
                &client.model_name(),
                "background:pondering",
                &result,
                Some(project_id),
                None,
            )
            .await;

            let content = result.content.as_deref().unwrap_or("[]");
            parse_insights(content)
        }
        Err(e) => {
            tracing::warn!("Failed to generate pondering insights: {}", e);
            Ok(vec![])
        }
    }
}

/// Parse LLM response into insights
fn parse_insights(content: &str) -> Result<Vec<PonderingInsight>, String> {
    parse_json_hardened(content).map_err(|e| {
        tracing::debug!("Failed to parse insights JSON: {}", e);
        e
    })
}

/// Store insights as behavior patterns
async fn store_insights(
    pool: &Arc<DatabasePool>,
    project_id: i64,
    insights: &[PonderingInsight],
) -> Result<usize, String> {
    if insights.is_empty() {
        return Ok(0);
    }

    let insights_clone: Vec<PonderingInsight> = insights
        .iter()
        .map(|i| PonderingInsight {
            pattern_type: i.pattern_type.clone(),
            description: i.description.clone(),
            confidence: i.confidence,
            evidence: i.evidence.clone(),
        })
        .collect();

    pool.interact(move |conn| {
        let mut stored = 0;

        for insight in &insights_clone {
            // Generate pattern key from description hash
            let pattern_key = {
                let mut hasher = Sha256::new();
                hasher.update(insight.description.as_bytes());
                format!("{:x}", hasher.finalize())[..16].to_string()
            };

            let pattern_data = serde_json::json!({
                "description": insight.description,
                "evidence": insight.evidence,
                "generated_by": "pondering",
            });

            // Upsert pattern - increment occurrence if exists
            let result = conn.execute(
                r#"
                INSERT INTO behavior_patterns
                    (project_id, pattern_type, pattern_key, pattern_data, confidence,
                     occurrence_count, last_triggered_at, first_seen_at, updated_at)
                VALUES (?, ?, ?, ?, ?, 1, datetime('now'), datetime('now'), datetime('now'))
                ON CONFLICT(project_id, pattern_type, pattern_key) DO UPDATE SET
                    occurrence_count = occurrence_count + 1,
                    confidence = (confidence + excluded.confidence) / 2,
                    last_triggered_at = datetime('now'),
                    updated_at = datetime('now')
                "#,
                params![
                    project_id,
                    insight.pattern_type,
                    pattern_key,
                    pattern_data.to_string(),
                    insight.confidence,
                ],
            );

            match result {
                Ok(_) => stored += 1,
                Err(e) => tracing::warn!("Failed to store insight: {}", e),
            }
        }

        Ok(stored)
    })
    .await
    .str_err()
}

/// Confidence cap for heuristic insights (consistent with TEMPLATE_CONFIDENCE_MULTIPLIER)
const HEURISTIC_MAX_CONFIDENCE: f64 = 0.85;

/// Minimum total calls before flagging a tool's failure rate
const MIN_CALLS_FOR_FRICTION: usize = 5;

/// Failure rate threshold for friction detection
const FRICTION_FAILURE_RATE: f64 = 0.20;

/// Generate insights from tool history and memories without LLM
fn generate_insights_heuristic(
    tool_history: &[ToolUsageEntry],
    memories: &[MemoryEntry],
) -> Vec<PonderingInsight> {
    let mut insights = Vec::new();

    // Build tool usage counts
    let mut tool_counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    let mut failure_counts: std::collections::HashMap<&str, usize> =
        std::collections::HashMap::new();

    for entry in tool_history {
        *tool_counts.entry(&entry.tool_name).or_default() += 1;
        if !entry.success {
            *failure_counts.entry(&entry.tool_name).or_default() += 1;
        }
    }

    // 1. Tool usage distribution — top tools by call count
    let mut sorted_tools: Vec<_> = tool_counts.iter().collect();
    sorted_tools.sort_by(|a, b| b.1.cmp(a.1));

    if !sorted_tools.is_empty() {
        let top: Vec<String> = sorted_tools
            .iter()
            .take(5)
            .map(|(name, count)| format!("{}: {} calls", name, count))
            .collect();
        insights.push(PonderingInsight {
            pattern_type: "insight_workflow".to_string(),
            description: format!("Tool usage distribution — most used: {}", sorted_tools[0].0),
            confidence: (0.6_f64).min(HEURISTIC_MAX_CONFIDENCE),
            evidence: top,
        });
    }

    // 2. Friction detection — tools with >20% failure rate AND >= 5 total calls
    for (tool, total) in &tool_counts {
        if *total >= MIN_CALLS_FOR_FRICTION {
            let failures = failure_counts.get(tool).copied().unwrap_or(0);
            let rate = failures as f64 / *total as f64;
            if rate >= FRICTION_FAILURE_RATE {
                insights.push(PonderingInsight {
                    pattern_type: "insight_friction".to_string(),
                    description: format!(
                        "High failure rate for '{}': {:.0}% ({}/{} calls failed)",
                        tool,
                        rate * 100.0,
                        failures,
                        total
                    ),
                    confidence: (rate * 0.85).min(HEURISTIC_MAX_CONFIDENCE),
                    evidence: vec![
                        format!("{} total calls", total),
                        format!("{} failures ({:.0}%)", failures, rate * 100.0),
                    ],
                });
            }
        }
    }

    // 3. Focus area detection — group memories by category, report counts
    let mut category_counts: std::collections::HashMap<&str, usize> =
        std::collections::HashMap::new();
    for memory in memories {
        let cat = memory.category.as_deref().unwrap_or("general");
        *category_counts.entry(cat).or_default() += 1;
    }

    if !category_counts.is_empty() {
        let mut sorted_cats: Vec<_> = category_counts.iter().collect();
        sorted_cats.sort_by(|a, b| b.1.cmp(a.1));

        let evidence: Vec<String> = sorted_cats
            .iter()
            .take(5)
            .map(|(cat, count)| format!("{}: {} memories", cat, count))
            .collect();

        if let Some((top_cat, top_count)) = sorted_cats.first() {
            insights.push(PonderingInsight {
                pattern_type: "insight_focus_area".to_string(),
                description: format!(
                    "Primary focus area: '{}' ({} memories across {} categories)",
                    top_cat,
                    top_count,
                    category_counts.len()
                ),
                confidence: (0.5_f64).min(HEURISTIC_MAX_CONFIDENCE),
                evidence,
            });
        }
    }

    insights
}

/// Truncate string with ellipsis
fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_insights_json() {
        let json = r#"[{"pattern_type": "insight_tool_chain", "description": "test", "confidence": 0.8, "evidence": ["a", "b"]}]"#;
        let insights = parse_insights(json).unwrap();
        assert_eq!(insights.len(), 1);
        assert_eq!(insights[0].pattern_type, "insight_tool_chain");
    }

    #[test]
    fn test_parse_insights_markdown() {
        let markdown = r#"Here are the insights:
```json
[{"pattern_type": "insight_workflow", "description": "test", "confidence": 0.7, "evidence": []}]
```"#;
        let insights = parse_insights(markdown).unwrap();
        assert_eq!(insights.len(), 1);
    }

    #[test]
    fn test_summarize_arguments() {
        let args = r#"{"file_path": "/secret/path", "query": "password"}"#;
        let summary = summarize_arguments(args);
        assert!(summary.contains("file_path"));
        assert!(!summary.contains("/secret/path"));
    }
}
