// crates/mira-server/src/cli/analyze.rs
// CLI handler for `mira analyze-session`

use anyhow::{Context, Result, bail};
use std::path::PathBuf;

use mira::jsonl::{self, CorrelatedSession};
use mira::jsonl::calibration;

/// Format a number with comma separators (e.g. 1234567 -> "1,234,567")
fn fmt_num(n: u64) -> String {
    let s = n.to_string();
    let mut result = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }
    result.chars().rev().collect()
}

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
    println!("  API turns:        {}", fmt_num(summary.turn_count() as u64));
    println!("  User prompts:     {}", fmt_num(summary.user_prompt_count));
    println!("  Tool results:     {}", fmt_num(summary.tool_result_count));
    println!("  Input tokens:     {}", fmt_num(summary.total_input_tokens()));
    println!("  Output tokens:    {}", fmt_num(summary.total_output_tokens()));
    println!("  Cache read:       {}", fmt_num(summary.total_cache_read_tokens()));
    println!("  Cache creation:   {}", fmt_num(summary.total_cache_creation_tokens()));
    println!("  Billable input:   {}", fmt_num(summary.total_billable_input()));
    if summary.compaction_count > 0 {
        println!("  Compactions:      {}", fmt_num(summary.compaction_count));
    }
    if summary.parse_errors > 0 {
        println!("  Parse errors:     {}", fmt_num(summary.parse_errors));
    }
    println!();

    // Tool breakdown
    if show_tools {
        let mut tools: Vec<_> = summary.tool_calls.iter().collect();
        tools.sort_by(|a, b| b.1.cmp(a.1));

        println!("--- Tool Calls ({} total) ---", fmt_num(summary.total_tool_calls()));
        for (name, count) in &tools {
            println!("  {:<20} {}", name, fmt_num(**count));
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
        Err(e) => {
            eprintln!("Warning: calibration failed: {e}, using default");
            calibration::Calibration::default()
        }
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
                    println!("  Injections:           {}", fmt_num(corr.injections));
                    println!("  Chars injected:       {}", fmt_num(corr.injected_chars));
                    println!("  Est. injected tokens: {}", fmt_num(corr.estimated_injected_tokens));
                    if let Some(ratio) = corr.injection_overhead_ratio {
                        println!("  Overhead ratio:       {:.2}%", ratio * 100.0);
                    }
                    println!("  Deduped:              {}", fmt_num(corr.injections_deduped));
                    println!("  Cached:               {}", fmt_num(corr.injections_cached));
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
            eprintln!("Warning: Mira DB not found at {}, skipping correlation. Run Mira MCP server first.", db_path.display());
        }
    }

    Ok(())
}

/// Resolve a session argument to (jsonl_path, session_id).
///
/// Accepts:
///   - A path to a .jsonl file
///   - A session ID (UUID)
///   - None (uses most recent session for the current working directory)
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
        } else if path.exists() {
            bail!("File exists but is not a .jsonl file: {s}");
        }

        // Try as session ID
        if let Some(path) = jsonl::find_session_jsonl(s) {
            return Ok((path, s.clone()));
        }

        bail!("Could not find session JSONL for: {s}");
    }

    // No argument: find most recent JSONL scoped to the current working directory.
    // Claude Code stores sessions in ~/.claude/projects/<slug>/ where slug is
    // derived from the absolute CWD path (e.g. /home/user/Mira -> -home-user-Mira).
    let cwd = std::env::current_dir().ok();
    let project_dir = cwd.as_ref().and_then(|cwd| {
        // Claude Code slug: replace '/' with '-', keeping the leading dash
        // e.g. /home/peter/Mira -> -home-peter-Mira
        let slug = cwd.to_string_lossy().replace('/', "-");
        let dir = dirs::home_dir()?.join(".claude/projects").join(&*slug);
        if dir.exists() { Some(dir) } else { None }
    });

    if let Some(project_dir) = project_dir {
        let mut newest: Option<(PathBuf, std::time::SystemTime)> = None;

        for file in std::fs::read_dir(&project_dir)?.flatten() {
            let fpath = file.path();
            if fpath.extension().is_some_and(|e| e == "jsonl")
                && let Ok(meta) = fpath.metadata()
                && let Ok(modified) = meta.modified()
                && newest.as_ref().is_none_or(|(_, t)| modified > *t)
            {
                newest = Some((fpath, modified));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fmt_num() {
        assert_eq!(fmt_num(0), "0");
        assert_eq!(fmt_num(1), "1");
        assert_eq!(fmt_num(12), "12");
        assert_eq!(fmt_num(123), "123");
        assert_eq!(fmt_num(1234), "1,234");
        assert_eq!(fmt_num(12345), "12,345");
        assert_eq!(fmt_num(123456), "123,456");
        assert_eq!(fmt_num(1234567), "1,234,567");
        assert_eq!(fmt_num(1234567890), "1,234,567,890");
    }
}
