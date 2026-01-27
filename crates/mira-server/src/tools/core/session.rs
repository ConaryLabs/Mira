// crates/mira-server/src/tools/core/session.rs
// Unified session management and collaboration tools

use crate::db::{create_session_sync, get_recent_sessions_sync, get_session_history_sync};
use crate::mcp::requests::SessionHistoryAction;
use crate::tools::core::ToolContext;
use mira_types::{AgentRole, WsEvent};
use uuid::Uuid;

/// Query session history
pub async fn session_history<C: ToolContext>(
    ctx: &C,
    action: SessionHistoryAction,
    session_id: Option<String>,
    limit: Option<i64>,
) -> Result<String, String> {
    let limit = limit.unwrap_or(20) as usize;

    match action {
        SessionHistoryAction::Current => {
            let session_id = ctx.get_session_id().await;
            match session_id {
                Some(id) => Ok(format!("Current session: {}", id)),
                None => Ok("No active session".to_string()),
            }
        }
        SessionHistoryAction::ListSessions => {
            let project = ctx.get_project().await;
            let project_id = project.as_ref().map(|p| p.id).ok_or("No active project")?;

            let sessions = ctx
                .pool()
                .run(move |conn| get_recent_sessions_sync(conn, project_id, limit))
                .await?;

            if sessions.is_empty() {
                return Ok("No sessions found.".to_string());
            }

            let mut output = format!("{} sessions:\n", sessions.len());
            for s in sessions {
                output.push_str(&format!(
                    "  [{}] {} - {} ({} tool calls)\n",
                    &s.id[..8],
                    s.started_at,
                    s.status,
                    s.summary.as_deref().unwrap_or("no summary")
                ));
            }
            Ok(output)
        }
        SessionHistoryAction::GetHistory => {
            // Use provided session_id or fall back to current session
            let target_session_id = match session_id {
                Some(id) => id,
                None => ctx
                    .get_session_id()
                    .await
                    .ok_or("No session_id provided and no active session")?,
            };

            let session_id_clone = target_session_id.clone();
            let history = ctx
                .pool()
                .run(move |conn| get_session_history_sync(conn, &session_id_clone, limit))
                .await?;

            if history.is_empty() {
                return Ok(format!(
                    "No history for session {}",
                    &target_session_id[..8]
                ));
            }

            let mut output = format!(
                "{} tool calls in session {}:\n",
                history.len(),
                &target_session_id[..8]
            );
            for entry in history {
                let status = if entry.success { "✓" } else { "✗" };
                let preview = entry
                    .result_summary
                    .as_ref()
                    .map(|s| {
                        if s.len() > 60 {
                            format!("{}...", &s[..60])
                        } else {
                            s.clone()
                        }
                    })
                    .unwrap_or_default();
                output.push_str(&format!(
                    "  {} {} [{}] {}\n",
                    status, entry.tool_name, entry.created_at, preview
                ));
            }
            Ok(output)
        }
    }
}

/// Ensure a session exists in database and return session ID
pub async fn ensure_session<C: ToolContext>(ctx: &C) -> Result<String, String> {
    // Check if session ID already exists
    if let Some(existing_id) = ctx.get_session_id().await {
        return Ok(existing_id);
    }

    // Generate new session ID
    let new_id = Uuid::new_v4().to_string();

    // Get project ID if available
    let project_id = ctx.project_id().await;

    // Create session in database
    let new_id_clone = new_id.clone();
    ctx.pool()
        .run(move |conn| create_session_sync(conn, &new_id_clone, project_id))
        .await?;

    // Set session ID in context
    ctx.set_session_id(new_id.clone()).await;

    Ok(new_id)
}

/// Send a response back to Mira during collaboration.
///
/// In MCP mode with WebSocket: Sends the response through the pending channel
/// and broadcasts an AgentResponse event to the frontend.
///
/// In CLI mode: Returns a message indicating no frontend is connected.
pub async fn reply_to_mira<C: ToolContext>(
    ctx: &C,
    in_reply_to: String,
    content: String,
    complete: bool,
) -> Result<String, String> {
    // Check if we have pending_responses (MCP with active collaboration)
    let Some(pending) = ctx.pending_responses() else {
        // CLI mode or no collaboration active - just acknowledge
        return Ok(format!(
            "(Reply not sent - no frontend connected) Content: {}",
            content
        ));
    };

    // Try to find and fulfill the pending response
    let sender = {
        let mut pending_map = pending.write().await;
        pending_map.remove(&in_reply_to)
    };

    match sender {
        Some(tx) => {
            // Send response through the channel
            if tx.send(content.clone()).is_err() {
                return Err("Response channel was closed".to_string());
            }

            // Broadcast AgentResponse event for frontend
            ctx.broadcast(WsEvent::AgentResponse {
                in_reply_to,
                from: AgentRole::Claude,
                content,
                complete,
            });

            Ok("Response sent to Mira".to_string())
        }
        None => {
            // No pending request found
            if ctx.is_collaborative() {
                // Server mode: This is an error (request timed out or invalid ID)
                Err(format!(
                    "No pending request found for message_id: {}. It may have timed out or been answered already.",
                    in_reply_to
                ))
            } else {
                // CLI/Offline mode: Log it and succeed
                Ok(format!(
                    "(Reply not sent - no pending request) Content: {}",
                    content
                ))
            }
        }
    }
}
