// crates/mira-server/src/background/session_summaries.rs
// Background worker for closing stale sessions and generating summaries

use super::HEURISTIC_PREFIX;
use crate::db::pool::DatabasePool;
use crate::db::{
    close_session_sync, get_session_behavior_summary_sync, get_session_tool_summary_sync,
    get_sessions_needing_summary_sync, get_stale_sessions_sync, update_session_summary_sync,
};
use crate::llm::{LlmClient, PromptBuilder, chat_with_usage};
use crate::utils::truncate_at_boundary;
use std::collections::HashMap;
use std::sync::Arc;

/// Minutes of inactivity before a session is considered stale
const STALE_SESSION_MINUTES: i64 = 30;

/// Minimum tool calls required to generate a summary (otherwise just close)
const MIN_TOOLS_FOR_SUMMARY: i64 = 3;

/// Max files to list in heuristic summary
const MAX_FILES_IN_SUMMARY: usize = 5;

/// Process stale sessions: close them and optionally generate summaries
/// Also generates summaries for already-closed sessions that don't have one
pub async fn process_stale_sessions(
    pool: &Arc<DatabasePool>,
    client: Option<&Arc<dyn LlmClient>>,
) -> Result<usize, String> {
    let mut processed = 0;

    // First, close stale active sessions
    processed += close_stale_sessions(pool, client).await?;

    // Then, generate summaries for completed sessions that need them
    processed += generate_missing_summaries(pool, client).await?;

    Ok(processed)
}

/// Close stale active sessions
async fn close_stale_sessions(
    pool: &Arc<DatabasePool>,
    client: Option<&Arc<dyn LlmClient>>,
) -> Result<usize, String> {
    let stale = pool
        .run(move |conn| get_stale_sessions_sync(conn, STALE_SESSION_MINUTES))
        .await?;

    if stale.is_empty() {
        return Ok(0);
    }

    let mut processed = 0;

    for (session_id, project_id, tool_count) in stale {
        // If session has enough tool calls, generate a summary
        let summary = if tool_count >= MIN_TOOLS_FOR_SUMMARY {
            generate_session_summary(pool, client, &session_id, project_id).await
        } else {
            None
        };

        // Close the session
        let session_id_clone = session_id.clone();
        let summary_clone = summary.clone();
        if let Err(e) = pool
            .run(move |conn| close_session_sync(conn, &session_id_clone, summary_clone.as_deref()))
            .await
        {
            tracing::warn!(
                "Failed to close session {}: {}",
                truncate_at_boundary(&session_id, 8),
                e
            );
            continue;
        }

        let summary_status = if summary.is_some() {
            "with summary"
        } else {
            "no summary"
        };
        tracing::info!(
            "Closed stale session {} ({} tools, {})",
            truncate_at_boundary(&session_id, 8),
            tool_count,
            summary_status
        );
        processed += 1;
    }

    Ok(processed)
}

/// Generate summaries for completed sessions that don't have one
async fn generate_missing_summaries(
    pool: &Arc<DatabasePool>,
    client: Option<&Arc<dyn LlmClient>>,
) -> Result<usize, String> {
    let sessions = pool.run(get_sessions_needing_summary_sync).await?;

    if sessions.is_empty() {
        return Ok(0);
    }

    let mut processed = 0;

    for (session_id, project_id, _tool_count) in sessions {
        if let Some(summary) = generate_session_summary(pool, client, &session_id, project_id).await
        {
            let session_id_clone = session_id.clone();
            let summary_clone = summary.clone();
            if let Err(e) = pool
                .run(move |conn| {
                    update_session_summary_sync(conn, &session_id_clone, &summary_clone)
                })
                .await
            {
                tracing::warn!(
                    "Failed to update summary for session {}: {}",
                    truncate_at_boundary(&session_id, 8),
                    e
                );
                continue;
            }

            tracing::info!(
                "Generated summary for session {}",
                truncate_at_boundary(&session_id, 8)
            );
            processed += 1;
        }
    }

    Ok(processed)
}

/// Generate a summary of the session — LLM when available, heuristic fallback otherwise
async fn generate_session_summary(
    pool: &Arc<DatabasePool>,
    client: Option<&Arc<dyn LlmClient>>,
    session_id: &str,
    project_id: Option<i64>,
) -> Option<String> {
    // Get tool history summary
    let session_id_clone = session_id.to_string();
    let tool_summary = pool
        .interact(move |conn| {
            get_session_tool_summary_sync(conn, &session_id_clone)
                .map_err(|e| anyhow::anyhow!("Failed to get tool summary: {}", e))
        })
        .await
        .ok()?;

    if tool_summary.is_empty() {
        // Fallback: try behavior log
        let session_id_clone2 = session_id.to_string();
        let behavior_summary = pool
            .interact(move |conn| {
                get_session_behavior_summary_sync(conn, &session_id_clone2)
                    .map_err(|e| anyhow::anyhow!("Failed to get behavior summary: {}", e))
            })
            .await
            .ok()?;

        if behavior_summary.is_empty() {
            return None;
        }

        return match client {
            Some(client) => {
                generate_session_summary_llm(pool, client, &behavior_summary, project_id).await
            }
            None => generate_session_summary_fallback(&behavior_summary),
        };
    }

    match client {
        Some(client) => generate_session_summary_llm(pool, client, &tool_summary, project_id).await,
        None => generate_session_summary_fallback(&tool_summary),
    }
}

/// Generate session summary using LLM
async fn generate_session_summary_llm(
    pool: &Arc<DatabasePool>,
    client: &Arc<dyn LlmClient>,
    tool_summary: &str,
    project_id: Option<i64>,
) -> Option<String> {
    let prompt = format!(
        r#"Summarize this Claude Code session in 1-2 concise sentences.

Focus on USER-FACING WORK: code written, bugs fixed, features added, files modified, questions answered.
De-emphasize or skip internal housekeeping: project switching, indexing, recall/remember operations, goal management.

If the session was mostly housekeeping with no substantive user work, respond with just: "Housekeeping session"

Be specific but brief. No preamble, just the summary.

Tool calls in session:
{}

Summary:"#,
        tool_summary
    );

    let messages = PromptBuilder::for_briefings().build_messages(prompt);

    match chat_with_usage(
        &**client,
        pool,
        messages,
        "background:session_summary",
        project_id,
        None,
    )
    .await
    {
        Ok(content) => {
            let summary = content.trim().to_string();
            if summary.is_empty() || summary.len() < 10 {
                None
            } else {
                Some(summary)
            }
        }
        Err(e) => {
            tracing::warn!("Failed to generate session summary: {}", e);
            None
        }
    }
}

/// Generate a heuristic session summary from tool usage (no LLM required)
fn generate_session_summary_fallback(tool_summary: &str) -> Option<String> {
    let mut tool_counts: HashMap<&str, usize> = HashMap::new();
    let mut files: HashMap<String, usize> = HashMap::new();
    let mut total_calls = 0usize;

    for line in tool_summary.lines() {
        total_calls += 1;
        // Lines are formatted as: "✓ ToolName(args) -> result" or "✗ ToolName(args) -> result"
        let line = line.trim();
        let line = if line.starts_with('✓') || line.starts_with('✗') {
            line[line.char_indices().nth(1).map(|(i, _)| i).unwrap_or(0)..].trim()
        } else {
            line
        };

        // Extract tool name (everything before the first '(')
        let tool_name = line.split('(').next().unwrap_or("").trim();
        if !tool_name.is_empty() {
            *tool_counts.entry(tool_name).or_default() += 1;
        }

        // Extract file paths from known path patterns in arguments
        // Look for file_path arguments (safe — no raw secrets)
        if let Some(args_start) = line.find('(')
            && let Some(args_end) = line.rfind(')')
        {
            let args = &line[args_start + 1..args_end];
            // Extract paths that look like file paths
            for segment in args.split(',') {
                let segment = segment.trim().trim_matches('"');
                if looks_like_file_path(segment) {
                    let short = shorten_path(segment);
                    *files.entry(short).or_default() += 1;
                }
            }
        }
    }

    if total_calls == 0 {
        return None;
    }

    // Detect session type from tool mix
    let edit_write = tool_counts.get("Edit").copied().unwrap_or(0)
        + tool_counts.get("Write").copied().unwrap_or(0);
    let read_search = tool_counts.get("Read").copied().unwrap_or(0)
        + tool_counts.get("Grep").copied().unwrap_or(0)
        + tool_counts.get("Glob").copied().unwrap_or(0);
    let bash = tool_counts.get("Bash").copied().unwrap_or(0);

    let session_type = if edit_write > read_search && edit_write > bash {
        "Coding session"
    } else if read_search > edit_write && read_search > bash {
        "Exploration session"
    } else if bash > edit_write && bash > read_search {
        "DevOps session"
    } else {
        "Development session"
    };

    // Top tools by usage
    let mut top_tools: Vec<(&&str, &usize)> = tool_counts.iter().collect();
    top_tools.sort_by(|a, b| b.1.cmp(a.1));
    let tool_names: Vec<&str> = top_tools.iter().take(4).map(|(name, _)| **name).collect();

    // Top files by mention count
    let mut top_files: Vec<(&String, &usize)> = files.iter().collect();
    top_files.sort_by(|a, b| b.1.cmp(a.1));
    let file_names: Vec<&str> = top_files
        .iter()
        .take(MAX_FILES_IN_SUMMARY)
        .map(|(name, _)| name.as_str())
        .collect();

    let mut summary = format!(
        "{}{}: Used {} ({} calls)",
        HEURISTIC_PREFIX,
        session_type,
        tool_names.join(", "),
        total_calls,
    );

    if !file_names.is_empty() {
        let extra = if files.len() > MAX_FILES_IN_SUMMARY {
            format!(" (+{} more)", files.len() - MAX_FILES_IN_SUMMARY)
        } else {
            String::new()
        };
        summary.push_str(&format!(". Files: {}{}", file_names.join(", "), extra));
    }

    Some(summary)
}

/// Check if a string segment looks like a file path
fn looks_like_file_path(s: &str) -> bool {
    // Must contain a slash or dot-extension, and be reasonably short
    s.len() > 2
        && s.len() < 200
        && !s.contains(' ')
        && (s.contains('/') || s.contains('.'))
        && !s.starts_with("http")
        && !s.contains("://")
}

/// Shorten a file path to just the filename (or last 2 components)
fn shorten_path(path: &str) -> String {
    let parts: Vec<&str> = path.split('/').collect();
    if parts.len() <= 2 {
        path.to_string()
    } else {
        parts[parts.len() - 2..].join("/")
    }
}

/// Close a specific session immediately (for stop hook)
/// This is synchronous-friendly - called from hook context
pub async fn close_session_now(
    pool: &Arc<DatabasePool>,
    client: Option<&Arc<dyn LlmClient>>,
    session_id: &str,
    project_id: Option<i64>,
) -> Result<Option<String>, String> {
    // Check tool count
    let session_id_clone = session_id.to_string();
    let tool_count: i64 = pool
        .run(move |conn| {
            conn.query_row(
                "SELECT COUNT(*) FROM tool_history WHERE session_id = ?",
                [&session_id_clone],
                |row| row.get(0),
            )
        })
        .await?;

    // Generate summary if enough tool calls (LLM or fallback)
    let summary = if tool_count >= MIN_TOOLS_FOR_SUMMARY {
        generate_session_summary(pool, client, session_id, project_id).await
    } else {
        // Fallback: check behavior log
        let session_id_clone2 = session_id.to_string();
        let behavior_count: i64 = pool
            .run(move |conn| {
                conn.query_row(
                    "SELECT COUNT(*) FROM session_behavior_log WHERE session_id = ? AND event_type = 'tool_use'",
                    [&session_id_clone2],
                    |row| row.get(0),
                )
            })
            .await
            .unwrap_or(0);

        if behavior_count >= MIN_TOOLS_FOR_SUMMARY {
            generate_session_summary(pool, client, session_id, project_id).await
        } else {
            None
        }
    };

    // Close the session
    let session_id_clone = session_id.to_string();
    let summary_clone = summary.clone();
    pool.run(move |conn| close_session_sync(conn, &session_id_clone, summary_clone.as_deref()))
        .await?;

    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fallback_summary_coding_session() {
        let tool_summary = "\
✓ Read(src/main.rs) -> ok
✓ Edit(src/main.rs) -> ok
✓ Edit(src/lib.rs) -> ok
✓ Write(src/new_file.rs) -> ok
✓ Bash(cargo build) -> ok";

        let summary = generate_session_summary_fallback(tool_summary).unwrap();
        assert!(summary.starts_with(HEURISTIC_PREFIX));
        assert!(summary.contains("Coding session"));
        assert!(summary.contains("Edit"));
    }

    #[test]
    fn test_fallback_summary_exploration_session() {
        let tool_summary = "\
✓ Read(src/main.rs) -> ok
✓ Grep(pattern) -> ok
✓ Glob(**/*.rs) -> ok
✓ Read(src/lib.rs) -> ok
✓ Read(Cargo.toml) -> ok";

        let summary = generate_session_summary_fallback(tool_summary).unwrap();
        assert!(summary.starts_with(HEURISTIC_PREFIX));
        assert!(summary.contains("Exploration session"));
    }

    #[test]
    fn test_fallback_summary_empty() {
        let result = generate_session_summary_fallback("");
        assert!(result.is_none());
    }

    #[test]
    fn test_looks_like_file_path() {
        assert!(looks_like_file_path("src/main.rs"));
        assert!(looks_like_file_path("Cargo.toml"));
        assert!(!looks_like_file_path("https://example.com"));
        assert!(!looks_like_file_path("hi"));
        assert!(!looks_like_file_path("hello world"));
    }

    #[test]
    fn test_shorten_path() {
        assert_eq!(
            shorten_path("src/background/slow_lane.rs"),
            "background/slow_lane.rs"
        );
        assert_eq!(shorten_path("main.rs"), "main.rs");
        assert_eq!(shorten_path("src/lib.rs"), "src/lib.rs");
    }
}
