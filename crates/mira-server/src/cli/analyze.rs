// crates/mira-server/src/cli/analyze.rs
// CLI handler for `mira analyze-session`

use anyhow::{Context, Result, bail};
use std::path::PathBuf;

use mira::jsonl::{self, CorrelatedSession};
use mira::jsonl::calibration;

/// Run the analyze-session command.
pub fn run(
    session: Option<String>,
    show_turns: bool,
    show_tools: bool,
    correlate: bool,
) -> Result<()> {
    let (jsonl_path, session_id) = resolve_session(session)?;

    let summary = jsonl::parse_session_file(&jsonl_path)
        .with_context(|| format!("Failed to parse {}", jsonl_path.display()))?;

    // Header
    println!("Session: {}", session_id);
    if let Some(ref v) = summary.version {
        print!("  Claude Code: {v}");
    }
    if let Some(ref b) = summary.git_branch {
        print!("  Branch: {b}");
    }
    println!();
    if let (Some(first), Some(last)) = (&summary.first_timestamp, &summary.last_timestamp) {
        println!("  Time: {} -> {}", first, last);
    }
    println!();

    // Token summary
    println!("--- Token Usage ---");
    println!("  API turns:        {}", summary.turn_count());
    println!("  User prompts:     {}", summary.user_prompt_count);
    println!("  Tool results:     {}", summary.tool_result_count);
    println!("  Input tokens:     {}", summary.total_input_tokens());
    println!("  Output tokens:    {}", summary.total_output_tokens());
    println!("  Cache read:       {}", summary.total_cache_read_tokens());
    println!("  Cache creation:   {}", summary.total_cache_creation_tokens());
    println!("  Billable input:   {}", summary.total_billable_input());
    if summary.compaction_count > 0 {
        println!("  Compactions:      {}", summary.compaction_count);
    }
    println!();

    // Tool breakdown
    if show_tools || !summary.tool_calls.is_empty() {
        let mut tools: Vec<_> = summary.tool_calls.iter().collect();
        tools.sort_by(|a, b| b.1.cmp(a.1));

        println!("--- Tool Calls ({} total) ---", summary.total_tool_calls());
        for (name, count) in &tools {
            println!("  {:<20} {}", name, count);
        }
        println!();
    }

    // Per-turn breakdown
    if show_turns {
        println!("--- Turns ---");
        for (i, turn) in summary.turns.iter().enumerate() {
            let tools = if turn.tool_calls.is_empty() {
                String::new()
            } else {
                format!(" [{}]", turn.tool_calls.join(", "))
            };
            let sidechain = if turn.is_sidechain { " (sidechain)" } else { "" };
            println!(
                "  {:>4}. in={:<6} out={:<6} cache_r={:<7} cache_c={:<6}{}{}",
                i + 1,
                turn.usage.input_tokens,
                turn.usage.output_tokens,
                turn.usage.cache_read_input_tokens,
                turn.usage.cache_creation_input_tokens,
                tools,
                sidechain,
            );
        }
        println!();
    }

    // Calibration
    let cal = match calibration::calibrate_from_file(&jsonl_path) {
        Ok(c) => {
            if !c.is_default {
                println!("--- Calibration ---");
                println!("  Chars/token:  {:.2} (from {} samples)", c.chars_per_token, c.sample_count);
                println!();
            }
            c
        }
        Err(_) => calibration::Calibration::default(),
    };

    // Injection correlation
    if correlate {
        let db_path = super::get_db_path();
        if db_path.exists() {
            match rusqlite::Connection::open(&db_path) {
                Ok(conn) => {
                    let stats = match mira::db::injection::get_injection_stats_for_session(
                        &conn,
                        &session_id,
                    ) {
                        Ok(s) => s,
                        Err(e) => {
                            eprintln!("Warning: Could not query injection stats: {e}");
                            eprintln!("  (Run the MCP server first to create the context_injections table)");
                            return Ok(());
                        }
                    };

                    let corr = CorrelatedSession::from_summary_and_stats_calibrated(
                        &session_id,
                        &summary,
                        &stats,
                        &cal,
                    );

                    println!("--- Mira Injection Correlation ---");
                    println!("  Injections:           {}", corr.injections);
                    println!("  Chars injected:       {}", corr.injected_chars);
                    println!("  Est. injected tokens: {}", corr.estimated_injected_tokens);
                    if let Some(ratio) = corr.injection_overhead_ratio {
                        println!("  Overhead ratio:       {:.2}%", ratio * 100.0);
                    }
                    println!("  Deduped:              {}", corr.injections_deduped);
                    println!("  Cached:               {}", corr.injections_cached);
                    if let Some(rate) = corr.dedup_rate {
                        println!("  Dedup rate:           {:.1}%", rate * 100.0);
                    }
                    if let Some(lat) = corr.injection_avg_latency_ms {
                        println!("  Avg latency:          {:.1}ms", lat);
                    }
                    println!();
                }
                Err(e) => {
                    eprintln!("Warning: Could not open Mira DB for correlation: {e}");
                }
            }
        } else {
            eprintln!("Warning: Mira DB not found at {}, skipping correlation", db_path.display());
        }
    }

    Ok(())
}

/// Resolve a session argument to (jsonl_path, session_id).
///
/// Accepts:
///   - A path to a .jsonl file
///   - A session ID (UUID)
///   - None (uses most recent session in the current project)
fn resolve_session(session: Option<String>) -> Result<(PathBuf, String)> {
    if let Some(ref s) = session {
        // Check if it's a file path
        let path = PathBuf::from(s);
        if path.exists() && path.extension().is_some_and(|e| e == "jsonl") {
            let session_id = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown")
                .to_string();
            return Ok((path, session_id));
        }

        // Try as session ID
        if let Some(path) = jsonl::find_session_jsonl(s) {
            return Ok((path, s.clone()));
        }

        bail!("Could not find session JSONL for: {s}");
    }

    // No argument: find most recent JSONL in any project directory
    let claude_dir = dirs::home_dir()
        .map(|h| h.join(".claude/projects"))
        .filter(|p| p.exists());

    if let Some(claude_dir) = claude_dir {
        let mut newest: Option<(PathBuf, std::time::SystemTime)> = None;

        for project_dir in std::fs::read_dir(&claude_dir)?.flatten() {
            if !project_dir.file_type()?.is_dir() {
                continue;
            }
            for file in std::fs::read_dir(project_dir.path())?.flatten() {
                let fpath = file.path();
                if fpath.extension().is_some_and(|e| e == "jsonl") {
                    if let Ok(meta) = fpath.metadata() {
                        if let Ok(modified) = meta.modified() {
                            if newest.as_ref().is_none_or(|(_, t)| modified > *t) {
                                newest = Some((fpath, modified));
                            }
                        }
                    }
                }
            }
        }

        if let Some((path, _)) = newest {
            let session_id = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown")
                .to_string();
            return Ok((path, session_id));
        }
    }

    bail!("No session JSONL files found. Provide a session ID or file path.");
}
