// crates/mira-server/src/tools/core/session.rs
// Unified session management tools

use crate::db::{create_session_sync, get_recent_sessions_sync, get_session_history_sync};
use crate::tools::core::ToolContext;
use uuid::Uuid;

/// Query session history
pub async fn session_history<C: ToolContext>(
    ctx: &C,
    action: String,
    session_id: Option<String>,
    limit: Option<i64>,
) -> Result<String, String> {
    let limit = limit.unwrap_or(20) as usize;

    match action.as_str() {
        "current" => {
            let session_id = ctx.get_session_id().await;
            match session_id {
                Some(id) => Ok(format!("Current session: {}", id)),
                None => Ok("No active session".to_string()),
            }
        }
        "list_sessions" => {
            let project = ctx.get_project().await;
            let project_id = project.as_ref().map(|p| p.id).ok_or("No active project")?;

            let sessions = ctx
                .pool()
                .interact(move |conn| {
                    get_recent_sessions_sync(conn, project_id, limit)
                        .map_err(|e| anyhow::anyhow!("{}", e))
                })
                .await
                .map_err(|e| e.to_string())?;

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
        "get_history" => {
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
                .interact(move |conn| {
                    get_session_history_sync(conn, &session_id_clone, limit)
                        .map_err(|e| anyhow::anyhow!("{}", e))
                })
                .await
                .map_err(|e| e.to_string())?;

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
        _ => Err(format!(
            "Unknown action: {}. Use: list_sessions, get_history, current",
            action
        )),
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
        .interact(move |conn| {
            create_session_sync(conn, &new_id_clone, project_id)
                .map_err(|e| anyhow::anyhow!("{}", e))
        })
        .await
        .map_err(|e| e.to_string())?;

    // Set session ID in context
    ctx.set_session_id(new_id.clone()).await;

    Ok(new_id)
}
