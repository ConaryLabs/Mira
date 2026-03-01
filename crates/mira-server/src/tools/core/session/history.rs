// crates/mira-server/src/tools/core/session/history.rs
//! Session history queries: current session, list sessions, get tool history.

use crate::db::{get_recent_sessions_sync, get_session_history_scoped_sync};
use crate::error::MiraError;
use crate::mcp::responses::Json;
use crate::mcp::responses::{
    HistoryEntry, SessionCurrentData, SessionData, SessionHistoryData, SessionListData,
    SessionOutput, SessionSummary,
};
use crate::tools::core::{ToolContext, require_project_id};
use crate::utils::{truncate, truncate_at_boundary};

/// Internal kind enum for session history queries (replaces deleted SessionHistoryAction)
pub enum HistoryKind {
    Current,
    List,
    GetHistory,
}

/// Query session history
pub async fn session_history<C: ToolContext>(
    ctx: &C,
    action: HistoryKind,
    session_id: Option<String>,
    limit: Option<i64>,
) -> Result<Json<SessionOutput>, MiraError> {
    let limit = limit.unwrap_or(20).max(0) as usize;

    match action {
        HistoryKind::Current => {
            let session_id = ctx.get_session_id().await;
            match session_id {
                Some(id) => Ok(Json(SessionOutput {
                    action: "current".into(),
                    message: format!("Current session: {}", id),
                    data: Some(SessionData::Current(SessionCurrentData { session_id: id })),
                })),
                None => Ok(Json(SessionOutput {
                    action: "current".into(),
                    message: "No active session".into(),
                    data: None,
                })),
            }
        }
        HistoryKind::List => {
            let project_id = require_project_id(ctx).await?;

            let sessions = ctx
                .pool()
                .run(move |conn| get_recent_sessions_sync(conn, project_id, limit))
                .await?;

            if sessions.is_empty() {
                return Ok(Json(SessionOutput {
                    action: "list_sessions".into(),
                    message: "No sessions found.".into(),
                    data: Some(SessionData::ListSessions(SessionListData {
                        sessions: vec![],
                        total: 0,
                    })),
                }));
            }

            let mut output = format!("{} sessions:\n", sessions.len());
            let items: Vec<SessionSummary> = sessions
                .into_iter()
                .map(|s| {
                    let source_info = match (&s.source, &s.resumed_from) {
                        (Some(src), Some(from)) => {
                            format!(" [{}←{}]", src, truncate_at_boundary(from, 8))
                        }
                        (Some(src), None) => format!(" [{}]", src),
                        _ => String::new(),
                    };
                    output.push_str(&format!(
                        "  [{}] {} - {}{} ({})\n",
                        truncate_at_boundary(&s.id, 8),
                        s.started_at,
                        s.status,
                        source_info,
                        s.summary.as_deref().unwrap_or("no summary")
                    ));
                    SessionSummary {
                        id: s.id,
                        started_at: s.started_at,
                        status: s.status,
                        summary: s.summary,
                        source: s.source,
                        resumed_from: s.resumed_from,
                    }
                })
                .collect();
            let total = items.len();
            Ok(Json(SessionOutput {
                action: "list_sessions".into(),
                message: output,
                data: Some(SessionData::ListSessions(SessionListData {
                    sessions: items,
                    total,
                })),
            }))
        }
        HistoryKind::GetHistory => {
            // Use provided session_id or fall back to current session
            let target_session_id = match session_id {
                Some(id) => id,
                None => ctx.get_session_id().await.ok_or_else(|| {
                    MiraError::InvalidInput(
                        "No session_id provided and no active session".to_string(),
                    )
                })?,
            };

            let project_id = require_project_id(ctx).await?;
            let session_id_clone = target_session_id.clone();
            let history = ctx
                .pool()
                .run(move |conn| {
                    get_session_history_scoped_sync(
                        conn,
                        &session_id_clone,
                        Some(project_id),
                        limit,
                    )
                })
                .await?;

            if history.is_empty() {
                return Ok(Json(SessionOutput {
                    action: "get_history".into(),
                    message: format!(
                        "No history for session {}",
                        truncate_at_boundary(&target_session_id, 8)
                    ),
                    data: Some(SessionData::History(SessionHistoryData {
                        session_id: target_session_id,
                        entries: vec![],
                        total: 0,
                    })),
                }));
            }

            let mut output = format!(
                "{} tool calls in session {}:\n",
                history.len(),
                truncate_at_boundary(&target_session_id, 8)
            );
            let items: Vec<HistoryEntry> = history
                .into_iter()
                .map(|entry| {
                    let status = if entry.success { "[ok]" } else { "[err]" };
                    let preview = entry
                        .result_summary
                        .as_ref()
                        .map(|s| truncate(s, 60))
                        .unwrap_or_default();
                    output.push_str(&format!(
                        "  {} {} [{}] {}\n",
                        status, entry.tool_name, entry.created_at, preview
                    ));
                    HistoryEntry {
                        tool_name: entry.tool_name,
                        created_at: entry.created_at,
                        success: entry.success,
                        result_preview: entry.result_summary.map(|s| truncate(&s, 60)),
                    }
                })
                .collect();
            let total = items.len();
            Ok(Json(SessionOutput {
                action: "get_history".into(),
                message: output,
                data: Some(SessionData::History(SessionHistoryData {
                    session_id: target_session_id,
                    entries: items,
                    total,
                })),
            }))
        }
    }
}
