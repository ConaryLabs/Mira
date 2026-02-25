// tools/core/project/formatting.rs
// Display formatting for session history and insights

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

/// Format doc tasks for display.
pub(super) fn format_session_insights(doc_task_counts: &[(String, i64)]) -> String {
    let mut out = String::new();

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
