// crates/mira-server/src/tools/core/session/storage.rs
//! Storage status, retention policy display, and data cleanup operations.

use crate::config::file::MiraConfig;
use crate::db::retention::{cleanup_orphans, count_retention_candidates, run_data_retention_sync};
use crate::error::MiraError;
use crate::mcp::responses::Json;
use crate::mcp::responses::SessionOutput;
use crate::tools::core::ToolContext;

/// Compile-time-safe enumeration of tables that may be counted.
/// Prevents SQL injection via table name interpolation.
#[allow(dead_code)]
pub(super) enum AllowedTable {
    Sessions,
    ToolHistory,
    LlmUsage,
    EmbeddingsUsage,
    BehaviorPatterns,
    Goals,
    SystemObservations,
}

impl AllowedTable {
    pub(super) fn as_str(&self) -> &'static str {
        match self {
            AllowedTable::Sessions => "sessions",
            AllowedTable::ToolHistory => "tool_history",
            AllowedTable::LlmUsage => "llm_usage",
            AllowedTable::EmbeddingsUsage => "embeddings_usage",
            AllowedTable::BehaviorPatterns => "behavior_patterns",
            AllowedTable::Goals => "goals",
            AllowedTable::SystemObservations => "system_observations",
        }
    }
}

/// Helper: count rows in a table, returning 0 if the table doesn't exist.
pub(super) fn count_table(conn: &rusqlite::Connection, table: AllowedTable) -> usize {
    let sql = format!("SELECT COUNT(*) FROM {}", table.as_str());
    conn.query_row(&sql, [], |row| row.get::<_, usize>(0))
        .unwrap_or(0)
}

/// Helper: format byte size in human-readable form.
pub(super) fn format_bytes(bytes: u64) -> String {
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

/// Format characters as human-readable KB/MB.
fn format_chars_as_kb(chars: u64) -> String {
    // Approximate: 1 char ~= 1 byte for ASCII-heavy content
    format_bytes(chars)
}

/// Show database storage status, row counts, retention policy, and injection activity.
pub(super) async fn storage_status<C: ToolContext>(
    ctx: &C,
) -> Result<Json<SessionOutput>, MiraError> {
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

    // Gather session_id and project_id for injection stats
    let session_id = ctx.get_session_id().await;
    let project_id = ctx.project_id().await;

    // Query row counts and injection stats from main DB
    let session_id_clone = session_id.clone();
    let counts_and_activity = ctx
        .pool()
        .run(move |conn| {
            let sessions = count_table(conn, AllowedTable::Sessions);
            let tool_history = count_table(conn, AllowedTable::ToolHistory);
            let llm_usage = count_table(conn, AllowedTable::LlmUsage);
            let embed_usage = count_table(conn, AllowedTable::EmbeddingsUsage);
            let behavior = count_table(conn, AllowedTable::BehaviorPatterns);
            let goals = count_table(conn, AllowedTable::Goals);
            let observations = count_table(conn, AllowedTable::SystemObservations);

            // Injection activity stats
            let session_stats = session_id_clone.as_deref().and_then(|sid| {
                crate::db::injection::get_injection_stats_for_session(conn, sid).ok()
            });
            let session_categories = session_id_clone.as_deref().and_then(|sid| {
                crate::db::injection::get_category_breakdown_sync(conn, Some(sid), None).ok()
            });

            let cumulative_stats =
                crate::db::injection::get_injection_stats_cumulative(conn, project_id, None).ok();
            let tracked_sessions =
                crate::db::injection::count_tracked_sessions(conn, project_id).ok();

            // Value heuristics
            let session_heuristics = session_id_clone.as_deref().and_then(|sid| {
                crate::db::injection::compute_value_heuristics_sync(conn, Some(sid), None).ok()
            });
            let cumulative_heuristics =
                crate::db::injection::compute_value_heuristics_sync(conn, None, project_id).ok();

            Ok::<_, String>((
                sessions,
                tool_history,
                llm_usage,
                embed_usage,
                behavior,
                goals,
                observations,
                session_stats,
                session_categories,
                cumulative_stats,
                tracked_sessions,
                session_heuristics,
                cumulative_heuristics,
            ))
        })
        .await?;

    let (
        sessions,
        tool_history,
        llm_usage,
        embed_usage,
        behavior,
        goals,
        observations,
        session_stats,
        session_categories,
        cumulative_stats,
        tracked_sessions,
        session_heuristics,
        cumulative_heuristics,
    ) = counts_and_activity;

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
    report.push_str("- Goals, Code index\n");

    // Injection activity section
    let has_session_activity = session_stats
        .as_ref()
        .is_some_and(|s| s.total_injections > 0);
    let has_cumulative_activity = cumulative_stats
        .as_ref()
        .is_some_and(|s| s.total_injections > 0);

    if has_session_activity || has_cumulative_activity {
        report.push_str("\n### Activity\n");

        if let Some(ref stats) = session_stats {
            if stats.total_injections > 0 {
                report.push_str("**This session:**\n");
                report.push_str(&format!(
                    "- Context delivered: {} injections ({})\n",
                    stats.total_injections,
                    format_chars_as_kb(stats.total_chars)
                ));
                if stats.total_deduped > 0 {
                    report.push_str(&format!(
                        "- Deduped (suppressed): {}\n",
                        stats.total_deduped
                    ));
                }
                if let Some(ref cats) = session_categories {
                    if !cats.is_empty() {
                        let mut sorted: Vec<_> = cats.iter().collect();
                        sorted.sort_by(|a, b| b.1.cmp(a.1));
                        let parts: Vec<String> =
                            sorted.iter().map(|(k, v)| format!("{} ({})", k, v)).collect();
                        report.push_str(&format!("- Sources: {}\n", parts.join(", ")));
                    }
                }
            }
        }

        if let Some(ref stats) = cumulative_stats {
            if stats.total_injections > 0 {
                report.push_str("**All time:**\n");
                report.push_str(&format!(
                    "- Context delivered: {} injections ({})\n",
                    stats.total_injections,
                    format_chars_as_kb(stats.total_chars)
                ));
                if let Some(tracked) = tracked_sessions {
                    report.push_str(&format!("- Sessions tracked: {}\n", tracked));
                }
                report.push_str(&format!("- Goals tracked: {}\n", goals));
            }
        }
    }

    // Value signals section
    let show_value_signals = |h: &crate::db::injection::ValueHeuristics| -> Vec<String> {
        let mut lines = Vec::new();
        if h.file_reread_hints > 0 {
            lines.push(format!(
                "  Stale file re-reads prevented: ~{}",
                h.file_reread_hints
            ));
        }
        if h.subagent_total > 0 {
            let pct =
                (h.subagent_context_loads as f64 / h.subagent_total as f64 * 100.0).round() as u64;
            lines.push(format!(
                "  Subagent context hits: {}% had pre-loaded context ({}/{})",
                pct, h.subagent_context_loads, h.subagent_total
            ));
        }
        if h.goal_injected_sessions > 0 {
            let pct = if h.goal_aware_sessions > 0 {
                (h.goal_aware_sessions as f64 / h.goal_injected_sessions as f64 * 100.0).round()
                    as u64
            } else {
                0
            };
            if h.goal_aware_sessions > 0 {
                lines.push(format!(
                    "  Goal awareness: {}% of goal-injected sessions used goals ({}/{})",
                    pct, h.goal_aware_sessions, h.goal_injected_sessions
                ));
            } else {
                lines.push(format!(
                    "  Goal-injected sessions: {}",
                    h.goal_injected_sessions
                ));
            }
        }
        lines
    };

    let session_lines = session_heuristics
        .as_ref()
        .map(show_value_signals)
        .unwrap_or_default();
    let cumulative_lines = cumulative_heuristics
        .as_ref()
        .map(show_value_signals)
        .unwrap_or_default();

    if !session_lines.is_empty() || !cumulative_lines.is_empty() {
        report.push_str("\n### Value Signals\n");
        if !session_lines.is_empty() {
            report.push_str("**This session:**\n");
            for line in &session_lines {
                report.push_str(line);
                report.push('\n');
            }
        }
        if !cumulative_lines.is_empty() {
            report.push_str("**All time:**\n");
            for line in &cumulative_lines {
                report.push_str(line);
                report.push('\n');
            }
        }
    }

    Ok(Json(SessionOutput {
        action: "storage_status".into(),
        message: report,
        data: None,
    }))
}

/// Run data cleanup with dry-run preview by default.
pub(super) async fn cleanup<C: ToolContext>(
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
        report.push_str("- Goals, Code index\n");
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
