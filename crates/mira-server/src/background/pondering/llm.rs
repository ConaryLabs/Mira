// background/pondering/llm.rs
// LLM-based insight generation for pondering

use super::types::{MemoryEntry, PonderingInsight, ProjectInsightData, ToolUsageEntry};
use crate::db::pool::DatabasePool;
use crate::llm::{LlmClient, PromptBuilder, chat_with_usage};
use crate::utils::json::parse_json_hardened;
use crate::utils::truncate;
use std::sync::Arc;

/// Generate insights using LLM with project-aware data.
#[allow(clippy::too_many_arguments)]
pub(super) async fn generate_insights(
    pool: &Arc<DatabasePool>,
    project_id: i64,
    project_name: &str,
    data: &ProjectInsightData,
    tool_history: &[ToolUsageEntry],
    memories: &[MemoryEntry],
    existing_insights: &[String],
    client: &Arc<dyn LlmClient>,
) -> Result<Vec<PonderingInsight>, String> {
    // If there's no meaningful data, skip the LLM call entirely
    if !data.has_data() && tool_history.is_empty() && memories.is_empty() {
        return Ok(vec![]);
    }

    let prompt = build_prompt(
        project_name,
        data,
        tool_history,
        memories,
        existing_insights,
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

/// Build the LLM prompt from project data.
fn build_prompt(
    project_name: &str,
    data: &ProjectInsightData,
    tool_history: &[ToolUsageEntry],
    memories: &[MemoryEntry],
    existing_insights: &[String],
) -> String {
    let mut sections = Vec::new();

    // Goals section
    if !data.stale_goals.is_empty() {
        let items: Vec<String> = data
            .stale_goals
            .iter()
            .map(|g| {
                format!(
                    "- Goal {} '{}': {} for {} days, {}% progress, {}/{} milestones",
                    g.goal_id,
                    truncate(&g.title, 60),
                    g.status,
                    g.days_since_update,
                    g.progress_percent,
                    g.milestones_completed,
                    g.milestones_total,
                )
            })
            .collect();
        sections.push(format!("## Goals & Progress\n{}", items.join("\n")));
    }

    // Code stability section
    if !data.fragile_modules.is_empty() {
        let items: Vec<String> = data
            .fragile_modules
            .iter()
            .map(|m| {
                format!(
                    "- {}: {:.0}% bad rate ({} reverts, {} follow-up fixes / {} changes)",
                    m.module,
                    m.bad_rate * 100.0,
                    m.reverted,
                    m.follow_up_fixes,
                    m.total_changes,
                )
            })
            .collect();
        sections.push(format!(
            "## Code Stability (last 30 days)\n{}",
            items.join("\n")
        ));
    }

    // Recent reverts section
    if !data.revert_clusters.is_empty() {
        let items: Vec<String> = data
            .revert_clusters
            .iter()
            .map(|r| {
                format!(
                    "- {}: {} reverts in {}h (commits: {})",
                    r.module,
                    r.revert_count,
                    r.timespan_hours,
                    truncate(&r.commits.join(", "), 80),
                )
            })
            .collect();
        sections.push(format!(
            "## Recent Reverts (last 7 days)\n{}",
            items.join("\n")
        ));
    }

    // Untested hotspots section
    if !data.untested_hotspots.is_empty() {
        let items: Vec<String> = data
            .untested_hotspots
            .iter()
            .map(|u| {
                format!(
                    "- {}: {} modifications across {} sessions, no test updates",
                    u.file_path, u.modification_count, u.sessions_involved,
                )
            })
            .collect();
        sections.push(format!(
            "## Frequently Modified Files Without Test Updates\n{}",
            items.join("\n")
        ));
    }

    // Session patterns section
    if !data.session_patterns.is_empty() {
        let items: Vec<String> = data
            .session_patterns
            .iter()
            .map(|s| format!("- {}", s.description))
            .collect();
        sections.push(format!("## Recent Session Patterns\n{}", items.join("\n")));
    }

    // Secondary context: recent tool usage
    if !tool_history.is_empty() {
        let tool_summary: Vec<String> = tool_history
            .iter()
            .take(15)
            .map(|e| {
                format!(
                    "- {} ({})",
                    e.tool_name,
                    if e.success { "ok" } else { "fail" }
                )
            })
            .collect();
        sections.push(format!(
            "## Recent Tool Activity\n{}",
            tool_summary.join("\n")
        ));
    }

    // Secondary context: recent memories
    if !memories.is_empty() {
        let memory_summary: Vec<String> = memories
            .iter()
            .take(10)
            .map(|m| {
                let cat = m.category.as_deref().unwrap_or("general");
                format!("- [{}] {}", cat, truncate(&m.content, 80))
            })
            .collect();
        sections.push(format!(
            "## Recent Decisions & Context\n{}",
            memory_summary.join("\n")
        ));
    }

    // Existing insights section — tell the LLM what's already known
    if !existing_insights.is_empty() {
        let items: Vec<String> = existing_insights
            .iter()
            .enumerate()
            .map(|(i, desc)| format!("{}. {}", i + 1, desc))
            .collect();
        sections.push(format!(
            "## Existing Insights (DO NOT REPEAT)\n{}",
            items.join("\n")
        ));
    }

    let data_block = sections.join("\n\n");

    format!(
        r#"You are a senior engineering advisor analyzing project "{project_name}".

{data_block}

## Task
Identify 1-3 SPECIFIC, ACTIONABLE insights that are NOT already covered above. Each MUST:
1. Reference specific files, modules, or goals by name
2. Explain WHY it's a problem
3. Suggest a concrete next step

BAD: "The developer uses context tools frequently"
BAD: "Consider adding more tests"
GOOD: "Goal 94 (deadpool migration) has been in_progress 23 days with 0/3 milestones — check if task 578 is blocking"
GOOD: "Module 'src/db' had 3 reverts in 24h after pool changes — consider adding integration tests before the next pool refactor"

If nothing notable, return []. Don't force insights.

JSON format:
```json
[
  {{
    "pattern_type": "insight_stale_goal|insight_fragile_code|insight_revert_cluster|insight_untested|insight_session|insight_workflow",
    "description": "Brief, specific description referencing actual project entities",
    "confidence": 0.0-1.0,
    "evidence": ["specific observation 1", "specific observation 2"]
  }}
]
```"#,
        project_name = project_name,
        data_block = data_block,
    )
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
        let json = r#"[{"pattern_type": "insight_stale_goal", "description": "test", "confidence": 0.8, "evidence": ["a", "b"]}]"#;
        let insights = parse_insights(json).unwrap();
        assert_eq!(insights.len(), 1);
        assert_eq!(insights[0].pattern_type, "insight_stale_goal");
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
    fn test_build_prompt_with_all_data() {
        use super::super::types::*;
        let data = ProjectInsightData {
            stale_goals: vec![StaleGoal {
                goal_id: 94,
                title: "deadpool migration".to_string(),
                status: "in_progress".to_string(),
                progress_percent: 0,
                days_since_update: 23,
                milestones_total: 3,
                milestones_completed: 0,
            }],
            fragile_modules: vec![FragileModule {
                module: "src/db".to_string(),
                total_changes: 10,
                reverted: 3,
                follow_up_fixes: 1,
                bad_rate: 0.4,
            }],
            revert_clusters: vec![],
            untested_hotspots: vec![],
            session_patterns: vec![],
        };
        let prompt = build_prompt("mira", &data, &[], &[], &[]);
        assert!(prompt.contains("deadpool migration"));
        assert!(prompt.contains("src/db"));
        assert!(prompt.contains("senior engineering advisor"));
        assert!(prompt.contains("SPECIFIC, ACTIONABLE"));
    }

    #[test]
    fn test_build_prompt_empty_data() {
        let data = ProjectInsightData::default();
        let prompt = build_prompt("mira", &data, &[], &[], &[]);
        // Should still produce a valid prompt, just without data sections
        assert!(prompt.contains("mira"));
        assert!(!prompt.contains("## Goals"));
    }

    #[test]
    fn test_build_prompt_with_tool_history() {
        let data = ProjectInsightData::default();
        let tools = vec![ToolUsageEntry {
            tool_name: "code_search".to_string(),
            arguments_summary: "query: auth".to_string(),
            success: true,
            timestamp: "2026-01-01".to_string(),
        }];
        let prompt = build_prompt("mira", &data, &tools, &[], &[]);
        assert!(prompt.contains("Recent Tool Activity"));
        assert!(prompt.contains("code_search"));
    }

    #[test]
    fn test_build_prompt_with_existing_insights() {
        let data = ProjectInsightData::default();
        let existing = vec![
            "Error handling quality issues".to_string(),
            "Heavy context switching".to_string(),
        ];
        let prompt = build_prompt("mira", &data, &[], &[], &existing);
        assert!(prompt.contains("Existing Insights (DO NOT REPEAT)"));
        assert!(prompt.contains("Error handling quality issues"));
        assert!(prompt.contains("Heavy context switching"));
        assert!(prompt.contains("NOT already covered above"));
    }
}
