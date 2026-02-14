// crates/mira-server/src/tools/core/session.rs
// Unified session management tools

use crate::config::file::{MiraConfig, RetentionConfig};
use crate::db::retention::{cleanup_orphans, count_retention_candidates, run_data_retention_sync};
use crate::db::{
    build_session_recap_sync, compute_age_days, create_session_ext_sync, dismiss_insight_sync,
    get_recent_sessions_sync, get_session_history_sync, get_unified_insights_sync,
};
use crate::hooks::session::{read_claude_session_id, read_source_info};
use crate::mcp::responses::Json;
use crate::mcp::responses::{
    HistoryEntry, InsightItem, InsightsData, SessionCurrentData, SessionData, SessionHistoryData,
    SessionListData, SessionOutput, SessionSummary,
};
use crate::tools::core::session_notes;
use crate::tools::core::{NO_ACTIVE_PROJECT_ERROR, ToolContext};
use crate::utils::{truncate, truncate_at_boundary};
use uuid::Uuid;

/// Unified session tool dispatcher
pub async fn handle_session<C: ToolContext>(
    ctx: &C,
    req: crate::mcp::requests::SessionRequest,
) -> Result<Json<SessionOutput>, String> {
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
            let message = super::usage_summary(ctx, req.since_days, req.limit).await?;
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
            query_insights(
                ctx,
                req.insight_source,
                req.min_confidence,
                req.since_days,
                req.limit,
            )
            .await
        }
        SessionAction::DismissInsight => dismiss_insight(ctx, req.insight_id).await,
        SessionAction::StorageStatus => storage_status(ctx).await,
        SessionAction::Cleanup => cleanup(ctx, req.dry_run, req.category).await,
        SessionAction::TasksList | SessionAction::TasksGet | SessionAction::TasksCancel => {
            // Tasks actions need MiraServer directly, not ToolContext
            // This branch is unreachable in MCP (router intercepts) but needed for CLI
            Err("Tasks actions must be handled by the router/CLI dispatcher directly.".into())
        }
    }
}

/// Map a tech-debt score to a letter-grade tier label.
fn score_to_tier_label(score: f64) -> &'static str {
    match score as u32 {
        0..=20 => "A (Healthy)",
        21..=40 => "B (Moderate)",
        41..=60 => "C (Needs Work)",
        61..=80 => "D (Poor)",
        _ => "F (Critical)",
    }
}

/// Category display order and human-readable labels.
const CATEGORY_ORDER: &[(&str, &str)] = &[
    ("attention", "Attention Required"),
    ("quality", "Code Quality"),
    ("testing", "Testing & Reliability"),
    ("workflow", "Workflow"),
    ("documentation", "Documentation"),
    ("health", "Health Trend"),
    ("other", "Other"),
];

/// Query unified insights digest, formatted as a categorized Health Dashboard.
async fn query_insights<C: ToolContext>(
    ctx: &C,
    insight_source: Option<String>,
    min_confidence: Option<f64>,
    since_days: Option<u32>,
    limit: Option<i64>,
) -> Result<Json<SessionOutput>, String> {
    use std::collections::BTreeMap;

    let project = ctx.get_project().await;
    let project_id = project
        .as_ref()
        .map(|p| p.id)
        .ok_or(NO_ACTIVE_PROJECT_ERROR)?;

    let filter_source = insight_source.clone();
    let min_conf = min_confidence.unwrap_or(0.5);
    let days_back = since_days.unwrap_or(30) as i64;
    let lim = limit.unwrap_or(20).max(0) as usize;

    let project_id_for_snapshot = project_id;

    let insights = ctx
        .pool()
        .run(move |conn| {
            get_unified_insights_sync(
                conn,
                project_id,
                filter_source.as_deref(),
                min_conf,
                days_back,
                lim,
            )
        })
        .await?;

    // Fetch latest health snapshot for the dashboard header
    let snapshot_summary = ctx
        .pool()
        .run(move |conn| {
            let result: Option<(f64, i64, String)> = conn
                .query_row(
                    "SELECT avg_debt_score, module_count, tier_distribution
                     FROM health_snapshots WHERE project_id = ?1
                     ORDER BY snapshot_at DESC LIMIT 1",
                    rusqlite::params![project_id_for_snapshot],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .ok();
            Ok::<_, String>(result)
        })
        .await?;

    // Handle empty state
    if insights.is_empty() {
        let empty_msg = if snapshot_summary.is_some() {
            "No active insights. Codebase is looking healthy.".to_string()
        } else {
            "No insights found.\n\nTo generate health data:\n  \
             1. Index your project: index(action=\"project\")\n  \
             2. Run health scan: index(action=\"health\")\n  \
             3. Then check insights again"
                .to_string()
        };
        return Ok(Json(SessionOutput {
            action: "insights".into(),
            message: empty_msg,
            data: Some(SessionData::Insights(InsightsData {
                insights: vec![],
                total: 0,
            })),
        }));
    }

    // ── Build categorized dashboard ──

    // Separate high-priority items into "attention" bucket; track their indices
    // so they don't also appear under their original category.
    let mut attention_indices: Vec<usize> = Vec::new();
    for (i, insight) in insights.iter().enumerate() {
        if insight.priority_score >= 0.75 {
            attention_indices.push(i);
        }
    }

    // Group remaining insights by category
    let mut by_category: BTreeMap<String, Vec<(usize, &crate::db::UnifiedInsight)>> =
        BTreeMap::new();
    for (i, insight) in insights.iter().enumerate() {
        if attention_indices.contains(&i) {
            by_category
                .entry("attention".to_string())
                .or_default()
                .push((i, insight));
        } else {
            let cat = insight.category.as_deref().unwrap_or("other").to_string();
            by_category.entry(cat).or_default().push((i, insight));
        }
    }

    // ── Dashboard header ──
    let mut output = String::from("## Project Health Dashboard\n\n");
    if let Some((avg_score, module_count, ref tier_dist)) = snapshot_summary {
        let tier = score_to_tier_label(avg_score);
        output.push_str(&format!(
            "### Overall: {} | Score: {:.1} | {} modules\n",
            tier, avg_score, module_count
        ));
        // Parse tier distribution for summary
        if let Ok(tiers) = serde_json::from_str::<serde_json::Value>(tier_dist)
            && let Some(obj) = tiers.as_object()
        {
            let tier_summary: Vec<String> =
                obj.iter().map(|(k, v)| format!("{}:{}", k, v)).collect();
            if !tier_summary.is_empty() {
                output.push_str(&format!("    tiers: {{{}}}\n", tier_summary.join(",")));
            }
        }
        output.push_str("\n---\n\n");
    }

    // ── Category sections (skip empty) ──
    for &(cat_key, cat_label) in CATEGORY_ORDER {
        let entries = by_category.get(cat_key);
        let count = entries.map_or(0, |v| v.len());
        if count == 0 {
            continue;
        }
        output.push_str(&format!("### {} ({})\n\n", cat_label, count));

        if let Some(entries) = entries {
            for (_idx, insight) in entries {
                let indicator = if insight.priority_score >= 0.75 {
                    "[!!]"
                } else if insight.priority_score >= 0.5 {
                    "[!]"
                } else {
                    "[ ]"
                };
                let age = format_age(&insight.timestamp);
                output.push_str(&format!("  {} {}{}\n", indicator, insight.description, age));
                if let Some(ref trend) = insight.trend
                    && let Some(ref summary) = insight.change_summary
                {
                    output.push_str(&format!("       Trend: {} ({})\n", trend, summary));
                }
                if let Some(ref evidence) = insight.evidence {
                    output.push_str(&format!("       {}\n", evidence));
                }
                output.push('\n');
            }
        }
    }

    // ── Footer ──
    let dismissable = insights.iter().filter(|i| i.row_id.is_some()).count();
    if dismissable > 0 {
        output.push_str("---\n");
        output.push_str(&format!(
            "{} dismissable (use session action=dismiss_insight insight_id=<row_id>)\n",
            dismissable
        ));
    }

    // ── Build InsightItem vec ──
    let items: Vec<InsightItem> = insights
        .iter()
        .enumerate()
        .map(|(i, insight)| {
            let item_category = if attention_indices.contains(&i) {
                Some("attention".to_string())
            } else {
                insight.category.clone()
            };
            InsightItem {
                row_id: insight.row_id,
                source: insight.source.clone(),
                source_type: insight.source_type.clone(),
                description: insight.description.clone(),
                priority_score: insight.priority_score,
                confidence: insight.confidence,
                evidence: insight.evidence.clone(),
                trend: insight.trend.clone(),
                change_summary: insight.change_summary.clone(),
                category: item_category,
            }
        })
        .collect();

    // Fire-and-forget: mark all returned pondering insights as shown by row ID
    let row_ids: Vec<i64> = insights.iter().filter_map(|i| i.row_id).collect();
    if !row_ids.is_empty() {
        let pool = ctx.pool().clone();
        tokio::spawn(async move {
            let _ = pool
                .run(move |conn| {
                    let placeholders: String =
                        row_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
                    let sql = format!(
                        "UPDATE behavior_patterns \
                         SET shown_count = COALESCE(shown_count, 0) + 1 \
                         WHERE id IN ({})",
                        placeholders
                    );
                    if let Err(e) = conn.execute(&sql, rusqlite::params_from_iter(row_ids.iter())) {
                        tracing::warn!("Failed to update insight shown_count: {}", e);
                    }
                    Ok::<_, String>(())
                })
                .await;
        });
    }

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

/// Dismiss a single insight by row ID
async fn dismiss_insight<C: ToolContext>(
    ctx: &C,
    insight_id: Option<i64>,
) -> Result<Json<SessionOutput>, String> {
    let id = insight_id.ok_or("insight_id is required for dismiss_insight action")?;

    let project = ctx.get_project().await;
    let project_id = project
        .as_ref()
        .map(|p| p.id)
        .ok_or(NO_ACTIVE_PROJECT_ERROR)?;

    let updated = ctx
        .pool()
        .run(move |conn| {
            dismiss_insight_sync(conn, project_id, id)
                .map_err(|e| format!("Failed to dismiss insight: {}", e))
        })
        .await?;

    let message = if updated {
        format!("Insight {} dismissed.", id)
    } else {
        format!("Insight {} not found or already dismissed.", id)
    };

    Ok(Json(SessionOutput {
        action: "dismiss_insight".into(),
        message,
        data: None,
    }))
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
) -> Result<Json<SessionOutput>, String> {
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
                .ok_or(NO_ACTIVE_PROJECT_ERROR)?;

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

/// Get session recap for MCP clients
/// Returns recent context, preferences, project state, and Claude Code session notes
pub async fn get_session_recap<C: ToolContext>(ctx: &C) -> Result<String, String> {
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
        "chat_messages",
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
async fn storage_status<C: ToolContext>(ctx: &C) -> Result<Json<SessionOutput>, String> {
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
            let chat = count_table(conn, "chat_messages");
            let llm_usage = count_table(conn, "llm_usage");
            let embed_usage = count_table(conn, "embeddings_usage");
            let behavior = count_table(conn, "behavior_patterns");
            let goals = count_table(conn, "goals");
            let observations = count_table(conn, "system_observations");

            Ok::<_, String>((
                memories,
                sessions,
                tool_history,
                chat,
                llm_usage,
                embed_usage,
                behavior,
                goals,
                observations,
            ))
        })
        .await?;

    let (
        memories,
        sessions,
        tool_history,
        chat,
        llm_usage,
        embed_usage,
        behavior,
        goals,
        observations,
    ) = counts;

    // Load retention config and count candidates
    let config = MiraConfig::load();
    let retention = config.retention;

    let retention_clone = RetentionConfig {
        enabled: retention.enabled,
        tool_history_days: retention.tool_history_days,
        chat_days: retention.chat_days,
        sessions_days: retention.sessions_days,
        analytics_days: retention.analytics_days,
        behavior_days: retention.behavior_days,
        observations_days: retention.observations_days,
    };

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
    report.push_str(&format!("- Chat messages: {}\n", chat));
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
    report.push_str(&format!("- Chat: {} days\n", retention.chat_days));
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

/// Build a `RetentionConfig` clone suitable for moving into closures.
fn clone_retention(r: &RetentionConfig) -> RetentionConfig {
    RetentionConfig {
        enabled: r.enabled,
        tool_history_days: r.tool_history_days,
        chat_days: r.chat_days,
        sessions_days: r.sessions_days,
        analytics_days: r.analytics_days,
        behavior_days: r.behavior_days,
        observations_days: r.observations_days,
    }
}

/// Run data cleanup with dry-run preview by default.
async fn cleanup<C: ToolContext>(
    ctx: &C,
    dry_run: Option<bool>,
    category: Option<String>,
) -> Result<Json<SessionOutput>, String> {
    let dry_run = dry_run.unwrap_or(true);

    if dry_run {
        // Preview mode: show what WOULD be deleted
        let config = MiraConfig::load();
        let retention = config.retention;
        let retention_clone = clone_retention(&retention);

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
                        "sessions"
                            | "session_snapshots"
                            | "session_tasks"
                            | "session_task_iterations"
                            | "tool_history"
                    ),
                    "analytics" => {
                        matches!(table.as_str(), "llm_usage" | "embeddings_usage")
                    }
                    "chat" => matches!(table.as_str(), "chat_messages" | "chat_summaries"),
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
        let retention_clone = clone_retention(&retention);

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

/// Format an insight timestamp as a human-readable age suffix.
fn format_age(timestamp: &str) -> String {
    let age_days = compute_age_days(timestamp);
    if age_days < 1.0 {
        " (today)".to_string()
    } else if age_days < 2.0 {
        " (yesterday)".to_string()
    } else if age_days < 7.0 {
        format!(" ({} days ago)", age_days as i64)
    } else if age_days < 14.0 {
        " (last week)".to_string()
    } else {
        format!(" ({} weeks ago)", (age_days / 7.0) as i64)
    }
}
