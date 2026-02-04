// background/pondering/heuristic.rs
// Heuristic (non-LLM) insight generation

use super::types::{MemoryEntry, PonderingInsight, ToolUsageEntry};

/// Confidence cap for heuristic insights (consistent with TEMPLATE_CONFIDENCE_MULTIPLIER)
const HEURISTIC_MAX_CONFIDENCE: f64 = 0.85;

/// Minimum total calls before flagging a tool's failure rate
const MIN_CALLS_FOR_FRICTION: usize = 5;

/// Failure rate threshold for friction detection
const FRICTION_FAILURE_RATE: f64 = 0.20;

/// Generate insights from tool history and memories without LLM
pub(super) fn generate_insights_heuristic(
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
