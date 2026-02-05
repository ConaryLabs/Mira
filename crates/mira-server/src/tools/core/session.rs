// crates/mira-server/src/tools/core/session.rs
// Unified session management and collaboration tools

use crate::db::{
    create_session_ext_sync, get_recent_sessions_sync, get_session_history_sync,
    get_unified_insights_sync,
};
use crate::hooks::session::{read_claude_session_id, read_source_info};
use crate::mcp::requests::SessionHistoryAction;
use crate::mcp::responses::Json;
use crate::mcp::responses::{
    HistoryEntry, InsightItem, InsightsData, ReplyOutput, SessionCurrentData, SessionData,
    SessionHistoryData, SessionListData, SessionOutput, SessionSummary,
};
use crate::tools::core::ToolContext;
use crate::utils::{truncate, truncate_at_boundary};
use mira_types::{AgentRole, WsEvent};
use uuid::Uuid;

/// Unified session tool dispatcher
pub async fn handle_session<C: ToolContext>(
    ctx: &C,
    req: crate::mcp::requests::SessionRequest,
) -> Result<Json<SessionOutput>, String> {
    use crate::mcp::requests::SessionAction;
    match req.action {
        SessionAction::History => {
            let history_action = req
                .history_action
                .ok_or("history_action is required for action 'history'")?;
            session_history(ctx, history_action, req.session_id, req.limit).await
        }
        SessionAction::Recap => {
            let message = super::get_session_recap(ctx).await?;
            Ok(Json(SessionOutput {
                action: "recap".into(),
                message,
                data: None,
            }))
        }
        SessionAction::Usage => {
            let usage_action = req
                .usage_action
                .ok_or("usage_action is required for action 'usage'")?;
            let message =
                super::usage(ctx, usage_action, req.group_by, req.since_days, req.limit).await?;
            Ok(Json(SessionOutput {
                action: "usage".into(),
                message,
                data: None,
            }))
        }
        SessionAction::Insights => {
            query_insights(ctx, req.insight_source, req.min_confidence, req.limit).await
        }
        SessionAction::Tasks => {
            // Handled at router level (returns TasksOutput, not SessionOutput).
            // This arm should never be reached.
            Err("Tasks action is handled at the router level. Use session(action=\"tasks\") via MCP.".into())
        }
    }
}

/// Query unified insights digest
async fn query_insights<C: ToolContext>(
    ctx: &C,
    insight_source: Option<String>,
    min_confidence: Option<f64>,
    limit: Option<i64>,
) -> Result<Json<SessionOutput>, String> {
    let project = ctx.get_project().await;
    let project_id = project.as_ref().map(|p| p.id).ok_or("No active project")?;

    let filter_source = insight_source.clone();
    let min_conf = min_confidence.unwrap_or(0.3);
    let lim = limit.unwrap_or(20) as usize;

    let insights = ctx
        .pool()
        .run(move |conn| {
            get_unified_insights_sync(
                conn,
                project_id,
                filter_source.as_deref(),
                min_conf,
                30,
                lim,
            )
        })
        .await?;

    if insights.is_empty() {
        return Ok(Json(SessionOutput {
            action: "insights".into(),
            message: "No insights found.".into(),
            data: Some(SessionData::Insights(InsightsData {
                insights: vec![],
                total: 0,
            })),
        }));
    }

    let mut output = format!("{} insights:\n\n", insights.len());
    let items: Vec<InsightItem> = insights
        .iter()
        .map(|insight| {
            output.push_str(&format!(
                "• [{}] {} (score: {:.2}, confidence: {:.0}%)\n",
                insight.source,
                insight.description,
                insight.priority_score,
                insight.confidence * 100.0,
            ));
            if let Some(ref evidence) = insight.evidence {
                output.push_str(&format!("  Evidence: {}\n", evidence));
            }
            output.push_str(&format!(
                "  Type: {} | {}\n\n",
                insight.source, insight.source_type
            ));
            InsightItem {
                source: insight.source.clone(),
                source_type: insight.source_type.clone(),
                description: insight.description.clone(),
                priority_score: insight.priority_score,
                confidence: insight.confidence,
                evidence: insight.evidence.clone(),
            }
        })
        .collect();
    let total = items.len();
    Ok(Json(SessionOutput {
        action: "insights".into(),
        message: output,
        data: Some(SessionData::Insights(InsightsData {
            insights: items,
            total,
        })),
    }))
}

/// Query session history
pub async fn session_history<C: ToolContext>(
    ctx: &C,
    action: SessionHistoryAction,
    session_id: Option<String>,
    limit: Option<i64>,
) -> Result<Json<SessionOutput>, String> {
    let limit = limit.unwrap_or(20) as usize;

    match action {
        SessionHistoryAction::Current => {
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
        SessionHistoryAction::ListSessions => {
            let project = ctx.get_project().await;
            let project_id = project.as_ref().map(|p| p.id).ok_or("No active project")?;

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
                        (Some(src), Some(from)) => format!(" [{}←{}]", src, truncate_at_boundary(from, 8)),
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
                    let status = if entry.success { "✓" } else { "✗" };
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

/// Ensure a session exists in database and return session ID
pub async fn ensure_session<C: ToolContext>(ctx: &C) -> Result<String, String> {
    // Check if session ID already exists in context
    if let Some(existing_id) = ctx.get_session_id().await {
        return Ok(existing_id);
    }

    // Read Claude's session ID (prefer over generating new)
    let session_id = read_claude_session_id().unwrap_or_else(|| Uuid::new_v4().to_string());

    // Read source info from hook
    let source_info = read_source_info();
    let source = source_info
        .as_ref()
        .map(|s| s.source.as_str())
        .unwrap_or("startup");

    // Get project ID if available
    let project_id = ctx.project_id().await;

    // Determine resumed_from for resume source
    let resumed_from = if source == "resume" {
        find_previous_session_heuristic(ctx, project_id).await
    } else {
        None
    };

    // Create/reactivate session using extended function
    let sid = session_id.clone();
    let src = source.to_string();
    let rf = resumed_from.clone();
    ctx.pool()
        .run(move |conn| create_session_ext_sync(conn, &sid, project_id, Some(&src), rf.as_deref()))
        .await?;

    // Set session ID in context
    ctx.set_session_id(session_id.clone()).await;

    Ok(session_id)
}

/// Find previous session using branch-aware heuristic
/// Prioritizes: same branch + recent + has tool history
async fn find_previous_session_heuristic<C: ToolContext>(
    ctx: &C,
    project_id: Option<i64>,
) -> Option<String> {
    let project_id = project_id?;
    let branch = ctx.get_branch().await;

    ctx.pool()
        .run(move |conn| {
            // Prioritize: same branch + recent + has tool history
            let sql = r#"
                SELECT s.id FROM sessions s
                LEFT JOIN tool_history t ON t.session_id = s.id
                WHERE s.project_id = ?1
                  AND s.status = 'completed'
                  AND (?2 IS NULL OR s.branch = ?2)
                  AND s.last_activity > datetime('now', '-24 hours')
                GROUP BY s.id
                ORDER BY COUNT(t.id) DESC, s.last_activity DESC
                LIMIT 1
            "#;
            let result: Option<String> = conn
                .query_row(sql, rusqlite::params![project_id, branch], |row| row.get(0))
                .ok();
            Ok::<_, String>(result)
        })
        .await
        .ok()
        .flatten()
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
) -> Result<Json<ReplyOutput>, String> {
    // Check if we have pending_responses (MCP with active collaboration)
    let Some(pending) = ctx.pending_responses() else {
        // CLI mode or no collaboration active - just acknowledge
        return Ok(Json(ReplyOutput {
            action: "reply".into(),
            message: format!(
                "(Reply not sent - no frontend connected) Content: {}",
                content
            ),
        }));
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

            Ok(Json(ReplyOutput {
                action: "reply".into(),
                message: "Response sent to Mira".into(),
            }))
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
                Ok(Json(ReplyOutput {
                    action: "reply".into(),
                    message: format!("(Reply not sent - no pending request) Content: {}", content),
                }))
            }
        }
    }
}
