// crates/mira-server/src/cli/cleanup.rs
// CLI handler for `mira cleanup` command

use anyhow::Result;
use mira::config::file::MiraConfig;
use mira::db::pool::DatabasePool;
use mira::db::retention;
use std::sync::Arc;

/// Map a table name to its category for filtering.
fn table_category(table: &str) -> &'static str {
    match table {
        "sessions"
        | "session_snapshots"
        | "session_tasks"
        | "session_task_iterations"
        | "tool_history" => "sessions",
        "llm_usage" | "embeddings_usage" => "analytics",
        "chat_messages" | "chat_summaries" => "chat",
        "behavior_patterns" | "system_observations" => "behavior",
        _ => "other",
    }
}

pub async fn run_cleanup(dry_run: bool, yes: bool, category: Option<String>) -> Result<()> {
    let db_path = super::get_db_path();

    if !db_path.exists() {
        println!("No Mira database found at {}", db_path.display());
        return Ok(());
    }

    let pool = Arc::new(DatabasePool::open(&db_path).await?);
    let config = MiraConfig::load().retention;

    // Show current retention policy
    println!("Retention policy:");
    if config.is_enabled() {
        println!("  Status: enabled");
        println!("  Tool history: {} days", config.tool_history_days);
        println!("  Chat messages: {} days", config.chat_days);
        println!("  Sessions: {} days", config.sessions_days);
        println!("  Analytics: {} days", config.analytics_days);
        println!("  Behavior: {} days", config.behavior_days);
        println!("  Observations: {} days", config.observations_days);
    } else {
        println!("  Status: disabled (only orphan cleanup will run)");
        println!("  Enable with: [retention] enabled = true in ~/.mira/config.toml");
    }
    println!();

    // Show what would be cleaned (dry-run preview)
    let preview_config = MiraConfig::load().retention;
    let candidates = pool
        .interact(move |conn| Ok(retention::count_retention_candidates(conn, &preview_config)))
        .await?;

    // Filter by category if specified
    let filter = category.as_deref().unwrap_or("all");
    let filtered: Vec<&(String, usize)> = candidates
        .iter()
        .filter(|(table, _)| filter == "all" || table_category(table) == filter)
        .collect();

    let total_candidates: usize = filtered.iter().map(|(_, c)| *c).sum();

    if total_candidates == 0 && !dry_run {
        println!("Nothing to clean up.");
        println!("\nRunning orphan cleanup...");
        let orphan_count = pool
            .interact(|conn| retention::cleanup_orphans(conn).map_err(|e| anyhow::anyhow!("{}", e)))
            .await?;
        if orphan_count > 0 {
            println!("  Cleaned {} orphaned rows", orphan_count);
        } else {
            println!("  No orphans found");
        }
        return Ok(());
    }

    if total_candidates > 0 {
        println!("Cleanup preview:");
        for (table, count) in &filtered {
            if *count > 0 {
                println!("  {} rows from {}", count, table);
            }
        }
        println!("  Total: {} rows eligible for cleanup", total_candidates);
        if filter != "all" {
            println!(
                "\n  Note: showing category '{}' only. Execution runs full cleanup.",
                filter
            );
        }
    }

    println!("\nProtected (never auto-deleted):");
    println!("  - Memories (use 'forget' to delete individually)");
    println!("  - Goals and milestones");
    println!("  - Active sessions");

    if dry_run {
        println!("\nDry run -- no changes made.");
        println!("Run `mira cleanup --execute` to apply, or `mira cleanup --execute --yes` to skip confirmation.");
        return Ok(());
    }

    // Confirmation
    if !yes {
        print!("\nProceed with cleanup? [y/N] ");
        use std::io::Write;
        std::io::stdout().flush()?;
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            println!("Cancelled.");
            return Ok(());
        }
    }

    // Execute cleanup
    println!("\nRunning cleanup...");

    if config.is_enabled() {
        let exec_config = MiraConfig::load().retention;
        let retention_count = pool
            .interact(move |conn| {
                retention::run_data_retention_sync(conn, &exec_config)
                    .map_err(|e| anyhow::anyhow!("{}", e))
            })
            .await?;
        println!("  Retention: deleted {} rows", retention_count);
    }

    let orphan_count = pool
        .interact(|conn| retention::cleanup_orphans(conn).map_err(|e| anyhow::anyhow!("{}", e)))
        .await?;
    println!("  Orphans: cleaned {} rows", orphan_count);

    println!("\nDone.");
    Ok(())
}
