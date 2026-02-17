// crates/mira-server/src/tools/core/session.rs
// Unified session management tools

use crate::config::file::MiraConfig;
use crate::db::retention::{cleanup_orphans, count_retention_candidates, run_data_retention_sync};
use crate::db::{
    build_session_recap_sync, create_session_ext_sync, get_error_patterns_sync,
    get_health_history_sync, get_recent_sessions_sync, get_session_history_scoped_sync,
    get_session_lineage_sync,
};
use crate::error::MiraError;
use crate::hooks::session::{read_claude_session_id, read_source_info};
use crate::mcp::responses::Json;
use crate::mcp::responses::{
    CapabilitiesData, CapabilityStatus, ErrorPatternItem, ErrorPatternsData, HealthSnapshotItem,
    HealthTrendsData, HistoryEntry, LineageSession, SessionCurrentData, SessionData,
    SessionHistoryData, SessionLineageData, SessionListData, SessionOutput, SessionSummary,
};
use crate::tools::core::session_notes;
use crate::tools::core::{NO_ACTIVE_PROJECT_ERROR, ToolContext};
use crate::utils::{truncate, truncate_at_boundary};
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
        SessionAction::StorageStatus => storage_status(ctx).await,
        SessionAction::Cleanup => cleanup(ctx, req.dry_run, req.category).await,
        SessionAction::ErrorPatterns => get_error_patterns(ctx, req.limit).await,
        SessionAction::HealthTrends => get_health_trends(ctx, req.limit).await,
        SessionAction::SessionLineage => get_session_lineage(ctx, req.limit).await,
        SessionAction::Capabilities => get_capabilities(ctx).await,
    }
}

/// Internal kind enum for session history queries (replaces deleted SessionHistoryAction)
pub(crate) enum HistoryKind {
    Current,
    List,
    GetHistory,
}

/// Query session history
pub(crate) async fn session_history<C: ToolContext>(
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
            let project = ctx.get_project().await;
            let project_id = project
                .as_ref()
                .map(|p| p.id)
                .ok_or_else(|| MiraError::InvalidInput(NO_ACTIVE_PROJECT_ERROR.to_string()))?;

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

            let project_id = ctx
                .get_project()
                .await
                .as_ref()
                .map(|p| p.id)
                .ok_or_else(|| MiraError::InvalidInput(NO_ACTIVE_PROJECT_ERROR.to_string()))?;
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

/// Helper: count rows in a table, returning 0 if the table doesn't exist.
fn count_table(conn: &rusqlite::Connection, table: &str) -> usize {
    const ALLOWED_TABLES: &[&str] = &[
        "memory_facts",
        "sessions",
        "tool_history",
        "llm_usage",
        "embeddings_usage",
        "behavior_patterns",
        "goals",
        "system_observations",
        "health_snapshots",
    ];
    if !ALLOWED_TABLES.contains(&table) {
        return 0;
    }
    let sql = format!("SELECT COUNT(*) FROM {table}");
    conn.query_row(&sql, [], |row| row.get::<_, usize>(0))
        .unwrap_or(0)
}

/// Helper: format byte size in human-readable form.
fn format_bytes(bytes: u64) -> String {
    if bytes >= 1_073_741_824 {
        format!("{:.1} GB", bytes as f64 / 1_073_741_824.0)
    } else if bytes >= 1_048_576 {
        format!("{:.1} MB", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{} B", bytes)
    }
}

/// Show database storage status, row counts, and retention policy.
async fn storage_status<C: ToolContext>(ctx: &C) -> Result<Json<SessionOutput>, MiraError> {
    // Get file sizes
    let home = dirs::home_dir().unwrap_or_default();
    let main_db_path = home.join(".mira/mira.db");
    let code_db_path = home.join(".mira/mira-code.db");

    let main_db_size = std::fs::metadata(&main_db_path)
        .map(|m| m.len())
        .unwrap_or(0);
    let code_db_size = std::fs::metadata(&code_db_path)
        .map(|m| m.len())
        .unwrap_or(0);

    // Query row counts from main DB
    let counts = ctx
        .pool()
        .run(move |conn| {
            let memories = count_table(conn, "memory_facts");
            let sessions = count_table(conn, "sessions");
            let tool_history = count_table(conn, "tool_history");
            let llm_usage = count_table(conn, "llm_usage");
            let embed_usage = count_table(conn, "embeddings_usage");
            let behavior = count_table(conn, "behavior_patterns");
            let goals = count_table(conn, "goals");
            let observations = count_table(conn, "system_observations");

            Ok::<_, String>((
                memories,
                sessions,
                tool_history,
                llm_usage,
                embed_usage,
                behavior,
                goals,
                observations,
            ))
        })
        .await?;

    let (memories, sessions, tool_history, llm_usage, embed_usage, behavior, goals, observations) =
        counts;

    // Load retention config and count candidates
    let config = MiraConfig::load();
    let retention = config.retention;

    let retention_clone = retention.clone();

    let candidates = ctx
        .pool()
        .run(move |conn| Ok::<_, String>(count_retention_candidates(conn, &retention_clone)))
        .await?;

    // Build report
    let mut report = String::new();
    report.push_str("## Storage Status\n\n");

    report.push_str("### Database Files\n");
    report.push_str(&format!("- mira.db: {}\n", format_bytes(main_db_size)));
    report.push_str(&format!("- mira-code.db: {}\n", format_bytes(code_db_size)));
    report.push_str(&format!(
        "- Total: {}\n\n",
        format_bytes(main_db_size + code_db_size)
    ));

    report.push_str("### Row Counts\n");
    report.push_str(&format!("- Memories: {}\n", memories));
    report.push_str(&format!("- Sessions: {}\n", sessions));
    report.push_str(&format!("- Tool history: {}\n", tool_history));
    report.push_str(&format!(
        "- Analytics: {} (LLM) + {} (embeddings)\n",
        llm_usage, embed_usage
    ));
    report.push_str(&format!("- Behavior patterns: {}\n", behavior));
    report.push_str(&format!("- Goals: {}\n", goals));
    report.push_str(&format!("- Observations: {}\n\n", observations));

    report.push_str("### Retention Policy\n");
    if retention.is_enabled() {
        report.push_str("Status: **enabled**\n");
    } else {
        report.push_str(
            "Status: **disabled** (enable in ~/.mira/config.toml [retention] enabled = true)\n",
        );
    }
    report.push_str(&format!(
        "- Tool history: {} days\n",
        retention.tool_history_days
    ));
    report.push_str(&format!("- Sessions: {} days\n", retention.sessions_days));
    report.push_str(&format!("- Analytics: {} days\n", retention.analytics_days));
    report.push_str(&format!("- Behavior: {} days\n", retention.behavior_days));
    report.push_str(&format!(
        "- Observations: {} days\n\n",
        retention.observations_days
    ));

    if candidates.is_empty() {
        report.push_str("### Cleanup Candidates\nNo rows eligible for cleanup.\n");
    } else {
        report.push_str("### Cleanup Candidates\n");
        let mut total_candidates = 0;
        for (table, count) in &candidates {
            report.push_str(&format!("- {}: {} rows\n", table, count));
            total_candidates += count;
        }
        report.push_str(&format!("\nTotal: {} rows eligible\n", total_candidates));
    }

    report.push_str("\n### Protected (never auto-deleted)\n");
    report.push_str("- Memories, Goals, Code index\n");

    Ok(Json(SessionOutput {
        action: "storage_status".into(),
        message: report,
        data: None,
    }))
}

/// Run data cleanup with dry-run preview by default.
async fn cleanup<C: ToolContext>(
    ctx: &C,
    dry_run: Option<bool>,
    category: Option<String>,
) -> Result<Json<SessionOutput>, MiraError> {
    let dry_run = dry_run.unwrap_or(true);

    if dry_run {
        // Preview mode: show what WOULD be deleted
        let config = MiraConfig::load();
        let retention = config.retention;
        let retention_clone = retention.clone();

        let candidates = ctx
            .pool()
            .run(move |conn| Ok::<_, String>(count_retention_candidates(conn, &retention_clone)))
            .await?;

        let mut report = String::new();
        report.push_str("## Cleanup Preview (dry run)\n\n");

        if candidates.is_empty() {
            report.push_str("No rows eligible for cleanup.\n");
        } else {
            // Filter display by category if specified
            let filter = category.as_deref().unwrap_or("all");
            let filtered: Vec<&(String, usize)> = candidates
                .iter()
                .filter(|(table, _)| match filter {
                    "sessions" => matches!(
                        table.as_str(),
                        "sessions" | "session_snapshots" | "session_tasks" | "tool_history"
                    ),
                    "analytics" => {
                        matches!(table.as_str(), "llm_usage" | "embeddings_usage")
                    }
                    "behavior" => {
                        matches!(table.as_str(), "behavior_patterns" | "system_observations")
                    }
                    _ => true, // "all" or unknown
                })
                .collect();

            if filtered.is_empty() {
                report.push_str(&format!(
                    "No rows eligible for cleanup in category '{}'.\n",
                    filter
                ));
            } else {
                report.push_str("Would delete:\n");
                let mut total = 0;
                for (table, count) in &filtered {
                    report.push_str(&format!("- {}: {} rows\n", table, count));
                    total += count;
                }
                report.push_str(&format!("\nTotal: {} rows\n", total));
            }

            if filter != "all" {
                report.push_str(&format!(
                    "\nNote: showing category '{}' only. Execute mode runs full cleanup regardless of category filter.\n",
                    filter
                ));
            }
        }

        report.push_str("\n### Protected (never deleted)\n");
        report.push_str("- Memories, Goals, Code index\n");
        report.push_str("\nTo execute cleanup, call: session(action=\"cleanup\", dry_run=false)\n");

        Ok(Json(SessionOutput {
            action: "cleanup".into(),
            message: report,
            data: None,
        }))
    } else {
        // Execute mode
        let config = MiraConfig::load();
        let retention = config.retention;
        let retention_enabled = retention.is_enabled();
        let retention_clone = retention.clone();

        let (retention_deleted, orphans_deleted) = ctx
            .pool()
            .run(move |conn| {
                let retention_deleted = if retention_enabled {
                    run_data_retention_sync(conn, &retention_clone)?
                } else {
                    0
                };
                let orphans_deleted = cleanup_orphans(conn)?;
                Ok::<_, String>((retention_deleted, orphans_deleted))
            })
            .await?;

        let mut report = String::new();
        report.push_str("## Cleanup Complete\n\n");

        if retention_enabled {
            report.push_str(&format!(
                "- Retention cleanup: {} rows deleted\n",
                retention_deleted
            ));
        } else {
            report.push_str("- Retention cleanup: skipped (not enabled)\n");
            report.push_str("  Enable in ~/.mira/config.toml: [retention] enabled = true\n");
        }
        report.push_str(&format!(
            "- Orphan cleanup: {} rows deleted\n",
            orphans_deleted
        ));
        report.push_str(&format!(
            "\nTotal: {} rows deleted\n",
            retention_deleted + orphans_deleted
        ));

        Ok(Json(SessionOutput {
            action: "cleanup".into(),
            message: report,
            data: None,
        }))
    }
}

/// Query error patterns for the active project.
async fn get_error_patterns<C: ToolContext>(
    ctx: &C,
    limit: Option<i64>,
) -> Result<Json<SessionOutput>, MiraError> {
    let project = ctx.get_project().await;
    let project_id = project
        .as_ref()
        .map(|p| p.id)
        .ok_or_else(|| MiraError::InvalidInput(NO_ACTIVE_PROJECT_ERROR.to_string()))?;

    let limit = limit.unwrap_or(20).clamp(1, 100) as usize;

    let rows = ctx
        .pool()
        .run(move |conn| Ok::<_, String>(get_error_patterns_sync(conn, project_id, limit)))
        .await?;

    if rows.is_empty() {
        return Ok(Json(SessionOutput {
            action: "error_patterns".into(),
            message: "No error patterns recorded yet.".to_string(),
            data: Some(SessionData::ErrorPatterns(ErrorPatternsData {
                patterns: vec![],
                total: 0,
            })),
        }));
    }

    let total = rows.len();
    let mut output = format!("Learned error patterns ({} total):\n\n", total);
    let items: Vec<ErrorPatternItem> = rows
        .into_iter()
        .map(|row| {
            output.push_str(&format!(
                "  [{}] (seen {}x) {}\n",
                row.tool_name, row.occurrence_count, row.error_fingerprint
            ));
            if let Some(ref fix) = row.fix_description {
                output.push_str(&format!("    Fix: {}\n", fix));
            }
            output.push('\n');
            ErrorPatternItem {
                tool_name: row.tool_name,
                error_fingerprint: row.error_fingerprint,
                fix_description: row.fix_description,
                occurrence_count: row.occurrence_count,
                last_seen: row.last_seen,
            }
        })
        .collect();

    Ok(Json(SessionOutput {
        action: "error_patterns".into(),
        message: output,
        data: Some(SessionData::ErrorPatterns(ErrorPatternsData {
            patterns: items,
            total,
        })),
    }))
}

/// Query health snapshot trends for the active project.
async fn get_health_trends<C: ToolContext>(
    ctx: &C,
    limit: Option<i64>,
) -> Result<Json<SessionOutput>, MiraError> {
    let project = ctx.get_project().await;
    let project_id = project
        .as_ref()
        .map(|p| p.id)
        .ok_or_else(|| MiraError::InvalidInput(NO_ACTIVE_PROJECT_ERROR.to_string()))?;

    let limit = limit.unwrap_or(10).clamp(1, 50) as usize;

    let snapshots = ctx
        .pool()
        .run(move |conn| get_health_history_sync(conn, project_id, limit))
        .await?;

    if snapshots.is_empty() {
        return Ok(Json(SessionOutput {
            action: "health_trends".into(),
            message: "No health snapshots found.\n\nRun a health scan first: `mira tool index '{\"action\":\"health\"}'`".to_string(),
            data: Some(SessionData::HealthTrends(HealthTrendsData {
                snapshots: vec![],
                trend: None,
            })),
        }));
    }

    // Snapshots are newest-first from DB; reverse for chronological display
    let chronological: Vec<_> = snapshots.iter().rev().collect();

    // Calculate trend by comparing first and last snapshot avg_debt_score
    let trend = if chronological.len() >= 2 {
        let Some(first_snap) = chronological.first() else {
            unreachable!()
        };
        let Some(last_snap) = chronological.last() else {
            unreachable!()
        };
        let first = first_snap.avg_debt_score;
        let last = last_snap.avg_debt_score;
        if first == 0.0 {
            Some("stable".to_string())
        } else {
            let delta_pct = ((last - first) / first) * 100.0;
            if delta_pct < -5.0 {
                Some("improving".to_string())
            } else if delta_pct > 5.0 {
                Some("degrading".to_string())
            } else {
                Some("stable".to_string())
            }
        }
    } else {
        None
    };

    // Build human-readable message
    let mut output = format!("## Health Trends ({} snapshots)\n\n", chronological.len());

    if let Some(ref t) = trend {
        output.push_str(&format!("Overall trend: **{}**\n\n", t));
    }

    output.push_str("| Date | Modules | Avg Debt | Max Debt | Warnings | TODOs | Findings |\n");
    output.push_str("|------|---------|----------|----------|----------|-------|----------|\n");

    for snap in &chronological {
        output.push_str(&format!(
            "| {} | {} | {:.1} | {:.1} | {} | {} | {} |\n",
            snap.snapshot_at,
            snap.module_count,
            snap.avg_debt_score,
            snap.max_debt_score,
            snap.warning_count,
            snap.todo_count,
            snap.total_finding_count,
        ));
    }

    // Show delta summary if we have multiple snapshots
    if chronological.len() >= 2 {
        let Some(first) = chronological.first() else {
            unreachable!()
        };
        let Some(last) = chronological.last() else {
            unreachable!()
        };
        output.push_str(&format!(
            "\nDelta: avg debt {:.1} \u{2192} {:.1}, modules {} \u{2192} {}, findings {} \u{2192} {}\n",
            first.avg_debt_score,
            last.avg_debt_score,
            first.module_count,
            last.module_count,
            first.total_finding_count,
            last.total_finding_count,
        ));
    }

    let items: Vec<HealthSnapshotItem> = chronological
        .iter()
        .map(|snap| HealthSnapshotItem {
            snapshot_at: snap.snapshot_at.clone(),
            module_count: snap.module_count,
            avg_debt_score: snap.avg_debt_score,
            max_debt_score: snap.max_debt_score,
            tier_distribution: snap.tier_distribution.clone(),
            warning_count: snap.warning_count,
            todo_count: snap.todo_count,
            total_finding_count: snap.total_finding_count,
        })
        .collect();

    Ok(Json(SessionOutput {
        action: "health_trends".into(),
        message: output,
        data: Some(SessionData::HealthTrends(HealthTrendsData {
            snapshots: items,
            trend,
        })),
    }))
}

/// Query session lineage (resume chains) for the active project.
async fn get_session_lineage<C: ToolContext>(
    ctx: &C,
    limit: Option<i64>,
) -> Result<Json<SessionOutput>, MiraError> {
    let project = ctx.get_project().await;
    let project_id = project
        .as_ref()
        .map(|p| p.id)
        .ok_or_else(|| MiraError::InvalidInput(NO_ACTIVE_PROJECT_ERROR.to_string()))?;

    let limit = limit.unwrap_or(20).clamp(1, 100) as usize;

    let rows = ctx
        .pool()
        .run(move |conn| get_session_lineage_sync(conn, project_id, limit))
        .await?;

    if rows.is_empty() {
        return Ok(Json(SessionOutput {
            action: "session_lineage".into(),
            message: "No sessions found for this project.".to_string(),
            data: Some(SessionData::SessionLineage(SessionLineageData {
                sessions: vec![],
                total: 0,
            })),
        }));
    }

    // Build a set of session IDs for quick lookup when determining indentation
    let session_ids: std::collections::HashSet<&str> = rows.iter().map(|r| r.id.as_str()).collect();

    // Format human-readable output with lineage indentation
    let mut output = format!("## Session Lineage ({} sessions)\n\n", rows.len());

    for row in &rows {
        let short_id = truncate_at_boundary(&row.id, 8);
        let source_tag = row.source.as_deref().unwrap_or("startup");
        let branch_info = row
            .branch
            .as_ref()
            .map(|b| format!(" (branch: {})", b))
            .unwrap_or_default();
        let age = super::insights::format_age(&row.last_activity);
        let goal_info = match row.goal_count {
            Some(n) if n > 0 => format!(" -- {} goal{}", n, if n == 1 { "" } else { "s" }),
            _ => String::new(),
        };

        // Indent resumed sessions that resume from a session in our result set
        let is_resume_child = row
            .resumed_from
            .as_ref()
            .is_some_and(|rf| session_ids.contains(rf.as_str()));

        if is_resume_child {
            output.push_str(&format!(
                "  <- [{}] {}{}{}{}\n",
                source_tag, short_id, branch_info, age, goal_info
            ));
        } else {
            output.push_str(&format!(
                "[{}] {}{}{}{}\n",
                source_tag, short_id, branch_info, age, goal_info
            ));
        }
    }

    let items: Vec<LineageSession> = rows
        .into_iter()
        .map(|row| LineageSession {
            id: row.id,
            source: row.source,
            resumed_from: row.resumed_from,
            branch: row.branch,
            started_at: row.started_at,
            last_activity: row.last_activity,
            status: row.status,
            goal_count: row.goal_count,
        })
        .collect();

    let total = items.len();
    Ok(Json(SessionOutput {
        action: "session_lineage".into(),
        message: output,
        data: Some(SessionData::SessionLineage(SessionLineageData {
            sessions: items,
            total,
        })),
    }))
}

/// Report which features are available, degraded, or unavailable.
///
/// Checks embeddings, LLM provider, fuzzy cache, and code index status.
/// CLI-only action — not exposed via MCP schema.
async fn get_capabilities<C: ToolContext>(ctx: &C) -> Result<Json<SessionOutput>, MiraError> {
    let mut caps = Vec::new();

    // Semantic search (requires embeddings)
    let has_embeddings = ctx.embeddings().is_some();
    caps.push(CapabilityStatus {
        name: "semantic_search".into(),
        status: if has_embeddings {
            "available"
        } else {
            "unavailable"
        }
        .into(),
        detail: if !has_embeddings {
            Some("Set OPENAI_API_KEY for semantic search".into())
        } else {
            None
        },
    });

    // Background LLM (requires LLM provider)
    let has_llm = ctx.llm_factory().has_any_capability();
    caps.push(CapabilityStatus {
        name: "background_llm".into(),
        status: if has_llm { "available" } else { "unavailable" }.into(),
        detail: if !has_llm {
            Some("Set DEEPSEEK_API_KEY or configure Ollama for background intelligence".into())
        } else {
            None
        },
    });

    // Fuzzy search (requires cache)
    let has_fuzzy = ctx.fuzzy_cache().is_some();
    caps.push(CapabilityStatus {
        name: "fuzzy_search".into(),
        status: if has_fuzzy {
            "available"
        } else {
            "unavailable"
        }
        .into(),
        detail: None,
    });

    // Code index (requires indexed symbols in code DB for this project)
    let project_id = ctx.project_id().await;
    let code_indexed = ctx
        .code_pool()
        .run(move |conn| {
            let count = conn
                .query_row(
                    "SELECT COUNT(*) FROM code_symbols WHERE project_id IS ?1",
                    rusqlite::params![project_id],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap_or(0);
            Ok::<_, MiraError>(count > 0)
        })
        .await
        .unwrap_or(false);
    caps.push(CapabilityStatus {
        name: "code_index".into(),
        status: if code_indexed {
            "available"
        } else {
            "unavailable"
        }
        .into(),
        detail: if !code_indexed {
            Some("Run index(action='project') to enable code intelligence".into())
        } else {
            None
        },
    });

    // MCP sampling (client supports createMessage)
    let has_sampling = ctx.has_sampling();
    caps.push(CapabilityStatus {
        name: "mcp_sampling".into(),
        status: if has_sampling {
            "available"
        } else {
            "unavailable"
        }
        .into(),
        detail: if !has_sampling {
            Some("MCP client does not support sampling/createMessage".into())
        } else {
            None
        },
    });

    // Format message
    let mut msg = String::from("Capability status:\n");
    for cap in &caps {
        let icon = match cap.status.as_str() {
            "available" => "\u{2713}",
            "degraded" => "~",
            _ => "\u{2717}",
        };
        msg.push_str(&format!("  {} {} ({})", icon, cap.name, cap.status));
        if let Some(ref detail) = cap.detail {
            msg.push_str(&format!(" \u{2014} {}", detail));
        }
        msg.push('\n');
    }

    Ok(Json(SessionOutput {
        action: "capabilities".into(),
        message: msg,
        data: Some(SessionData::Capabilities(CapabilitiesData {
            capabilities: caps,
        })),
    }))
}
