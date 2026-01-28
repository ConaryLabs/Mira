// crates/mira-server/src/background/session_summaries.rs
// Background worker for closing stale sessions and generating summaries

use crate::db::pool::DatabasePool;
use crate::db::{
    close_session_sync, get_session_tool_summary_sync, get_sessions_needing_summary_sync,
    get_stale_sessions_sync, update_session_summary_sync,
};
use crate::llm::{LlmClient, PromptBuilder, record_llm_usage};
use std::sync::Arc;

/// Minutes of inactivity before a session is considered stale
const STALE_SESSION_MINUTES: i64 = 30;

/// Minimum tool calls required to generate a summary (otherwise just close)
const MIN_TOOLS_FOR_SUMMARY: i64 = 3;

/// Process stale sessions: close them and optionally generate summaries
/// Also generates summaries for already-closed sessions that don't have one
pub async fn process_stale_sessions(
    pool: &Arc<DatabasePool>,
    client: &Arc<dyn LlmClient>,
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
    client: &Arc<dyn LlmClient>,
) -> Result<usize, String> {
    let stale = pool
        .interact(move |conn| {
            get_stale_sessions_sync(conn, STALE_SESSION_MINUTES)
                .map_err(|e| anyhow::anyhow!("Failed to get stale sessions: {}", e))
        })
        .await
        .map_err(|e| e.to_string())?;

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
            .interact(move |conn| {
                close_session_sync(conn, &session_id_clone, summary_clone.as_deref())
                    .map_err(|e| anyhow::anyhow!("Failed to close session: {}", e))
            })
            .await
        {
            tracing::warn!("Failed to close session {}: {}", &session_id[..8], e);
            continue;
        }

        let summary_status = if summary.is_some() {
            "with summary"
        } else {
            "no summary"
        };
        tracing::info!(
            "Closed stale session {} ({} tools, {})",
            &session_id[..8.min(session_id.len())],
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
    client: &Arc<dyn LlmClient>,
) -> Result<usize, String> {
    let sessions = pool
        .interact(move |conn| {
            get_sessions_needing_summary_sync(conn)
                .map_err(|e| anyhow::anyhow!("Failed to get sessions needing summary: {}", e))
        })
        .await
        .map_err(|e| e.to_string())?;

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
                .interact(move |conn| {
                    update_session_summary_sync(conn, &session_id_clone, &summary_clone)
                        .map_err(|e| anyhow::anyhow!("Failed to update summary: {}", e))
                })
                .await
            {
                tracing::warn!(
                    "Failed to update summary for session {}: {}",
                    &session_id[..8],
                    e
                );
                continue;
            }

            tracing::info!(
                "Generated summary for session {}",
                &session_id[..8.min(session_id.len())]
            );
            processed += 1;
        }
    }

    Ok(processed)
}

/// Generate a summary of the session using LLM
async fn generate_session_summary(
    pool: &Arc<DatabasePool>,
    client: &Arc<dyn LlmClient>,
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
        return None;
    }

    // Build prompt for summarization
    let prompt = format!(
        r#"Summarize this Claude Code session in 1-2 concise sentences.
Focus on: what was accomplished, which areas of code were worked on.
Be specific but brief. No preamble, just the summary.

Tool calls in session:
{}

Summary:"#,
        tool_summary
    );

    let messages = PromptBuilder::for_briefings().build_messages(prompt);

    // Generate summary
    match client.chat(messages, None).await {
        Ok(result) => {
            // Record usage
            record_llm_usage(
                pool,
                client.provider_type(),
                &client.model_name(),
                "background:session_summary",
                &result,
                project_id,
                None,
            )
            .await;

            let summary = result.content.as_deref().unwrap_or("").trim().to_string();
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
        .interact(move |conn| {
            conn.query_row(
                "SELECT COUNT(*) FROM tool_history WHERE session_id = ?",
                [&session_id_clone],
                |row| row.get(0),
            )
            .map_err(|e| anyhow::anyhow!("Failed to count tools: {}", e))
        })
        .await
        .map_err(|e| e.to_string())?;

    // Generate summary if we have a client and enough tool calls
    let summary = if let Some(client) = client {
        if tool_count >= MIN_TOOLS_FOR_SUMMARY {
            generate_session_summary(pool, client, session_id, project_id).await
        } else {
            None
        }
    } else {
        None
    };

    // Close the session
    let session_id_clone = session_id.to_string();
    let summary_clone = summary.clone();
    pool.interact(move |conn| {
        close_session_sync(conn, &session_id_clone, summary_clone.as_deref())
            .map_err(|e| anyhow::anyhow!("Failed to close session: {}", e))
    })
    .await
    .map_err(|e| e.to_string())?;

    Ok(summary)
}
