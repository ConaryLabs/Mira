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
        SessionAction::HealthTrends => analytics::get_health_trends(ctx, req.limit).await,
        SessionAction::SessionLineage => analytics::get_session_lineage(ctx, req.limit).await,
        SessionAction::Capabilities => analytics::get_capabilities(ctx).await,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::pool::DatabasePool;
    use crate::mcp::requests::{SessionAction, SessionRequest};
    use crate::mcp::responses::SessionData;
    use crate::tools::core::test_utils::MockToolContext;
    use std::sync::Arc;

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
        pool: &Arc<DatabasePool>,
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
        pool: &Arc<DatabasePool>,
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
            .run(|conn| Ok::<_, rusqlite::Error>(storage::count_table(conn, "sessions")))
            .await
            .unwrap();
        // Table exists (migrations ran), count should be 0
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn test_count_table_disallowed_table() {
        let pool = DatabasePool::open_in_memory().await.expect("pool");
        let count = pool
            .run(|conn| Ok::<_, rusqlite::Error>(storage::count_table(conn, "projects")))
            .await
            .unwrap();
        // "projects" not in allowlist, should return 0
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn test_count_table_allowed_empty_table() {
        let pool = DatabasePool::open_in_memory().await.expect("pool");
        let count = pool
            .run(|conn| Ok::<_, rusqlite::Error>(storage::count_table(conn, "memory_facts")))
            .await
            .unwrap();
        // In allowlist and schema exists, but 0 rows
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn test_count_table_rejected_by_allowlist() {
        let pool = DatabasePool::open_in_memory().await.expect("pool");
        // "bogus_table" is not in ALLOWED_TABLES, so count_table short-circuits to 0
        // without executing any SQL (prevents SQL injection via table name)
        let count = pool
            .run(|conn| Ok::<_, rusqlite::Error>(storage::count_table(conn, "bogus_table")))
            .await
            .unwrap();
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
            .run(|conn| Ok::<_, rusqlite::Error>(storage::count_table(conn, "sessions")))
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
    // HealthTrends
    // ========================================================================

    #[tokio::test]
    async fn test_health_trends_no_project() {
        let ctx = MockToolContext::new().await;
        let result = handle_session(&ctx, make_request(SessionAction::HealthTrends)).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_health_trends_empty() {
        let ctx = MockToolContext::with_project().await;
        let result = handle_session(&ctx, make_request(SessionAction::HealthTrends))
            .await
            .unwrap();
        assert_eq!(result.0.action, "health_trends");
        assert!(result.0.message.contains("No health snapshots"));
        match result.0.data {
            Some(SessionData::HealthTrends(data)) => {
                assert!(data.snapshots.is_empty());
                assert!(data.trend.is_none());
            }
            other => panic!("Expected SessionData::HealthTrends, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_health_trends_with_snapshots() {
        let ctx = MockToolContext::with_project().await;
        let pid = ctx.project_id().await.unwrap();

        // Insert health snapshots
        ctx.pool
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO health_snapshots (project_id, avg_debt_score, max_debt_score,
                     tier_distribution, module_count, snapshot_at, warning_count, todo_count,
                     unwrap_count, error_handling_count, total_finding_count)
                     VALUES (?1, 3.5, 8.0, '{\"A\":5}', 10, datetime('now', '-1 day'), 2, 3, 1, 1, 7)",
                    rusqlite::params![pid],
                )?;
                conn.execute(
                    "INSERT INTO health_snapshots (project_id, avg_debt_score, max_debt_score,
                     tier_distribution, module_count, snapshot_at, warning_count, todo_count,
                     unwrap_count, error_handling_count, total_finding_count)
                     VALUES (?1, 1.0, 3.0, '{\"A\":10}', 10, datetime('now'), 0, 1, 0, 0, 1)",
                    rusqlite::params![pid],
                )?;
                Ok::<_, rusqlite::Error>(())
            })
            .await
            .unwrap();

        let result = handle_session(&ctx, make_request(SessionAction::HealthTrends))
            .await
            .unwrap();
        match result.0.data {
            Some(SessionData::HealthTrends(data)) => {
                assert_eq!(data.snapshots.len(), 2);
                assert!(data.trend.is_some());
                let trend = data.trend.unwrap();
                // Debt went from 3.5 to 1.0 — should be "improving"
                assert_eq!(trend, "improving");
            }
            other => panic!("Expected SessionData::HealthTrends, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_health_trends_single_snapshot_no_trend() {
        let ctx = MockToolContext::with_project().await;
        let pid = ctx.project_id().await.unwrap();

        ctx.pool
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO health_snapshots (project_id, avg_debt_score, max_debt_score,
                     tier_distribution, module_count, snapshot_at, warning_count, todo_count,
                     unwrap_count, error_handling_count, total_finding_count)
                     VALUES (?1, 2.0, 5.0, '{\"A\":8}', 8, datetime('now'), 1, 2, 0, 0, 3)",
                    rusqlite::params![pid],
                )?;
                Ok::<_, rusqlite::Error>(())
            })
            .await
            .unwrap();

        let result = handle_session(&ctx, make_request(SessionAction::HealthTrends))
            .await
            .unwrap();
        match result.0.data {
            Some(SessionData::HealthTrends(data)) => {
                assert_eq!(data.snapshots.len(), 1);
                assert!(data.trend.is_none());
            }
            other => panic!("Expected SessionData::HealthTrends, got {:?}", other),
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
            SessionAction::HealthTrends,
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

        // Insert some memories so recap has content
        ctx.pool
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO memory_facts (content, fact_type, category, confidence, project_id, created_at, updated_at)
                     VALUES ('Test preference', 'preference', 'general', 0.9, ?1, datetime('now'), datetime('now'))",
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
