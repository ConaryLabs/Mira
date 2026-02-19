// tools/core/project/formatting.rs
// Display formatting for session history and insights

use mira_types::MemoryFact;

use crate::proactive::interventions;
use crate::utils::truncate;

use super::SessionInfo;

/// Format recent sessions for display
pub(super) fn format_recent_sessions(sessions: &[SessionInfo]) -> String {
    let mut out = String::from("\nRecent sessions:\n");
    for (sess_id, last_activity, summary, tool_count, tools) in sessions {
        let short_id = &sess_id[..8.min(sess_id.len())];
        let timestamp = &last_activity[..16.min(last_activity.len())];

        if let Some(sum) = summary {
            out.push_str(&format!("  [{}] {} - {}\n", short_id, timestamp, sum));
        } else if *tool_count > 0 {
            let tools_str = tools.join(", ");
            out.push_str(&format!(
                "  [{}] {} - {} tool calls ({})\n",
                short_id, timestamp, tool_count, tools_str
            ));
        } else {
            out.push_str(&format!("  [{}] {} - (no activity)\n", short_id, timestamp));
        }
    }
    out.push_str("  Use session(action=\"recap\") for current session context\n");
    out
}

/// Format preferences, context, health alerts, and interventions for display
pub(super) fn format_session_insights(
    preferences: &[MemoryFact],
    memories: &[MemoryFact],
    health_alerts: &[MemoryFact],
    pending_interventions: &[interventions::PendingIntervention],
    doc_task_counts: &[(String, i64)],
) -> String {
    let mut out = String::new();

    if !preferences.is_empty() {
        out.push_str("\nPreferences:\n");
        for pref in preferences {
            let category = pref.category.as_deref().unwrap_or("general");
            out.push_str(&format!("  [{}] {}\n", category, pref.content));
        }
    }

    let non_pref_memories: Vec<_> = memories
        .iter()
        .filter(|m| m.fact_type != "preference")
        .take(5)
        .collect();

    if !non_pref_memories.is_empty() {
        out.push_str("\nRecent context:\n");
        for mem in non_pref_memories {
            let preview = truncate(&mem.content, 80);
            out.push_str(&format!("  - {}\n", preview));
        }
    }

    if !health_alerts.is_empty() {
        out.push_str("\nHealth alerts:\n");
        for alert in health_alerts {
            let category = alert.category.as_deref().unwrap_or("issue");
            let preview = truncate(&alert.content, 100);
            out.push_str(&format!("  [{}] {}\n", category, preview));
        }
    }

    if !pending_interventions.is_empty() {
        out.push_str("\nInsights (from background analysis):\n");
        for intervention in pending_interventions {
            out.push_str(&format!("  {}\n", intervention.format()));
        }
    }

    let pending_doc_count = doc_task_counts
        .iter()
        .find(|(status, _)| status == "pending")
        .map(|(_, count)| *count)
        .unwrap_or(0);

    if pending_doc_count > 0 {
        out.push_str(&format!(
            "\nDocumentation: {} items need docs\n  CLI: `mira tool documentation '{{\"action\":\"list\"}}'`\n",
            pending_doc_count
        ));
    }

    out
}
