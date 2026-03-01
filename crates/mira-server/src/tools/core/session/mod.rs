// crates/mira-server/src/tools/core/session/mod.rs
//! Unified session management tools.

mod analytics;
mod history;
mod storage;

pub use history::{HistoryKind, session_history};

use crate::db::{build_session_recap_sync, create_session_ext_sync};
use crate::error::MiraError;
use crate::hooks::session::{read_claude_session_id, read_source_info};
use crate::mcp::responses::Json;
use crate::mcp::responses::SessionOutput;
use crate::tools::core::ToolContext;
use crate::tools::core::session_notes;
use uuid::Uuid;

/// Unified session tool dispatcher
pub async fn handle_session<C: ToolContext>(
    ctx: &C,
    req: crate::mcp::requests::SessionRequest,
) -> Result<Json<SessionOutput>, MiraError> {
    use crate::mcp::requests::SessionAction;
    match req.action {
        SessionAction::CurrentSession => {
            session_history(ctx, HistoryKind::Current, req.session_id, req.limit).await
        }
        SessionAction::ListSessions => {
            session_history(ctx, HistoryKind::List, req.session_id, req.limit).await
        }
        SessionAction::GetHistory => {
            session_history(ctx, HistoryKind::GetHistory, req.session_id, req.limit).await
        }
        SessionAction::Recap => {
            let message = get_session_recap(ctx).await?;
            Ok(Json(SessionOutput {
                action: "recap".into(),
                message,
                data: None,
            }))
        }
        SessionAction::UsageSummary => {
            let message = super::usage_summary(ctx, req.since_days).await?;
            Ok(Json(SessionOutput {
                action: "usage_summary".into(),
                message,
                data: None,
            }))
        }
        SessionAction::UsageStats => {
            let message = super::usage_stats(ctx, req.group_by, req.since_days, req.limit).await?;
            Ok(Json(SessionOutput {
                action: "usage_stats".into(),
                message,
                data: None,
            }))
        }
        SessionAction::UsageList => {
            let message = super::usage_list(ctx, req.since_days, req.limit).await?;
            Ok(Json(SessionOutput {
                action: "usage_list".into(),
                message,
                data: None,
            }))
        }
        SessionAction::Insights => {
            super::insights::query_insights(
                ctx,
                req.insight_source,
                req.min_confidence,
                req.since_days,
                req.limit,
            )
            .await
        }
        SessionAction::DismissInsight => {
            super::insights::dismiss_insight(ctx, req.insight_id, req.insight_source).await
        }
        SessionAction::StorageStatus => storage::storage_status(ctx).await,
        SessionAction::Cleanup => storage::cleanup(ctx, req.dry_run, req.category).await,
        SessionAction::ErrorPatterns => analytics::get_error_patterns(ctx, req.limit).await,
        SessionAction::SessionLineage => analytics::get_session_lineage(ctx, req.limit).await,
        SessionAction::Capabilities => analytics::get_capabilities(ctx).await,
        SessionAction::Report => {
            let message = get_injection_report(ctx, req.session_id).await?;
            Ok(Json(SessionOutput {
                action: "report".into(),
                message,
                data: None,
            }))
        }
    }
}

/// Ensure a session exists in database and return session ID
pub async fn ensure_session<C: ToolContext>(ctx: &C) -> Result<String, MiraError> {
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

/// Get session recap for MCP clients
/// Returns recent context, preferences, project state, and Claude Code session notes
pub async fn get_session_recap<C: ToolContext>(ctx: &C) -> Result<String, MiraError> {
    let project = ctx.get_project().await;
    let project_id = project.as_ref().map(|p| p.id);

    let mut recap = ctx
        .pool()
        .run(move |conn| Ok::<_, String>(build_session_recap_sync(conn, project_id)))
        .await?;

    // Add Claude Code session notes if available
    if let Some(proj) = &project {
        let notes = session_notes::get_recent_session_notes(&proj.path, 3);
        if !notes.is_empty() {
            recap.push_str(&session_notes::format_session_notes(&notes));
        }
    }

    if recap.is_empty() {
        Ok("No session recap available.".to_string())
    } else {
        Ok(recap)
    }
}

/// Get injection efficiency report for a session (or cumulative)
async fn get_injection_report<C: ToolContext>(
    ctx: &C,
    session_id: Option<String>,
) -> Result<String, MiraError> {
    let pool = ctx.pool();
    let project_id = ctx.project_id().await;

    let message = pool
        .run(move |conn| {
            let mut lines = Vec::new();

            if let Some(sid) = &session_id {
                // Session-specific report
                let stats = crate::db::injection::get_injection_stats_for_session(conn, sid)
                    .map_err(|e| e.to_string())?;

                if stats.total_injections == 0 {
                    return Ok::<_, String>("No injection data for this session.".to_string());
                }

                lines.push(format!(
                    "Session Injection Report ({})",
                    &sid[..sid.len().min(8)]
                ));
                lines.push(format!("  Injections: {}", stats.total_injections));
                lines.push(format!("  Total chars: {}", stats.total_chars));
                lines.push(format!("  Avg chars/injection: {:.0}", stats.avg_chars));
                lines.push(format!("  Deduped (suppressed): {}", stats.total_deduped));
                lines.push(format!("  Cache hits: {}", stats.total_cached));
                if let Some(ms) = stats.avg_latency_ms {
                    lines.push(format!("  Avg latency: {:.0}ms", ms));
                }

                // Efficiency ratio: non-deduped injections / total
                let effective = stats.total_injections - stats.total_deduped;
                let ratio = if stats.total_injections > 0 {
                    effective as f64 / stats.total_injections as f64 * 100.0
                } else {
                    0.0
                };
                lines.push(format!(
                    "  Injection efficiency: {:.0}% ({}/{} delivered)",
                    ratio, effective, stats.total_injections
                ));
            } else {
                // Cumulative report
                let stats =
                    crate::db::injection::get_injection_stats_cumulative(conn, project_id, None)
                        .map_err(|e| e.to_string())?;

                if stats.total_injections == 0 {
                    return Ok::<_, String>("No injection data recorded yet.".to_string());
                }

                let sessions = crate::db::injection::count_tracked_sessions(conn, project_id)
                    .map_err(|e| e.to_string())?;

                lines.push("Cumulative Injection Report".to_string());
                lines.push(format!("  Sessions tracked: {}", sessions));
                lines.push(format!("  Total injections: {}", stats.total_injections));
                lines.push(format!("  Total chars injected: {}", stats.total_chars));
                lines.push(format!("  Avg chars/injection: {:.0}", stats.avg_chars));
                lines.push(format!("  Total deduped: {}", stats.total_deduped));
                lines.push(format!("  Total cached: {}", stats.total_cached));
                if let Some(ms) = stats.avg_latency_ms {
                    lines.push(format!("  Avg latency: {:.0}ms", ms));
                }
            }

            Ok(lines.join("\n"))
        })
        .await?;

    Ok(message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::pool::{DatabasePool, MainPool};
    use crate::mcp::requests::{SessionAction, SessionRequest};
    use crate::mcp::responses::SessionData;
    use crate::tools::core::test_utils::MockToolContext;

    /// Helper: build a SessionRequest for a given action with all other fields None.
    fn make_request(action: SessionAction) -> SessionRequest {
        SessionRequest {
            action,
            session_id: None,
            limit: None,
            group_by: None,
            since_days: None,
            insight_source: None,
            min_confidence: None,
            insight_id: None,
            dry_run: None,
            category: None,
        }
    }

    /// Helper: insert a session row into the test DB.
    async fn insert_session(
        pool: &MainPool,
        id: &str,
        project_id: i64,
        status: &str,
        source: Option<&str>,
    ) {
        let id = id.to_string();
        let status = status.to_string();
        let source = source.map(|s| s.to_string());
        pool.run(move |conn| {
            conn.execute(
                "INSERT INTO sessions (id, project_id, status, source, started_at, last_activity)
                 VALUES (?1, ?2, ?3, ?4, datetime('now'), datetime('now'))",
                rusqlite::params![id, project_id, status, source],
            )?;
            Ok::<_, rusqlite::Error>(())
        })
        .await
        .expect("Failed to insert session");
    }

    /// Helper: insert a tool_history row.
    async fn insert_tool_history(
        pool: &MainPool,
        session_id: &str,
        tool_name: &str,
        success: bool,
    ) {
        let session_id = session_id.to_string();
        let tool_name = tool_name.to_string();
        pool.run(move |conn| {
            conn.execute(
                "INSERT INTO tool_history (session_id, tool_name, success, created_at)
                 VALUES (?1, ?2, ?3, datetime('now'))",
                rusqlite::params![session_id, tool_name, success],
            )?;
            Ok::<_, rusqlite::Error>(())
        })
        .await
        .expect("Failed to insert tool history");
    }

    // ========================================================================
    // Pure function tests: format_bytes
    // ========================================================================

    #[test]
    fn test_format_bytes_zero() {
        assert_eq!(storage::format_bytes(0), "0 B");
    }

    #[test]
    fn test_format_bytes_bytes() {
        assert_eq!(storage::format_bytes(512), "512 B");
        assert_eq!(storage::format_bytes(1023), "1023 B");
    }

    #[test]
    fn test_format_bytes_kilobytes() {
        assert_eq!(storage::format_bytes(1024), "1.0 KB");
        assert_eq!(storage::format_bytes(1536), "1.5 KB");
    }

    #[test]
    fn test_format_bytes_megabytes() {
        assert_eq!(storage::format_bytes(1_048_576), "1.0 MB");
        assert_eq!(storage::format_bytes(5_242_880), "5.0 MB");
    }

    #[test]
    fn test_format_bytes_gigabytes() {
        assert_eq!(storage::format_bytes(1_073_741_824), "1.0 GB");
        assert_eq!(storage::format_bytes(2_684_354_560), "2.5 GB");
    }

    // ========================================================================
    // Pure function tests: count_table
    // ========================================================================

    #[tokio::test]
    async fn test_count_table_allowed_table() {
        let pool = DatabasePool::open_in_memory().await.expect("pool");
        let count = pool
            .run(|conn| {
                Ok::<_, rusqlite::Error>(storage::count_table(
                    conn,
                    storage::AllowedTable::Sessions,
                ))
            })
            .await
            .unwrap();
        // Table exists (migrations ran), count should be 0
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn test_count_table_allowed_empty_table() {
        let pool = DatabasePool::open_in_memory().await.expect("pool");
        let count = pool
            .run(|conn| {
                Ok::<_, rusqlite::Error>(storage::count_table(
                    conn,
                    storage::AllowedTable::Sessions,
                ))
            })
            .await
            .unwrap();
        // In allowlist and schema exists, but 0 rows
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn test_count_table_with_data() {
        let ctx = MockToolContext::with_project().await;
        let project_id = ctx.project_id().await.unwrap();
        insert_session(&ctx.pool, "sess-1", project_id, "active", Some("startup")).await;
        insert_session(
            &ctx.pool,
            "sess-2",
            project_id,
            "completed",
            Some("startup"),
        )
        .await;

        let count = ctx
            .pool
            .run(|conn| {
                Ok::<_, rusqlite::Error>(storage::count_table(
                    conn,
                    storage::AllowedTable::Sessions,
                ))
            })
            .await
            .unwrap();
        assert_eq!(count, 2);
    }

    // ========================================================================
    // session_history: CurrentSession
    // ========================================================================

    #[tokio::test]
    async fn test_current_session_none() {
        let ctx = MockToolContext::new().await;
        let result = handle_session(&ctx, make_request(SessionAction::CurrentSession))
            .await
            .unwrap();
        assert_eq!(result.0.action, "current");
        assert!(result.0.message.contains("No active session"));
        assert!(result.0.data.is_none());
    }

    #[tokio::test]
    async fn test_current_session_with_id() {
        let ctx = MockToolContext::new().await;
        ctx.set_session_id("test-session-abc".into()).await;
        let result = handle_session(&ctx, make_request(SessionAction::CurrentSession))
            .await
            .unwrap();
        assert_eq!(result.0.action, "current");
        assert!(result.0.message.contains("test-session-abc"));
        match result.0.data {
            Some(SessionData::Current(data)) => {
                assert_eq!(data.session_id, "test-session-abc");
            }
            other => panic!("Expected SessionData::Current, got {:?}", other),
        }
    }

    // ========================================================================
    // session_history: ListSessions
    // ========================================================================

    #[tokio::test]
    async fn test_list_sessions_no_project() {
        let ctx = MockToolContext::new().await;
        let result = handle_session(&ctx, make_request(SessionAction::ListSessions)).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_list_sessions_empty() {
        let ctx = MockToolContext::with_project().await;
        let result = handle_session(&ctx, make_request(SessionAction::ListSessions))
            .await
            .unwrap();
        assert_eq!(result.0.action, "list_sessions");
        assert!(result.0.message.contains("No sessions found"));
        match result.0.data {
            Some(SessionData::ListSessions(data)) => {
                assert!(data.sessions.is_empty());
                assert_eq!(data.total, 0);
            }
            other => panic!("Expected SessionData::ListSessions, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_list_sessions_with_data() {
        let ctx = MockToolContext::with_project().await;
        let pid = ctx.project_id().await.unwrap();
        insert_session(&ctx.pool, "sess-aaa", pid, "completed", Some("startup")).await;
        insert_session(&ctx.pool, "sess-bbb", pid, "active", Some("resume")).await;

        let result = handle_session(&ctx, make_request(SessionAction::ListSessions))
            .await
            .unwrap();
        assert_eq!(result.0.action, "list_sessions");
        match result.0.data {
            Some(SessionData::ListSessions(data)) => {
                assert_eq!(data.sessions.len(), 2);
                assert_eq!(data.total, 2);
            }
            other => panic!("Expected SessionData::ListSessions, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_list_sessions_respects_limit() {
        let ctx = MockToolContext::with_project().await;
        let pid = ctx.project_id().await.unwrap();
        for i in 0..5 {
            insert_session(&ctx.pool, &format!("sess-{i}"), pid, "completed", None).await;
        }

        let mut req = make_request(SessionAction::ListSessions);
        req.limit = Some(2);
        let result = handle_session(&ctx, req).await.unwrap();
        match result.0.data {
            Some(SessionData::ListSessions(data)) => {
                assert_eq!(data.sessions.len(), 2);
            }
            other => panic!("Expected SessionData::ListSessions, got {:?}", other),
        }
    }

    // ========================================================================
    // session_history: GetHistory
    // ========================================================================

    #[tokio::test]
    async fn test_get_history_no_session_no_project() {
        let ctx = MockToolContext::new().await;
        // No session_id provided and no active session
        let result = handle_session(&ctx, make_request(SessionAction::GetHistory)).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_get_history_empty() {
        let ctx = MockToolContext::with_project().await;
        ctx.set_session_id("sess-empty".into()).await;
        let result = handle_session(&ctx, make_request(SessionAction::GetHistory))
            .await
            .unwrap();
        assert_eq!(result.0.action, "get_history");
        assert!(result.0.message.contains("No history"));
        match result.0.data {
            Some(SessionData::History(data)) => {
                assert!(data.entries.is_empty());
                assert_eq!(data.total, 0);
            }
            other => panic!("Expected SessionData::History, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_get_history_with_tool_calls() {
        let ctx = MockToolContext::with_project().await;
        let pid = ctx.project_id().await.unwrap();
        insert_session(&ctx.pool, "sess-hist", pid, "active", Some("startup")).await;
        insert_tool_history(&ctx.pool, "sess-hist", "memory", true).await;
        insert_tool_history(&ctx.pool, "sess-hist", "code", false).await;

        ctx.set_session_id("sess-hist".into()).await;
        let result = handle_session(&ctx, make_request(SessionAction::GetHistory))
            .await
            .unwrap();
        match result.0.data {
            Some(SessionData::History(data)) => {
                assert_eq!(data.entries.len(), 2);
                assert_eq!(data.total, 2);
                assert_eq!(data.session_id, "sess-hist");
            }
            other => panic!("Expected SessionData::History, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_get_history_with_explicit_session_id() {
        let ctx = MockToolContext::with_project().await;
        let pid = ctx.project_id().await.unwrap();
        insert_session(&ctx.pool, "sess-explicit", pid, "active", None).await;
        insert_tool_history(&ctx.pool, "sess-explicit", "project", true).await;

        let mut req = make_request(SessionAction::GetHistory);
        req.session_id = Some("sess-explicit".into());
        let result = handle_session(&ctx, req).await.unwrap();
        match result.0.data {
            Some(SessionData::History(data)) => {
                assert_eq!(data.session_id, "sess-explicit");
                assert_eq!(data.entries.len(), 1);
            }
            other => panic!("Expected SessionData::History, got {:?}", other),
        }
    }

    // ========================================================================
    // Recap
    // ========================================================================

    #[tokio::test]
    async fn test_recap_no_project() {
        let ctx = MockToolContext::new().await;
        let result = handle_session(&ctx, make_request(SessionAction::Recap))
            .await
            .unwrap();
        assert_eq!(result.0.action, "recap");
        // Should return something (at minimum a no-data message)
        assert!(!result.0.message.is_empty());
    }

    #[tokio::test]
    async fn test_recap_with_project() {
        let ctx = MockToolContext::with_project().await;
        let result = handle_session(&ctx, make_request(SessionAction::Recap))
            .await
            .unwrap();
        assert_eq!(result.0.action, "recap");
        assert!(!result.0.message.is_empty());
    }

    // ========================================================================
    // ErrorPatterns
    // ========================================================================

    #[tokio::test]
    async fn test_error_patterns_no_project() {
        let ctx = MockToolContext::new().await;
        let result = handle_session(&ctx, make_request(SessionAction::ErrorPatterns)).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_error_patterns_empty() {
        let ctx = MockToolContext::with_project().await;
        let result = handle_session(&ctx, make_request(SessionAction::ErrorPatterns))
            .await
            .unwrap();
        assert_eq!(result.0.action, "error_patterns");
        assert!(result.0.message.contains("No error patterns"));
        match result.0.data {
            Some(SessionData::ErrorPatterns(data)) => {
                assert!(data.patterns.is_empty());
                assert_eq!(data.total, 0);
            }
            other => panic!("Expected SessionData::ErrorPatterns, got {:?}", other),
        }
    }

    // ========================================================================
    // SessionLineage
    // ========================================================================

    #[tokio::test]
    async fn test_session_lineage_no_project() {
        let ctx = MockToolContext::new().await;
        let result = handle_session(&ctx, make_request(SessionAction::SessionLineage)).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_session_lineage_empty() {
        let ctx = MockToolContext::with_project().await;
        let result = handle_session(&ctx, make_request(SessionAction::SessionLineage))
            .await
            .unwrap();
        assert_eq!(result.0.action, "session_lineage");
        assert!(result.0.message.contains("No sessions found"));
        match result.0.data {
            Some(SessionData::SessionLineage(data)) => {
                assert!(data.sessions.is_empty());
                assert_eq!(data.total, 0);
            }
            other => panic!("Expected SessionData::SessionLineage, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_session_lineage_with_chain() {
        let ctx = MockToolContext::with_project().await;
        let pid = ctx.project_id().await.unwrap();
        insert_session(&ctx.pool, "sess-parent", pid, "completed", Some("startup")).await;

        // Insert a resumed session
        let pid_clone = pid;
        ctx.pool
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO sessions (id, project_id, status, source, resumed_from, started_at, last_activity)
                     VALUES ('sess-child', ?1, 'active', 'resume', 'sess-parent', datetime('now'), datetime('now'))",
                    rusqlite::params![pid_clone],
                )?;
                Ok::<_, rusqlite::Error>(())
            })
            .await
            .unwrap();

        let result = handle_session(&ctx, make_request(SessionAction::SessionLineage))
            .await
            .unwrap();
        match result.0.data {
            Some(SessionData::SessionLineage(data)) => {
                assert_eq!(data.sessions.len(), 2);
                assert_eq!(data.total, 2);
                // The child should have resumed_from set
                let child = data.sessions.iter().find(|s| s.id == "sess-child").unwrap();
                assert_eq!(child.resumed_from.as_deref(), Some("sess-parent"));
                assert_eq!(child.source.as_deref(), Some("resume"));
            }
            other => panic!("Expected SessionData::SessionLineage, got {:?}", other),
        }
    }

    // ========================================================================
    // Capabilities
    // ========================================================================

    #[tokio::test]
    async fn test_capabilities_basic() {
        let ctx = MockToolContext::with_project().await;
        let result = handle_session(&ctx, make_request(SessionAction::Capabilities))
            .await
            .unwrap();
        assert_eq!(result.0.action, "capabilities");
        match result.0.data {
            Some(SessionData::Capabilities(data)) => {
                assert!(!data.capabilities.is_empty());
                let names: Vec<&str> = data.capabilities.iter().map(|c| c.name.as_str()).collect();
                assert!(names.contains(&"semantic_search"));
                assert!(names.contains(&"fuzzy_search"));
                assert!(names.contains(&"code_index"));
                assert!(names.contains(&"mcp_sampling"));
            }
            other => panic!("Expected SessionData::Capabilities, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_capabilities_no_embeddings_unavailable() {
        let ctx = MockToolContext::new().await;
        let result = handle_session(&ctx, make_request(SessionAction::Capabilities))
            .await
            .unwrap();
        match result.0.data {
            Some(SessionData::Capabilities(data)) => {
                let semantic = data
                    .capabilities
                    .iter()
                    .find(|c| c.name == "semantic_search")
                    .unwrap();
                assert_eq!(semantic.status, "unavailable");
                assert!(semantic.detail.is_some());
            }
            other => panic!("Expected SessionData::Capabilities, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_capabilities_background_analysis_available() {
        let ctx = MockToolContext::new().await;
        let result = handle_session(&ctx, make_request(SessionAction::Capabilities))
            .await
            .unwrap();
        match result.0.data {
            Some(SessionData::Capabilities(data)) => {
                let analysis = data
                    .capabilities
                    .iter()
                    .find(|c| c.name == "background_analysis")
                    .unwrap();
                assert_eq!(analysis.status, "available");
            }
            other => panic!("Expected SessionData::Capabilities, got {:?}", other),
        }
    }

    // ========================================================================
    // StorageStatus
    // ========================================================================

    #[tokio::test]
    async fn test_storage_status() {
        let ctx = MockToolContext::with_project().await;
        let result = handle_session(&ctx, make_request(SessionAction::StorageStatus))
            .await
            .unwrap();
        assert_eq!(result.0.action, "storage_status");
        assert!(result.0.message.contains("Storage Status"));
        assert!(result.0.message.contains("Row Counts"));
        assert!(result.0.message.contains("Retention Policy"));
    }

    // ========================================================================
    // Cleanup
    // ========================================================================

    #[tokio::test]
    async fn test_cleanup_dry_run_default() {
        let ctx = MockToolContext::with_project().await;
        let result = handle_session(&ctx, make_request(SessionAction::Cleanup))
            .await
            .unwrap();
        assert_eq!(result.0.action, "cleanup");
        // Default is dry_run=true
        assert!(result.0.message.contains("Preview") || result.0.message.contains("dry run"));
    }

    #[tokio::test]
    async fn test_cleanup_dry_run_explicit() {
        let ctx = MockToolContext::with_project().await;
        let mut req = make_request(SessionAction::Cleanup);
        req.dry_run = Some(true);
        let result = handle_session(&ctx, req).await.unwrap();
        assert_eq!(result.0.action, "cleanup");
        assert!(result.0.message.contains("Preview") || result.0.message.contains("dry run"));
    }

    #[tokio::test]
    async fn test_cleanup_execute() {
        let ctx = MockToolContext::with_project().await;
        let mut req = make_request(SessionAction::Cleanup);
        req.dry_run = Some(false);
        let result = handle_session(&ctx, req).await.unwrap();
        assert_eq!(result.0.action, "cleanup");
        assert!(result.0.message.contains("Cleanup Complete"));
    }

    // ========================================================================
    // ensure_session
    // ========================================================================

    #[tokio::test]
    async fn test_ensure_session_returns_existing() {
        let ctx = MockToolContext::with_project().await;
        ctx.set_session_id("already-exists".into()).await;
        let result = ensure_session(&ctx).await.unwrap();
        assert_eq!(result, "already-exists");
    }

    // ========================================================================
    // handle_session dispatcher
    // ========================================================================

    #[tokio::test]
    async fn test_handle_session_dispatches_all_actions() {
        // Verify the dispatcher doesn't panic for every SessionAction variant.
        // Actions that succeed with a project + session set:
        let ctx = MockToolContext::with_project().await;
        ctx.set_session_id("test-dispatch".into()).await;

        let succeeding_actions = vec![
            SessionAction::CurrentSession,
            SessionAction::ListSessions,
            SessionAction::GetHistory,
            SessionAction::Recap,
            SessionAction::ErrorPatterns,
            SessionAction::SessionLineage,
            SessionAction::Capabilities,
            SessionAction::StorageStatus,
            SessionAction::Cleanup,
            SessionAction::UsageSummary,
            SessionAction::UsageStats,
            SessionAction::UsageList,
            SessionAction::Insights,
        ];

        for action in succeeding_actions {
            let req = make_request(action);
            let result = handle_session(&ctx, req).await;
            assert!(
                result.is_ok(),
                "handle_session failed for {:?}: {:?}",
                action,
                result.err()
            );
        }

        // DismissInsight with valid params but nonexistent ID — should succeed
        // with a "not found" response, not panic or error.
        let mut req = make_request(SessionAction::DismissInsight);
        req.insight_id = Some(999);
        req.insight_source = Some("pondering".into());
        let result = handle_session(&ctx, req).await;
        assert!(
            result.is_ok(),
            "DismissInsight with nonexistent ID should succeed: {:?}",
            result.err()
        );
    }

    // ========================================================================
    // HistoryKind enum coverage
    // ========================================================================

    #[tokio::test]
    async fn test_session_history_current_kind() {
        let ctx = MockToolContext::new().await;
        ctx.set_session_id("kind-test".into()).await;
        let result = session_history(&ctx, HistoryKind::Current, None, None)
            .await
            .unwrap();
        assert_eq!(result.0.action, "current");
    }

    #[tokio::test]
    async fn test_session_history_list_kind() {
        let ctx = MockToolContext::with_project().await;
        let result = session_history(&ctx, HistoryKind::List, None, None)
            .await
            .unwrap();
        assert_eq!(result.0.action, "list_sessions");
    }

    #[tokio::test]
    async fn test_session_history_get_history_kind() {
        let ctx = MockToolContext::with_project().await;
        let result = session_history(&ctx, HistoryKind::GetHistory, Some("any-id".into()), None)
            .await
            .unwrap();
        assert_eq!(result.0.action, "get_history");
    }

    // ========================================================================
    // List sessions with source/resumed_from formatting
    // ========================================================================

    #[tokio::test]
    async fn test_list_sessions_source_formatting() {
        let ctx = MockToolContext::with_project().await;
        let pid = ctx.project_id().await.unwrap();

        // Session with source and resumed_from
        ctx.pool
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO sessions (id, project_id, status, source, resumed_from, started_at, last_activity)
                     VALUES ('sess-fmt', ?1, 'active', 'resume', 'sess-prev', datetime('now'), datetime('now'))",
                    rusqlite::params![pid],
                )?;
                Ok::<_, rusqlite::Error>(())
            })
            .await
            .unwrap();

        let result = handle_session(&ctx, make_request(SessionAction::ListSessions))
            .await
            .unwrap();
        // Message should contain the resume source info
        assert!(result.0.message.contains("resume"));
    }

    // ========================================================================
    // get_session_recap
    // ========================================================================

    #[tokio::test]
    async fn test_get_session_recap_no_project() {
        let ctx = MockToolContext::new().await;
        let recap = get_session_recap(&ctx).await.unwrap();
        // Should return something even without a project
        assert!(!recap.is_empty());
    }

    #[tokio::test]
    async fn test_get_session_recap_with_data() {
        let ctx = MockToolContext::with_project().await;
        let pid = ctx.project_id().await.unwrap();

        // Insert a goal so recap has content (memory_facts removed in Phase 4)
        ctx.pool
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO goals (project_id, title, status, priority, progress_percent, created_at, updated_at)
                     VALUES (?1, 'Test goal', 'in_progress', 'high', 50, datetime('now'), datetime('now'))",
                    rusqlite::params![pid],
                )?;
                Ok::<_, rusqlite::Error>(())
            })
            .await
            .unwrap();

        let recap = get_session_recap(&ctx).await.unwrap();
        assert!(!recap.is_empty());
    }
}
