// crates/mira-server/src/tools/core/session/storage.rs
//! Storage status, retention policy display, and data cleanup operations.

use crate::config::file::MiraConfig;
use crate::db::retention::{cleanup_orphans, count_retention_candidates, run_data_retention_sync};
use crate::error::MiraError;
use crate::mcp::responses::Json;
use crate::mcp::responses::SessionOutput;
use crate::tools::core::ToolContext;

/// Helper: count rows in a table, returning 0 if the table doesn't exist.
pub(super) fn count_table(conn: &rusqlite::Connection, table: &str) -> usize {
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

/// Show database storage status, row counts, and retention policy.
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
