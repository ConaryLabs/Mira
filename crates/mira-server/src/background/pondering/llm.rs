// background/pondering/llm.rs
// LLM-based insight generation for pondering

use super::types::{MemoryEntry, PonderingInsight, ToolUsageEntry};
use crate::db::pool::DatabasePool;
use crate::llm::{LlmClient, PromptBuilder, chat_with_usage};
use crate::utils::json::parse_json_hardened;
use crate::utils::truncate;
use std::sync::Arc;

/// Hours to look back for recent activity
const LOOKBACK_HOURS: i64 = 24;

/// Generate insights using LLM
pub(super) async fn generate_insights(
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

    match chat_with_usage(
        &**client,
        pool,
        messages,
        "background:pondering",
        Some(project_id),
        None,
    )
    .await
    {
        Ok(content) => parse_insights(&content),
        Err(e) => {
            tracing::warn!("Failed to generate pondering insights: {}", e);
            Ok(vec![])
        }
    }
}

/// Parse LLM response into insights
pub(super) fn parse_insights(content: &str) -> Result<Vec<PonderingInsight>, String> {
    parse_json_hardened(content).map_err(|e| {
        tracing::debug!("Failed to parse insights JSON: {}", e);
        e
    })
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
}
