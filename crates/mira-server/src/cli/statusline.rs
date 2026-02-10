// crates/mira-server/src/cli/statusline.rs
// Status line output for Claude Code — queries Mira databases for live stats.
// Designed to be fast (<50ms). All queries use indexed columns.

use anyhow::Result;
use rusqlite::Connection;
use std::io::BufRead;
use std::path::PathBuf;

/// ANSI escape codes
const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";
const CYAN: &str = "\x1b[36m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const MAGENTA: &str = "\x1b[35m";

const DOT: &str = " \x1b[2m\u{00b7}\x1b[0m ";

/// Parse the `cwd` field from the JSON that Claude Code sends on stdin.
/// Reads a single line instead of read_to_string to avoid blocking if stdin stays open.
fn parse_cwd_from_stdin() -> Option<String> {
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line).ok()?;
    let v: serde_json::Value = serde_json::from_str(&line).ok()?;
    v.get("cwd")?.as_str().map(|s| s.to_string())
}

/// Resolve project by matching cwd against the projects table.
/// Returns (project_id, project_name).
fn resolve_project(conn: &Connection, cwd: &str) -> Option<(i64, String)> {
    conn.query_row(
        "SELECT id, COALESCE(name, '') FROM projects \
         WHERE ?1 = path OR ?1 LIKE path || '/%' \
         ORDER BY LENGTH(path) DESC LIMIT 1",
        [cwd],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .ok()
}


/// Count active goals for a project.
fn query_goals(conn: &Connection, project_id: i64) -> i64 {
    conn.query_row(
        "SELECT COUNT(*) FROM goals \
         WHERE project_id = ?1 AND status NOT IN ('completed', 'abandoned')",
        [project_id],
        |r| r.get(0),
    )
    .unwrap_or(0)
}

/// Count non-dismissed insights from the last 7 days.
/// Returns (new_count, total_count) where new = shown_count == 0.
fn query_insights(conn: &Connection, project_id: i64) -> (i64, i64) {
    conn.query_row(
        "SELECT \
           COALESCE(SUM(CASE WHEN shown_count = 0 THEN 1 ELSE 0 END), 0), \
           COUNT(*) \
         FROM behavior_patterns \
         WHERE project_id = ?1 \
           AND pattern_type LIKE 'insight_%' \
           AND first_seen_at > datetime('now', '-7 days') \
           AND (dismissed IS NULL OR dismissed = 0)",
        [project_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .unwrap_or((0, 0))
}

/// Get how long ago the code index was last updated, as a human-readable string.
fn query_index_age(conn: &Connection, project_id: i64) -> Option<String> {
    let seconds: i64 = conn
        .query_row(
            "SELECT CAST((julianday('now') - julianday(MAX(indexed_at))) * 86400 AS INTEGER) \
             FROM code_symbols WHERE project_id = ?1",
            [project_id],
            |r| r.get(0),
        )
        .ok()?;

    Some(format_duration(seconds))
}

/// Count pending/draft documentation tasks for a project.
fn query_stale_docs(conn: &Connection, project_id: i64) -> i64 {
    conn.query_row(
        "SELECT COUNT(*) FROM documentation_tasks \
         WHERE project_id = ?1 AND status IN ('pending', 'draft_ready')",
        [project_id],
        |r| r.get(0),
    )
    .unwrap_or(0)
}

/// Check if background processing is stalled.
/// Uses the slow lane heartbeat (written every cycle) instead of pondering timestamps,
/// which have many legitimate reasons to go stale (cooldown, insufficient data, etc.).
/// Returns true if the heartbeat is >5 minutes old (and has ever been written).
fn query_bg_stalled(conn: &Connection) -> bool {
    conn.query_row(
        "SELECT CAST((julianday('now') - julianday(value)) * 86400 AS INTEGER) \
         FROM server_state WHERE key = 'last_bg_heartbeat'",
        [],
        |r| r.get::<_, i64>(0),
    )
    .map(|secs| secs > 300) // 5 minutes
    .unwrap_or(false) // never ran = don't warn (fresh install)
}

/// Count pending embedding chunks.
fn query_pending(conn: &Connection, project_id: i64) -> i64 {
    conn.query_row(
        "SELECT COUNT(*) FROM pending_embeddings \
         WHERE project_id = ?1 AND status = 'pending'",
        [project_id],
        |r| r.get(0),
    )
    .unwrap_or(0)
}

/// Format seconds into a compact human-readable duration.
fn format_duration(seconds: i64) -> String {
    if seconds < 60 {
        "just now".to_string()
    } else if seconds < 3600 {
        format!("{}m ago", seconds / 60)
    } else if seconds < 86400 {
        format!("{}h ago", seconds / 3600)
    } else {
        format!("{}d ago", seconds / 86400)
    }
}

fn open_readonly(path: &PathBuf) -> Option<Connection> {
    let conn = Connection::open_with_flags(
        path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .ok()?;
    // Set busy_timeout to avoid SQLITE_BUSY during write-heavy workloads (indexing).
    let _ = conn.execute_batch("PRAGMA busy_timeout = 1000");
    Some(conn)
}

/// Print the status line to stdout.
pub fn run() -> Result<()> {
    let cwd = parse_cwd_from_stdin();

    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let mira_dir = home.join(".mira");
    let main_db = mira_dir.join("mira.db");
    let code_db = mira_dir.join("mira-code.db");

    if !main_db.exists() {
        return Ok(());
    }

    let main_conn = match open_readonly(&main_db) {
        Some(c) => c,
        None => return Ok(()),
    };

    // Resolve project from cwd
    let project = cwd
        .as_deref()
        .and_then(|cwd| resolve_project(&main_conn, cwd));

    let (project_id, project_name) = match project {
        Some((id, name)) => (id, name),
        None => {
            // No project found — show minimal line
            println!("{DIM}\u{1d5c6}\u{1d5c2}\u{1d5cb}\u{1d5ba}{RESET}");
            return Ok(());
        }
    };

    // Query stats
    let goals = query_goals(&main_conn, project_id);
    let (new_insights, total_insights) = query_insights(&main_conn, project_id);
    let stale_docs = query_stale_docs(&main_conn, project_id);
    let bg_stalled = query_bg_stalled(&main_conn);

    let code_conn = open_readonly(&code_db);
    let index_age = code_conn
        .as_ref()
        .and_then(|c| query_index_age(c, project_id));
    let pending = code_conn
        .as_ref()
        .map(|c| query_pending(c, project_id))
        .unwrap_or(0);

    // Build output: ᓚᘏᗢ ProjectName · 3 goals · indexed 2h ago · 7 insights (2 new)
    let mut parts = Vec::new();

    if !project_name.is_empty() {
        parts.push(format!("{CYAN}{project_name}{RESET}"));
    }

    if goals > 0 {
        parts.push(format!("{GREEN}{goals}{RESET} goals"));
    }

    // Actionable items first
    if total_insights > 0 {
        if new_insights > 0 {
            parts.push(format!(
                "{MAGENTA}{total_insights}{RESET} insights ({MAGENTA}{new_insights} new{RESET})"
            ));
        } else {
            parts.push(format!("{DIM}{total_insights} insights{RESET}"));
        }
    }

    if stale_docs > 0 {
        parts.push(format!("{YELLOW}{stale_docs} stale docs{RESET}"));
    }

    // Informational / diagnostics
    if bg_stalled {
        parts.push(format!("{YELLOW}bg stalled{RESET}"));
    }

    if pending > 0 {
        parts.push(format!("{YELLOW}embedding {pending} chunks{RESET}"));
    }

    if let Some(age) = &index_age {
        parts.push(format!("{DIM}indexed {age}{RESET}"));
    }

    let joined = parts.join(DOT);
    println!("{DIM}\u{14da}\u{160f}\u{15e2}{RESET} {joined}");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_duration_just_now() {
        assert_eq!(format_duration(0), "just now");
        assert_eq!(format_duration(59), "just now");
    }

    #[test]
    fn test_format_duration_minutes() {
        assert_eq!(format_duration(60), "1m ago");
        assert_eq!(format_duration(300), "5m ago");
        assert_eq!(format_duration(3599), "59m ago");
    }

    #[test]
    fn test_format_duration_hours() {
        assert_eq!(format_duration(3600), "1h ago");
        assert_eq!(format_duration(7200), "2h ago");
        assert_eq!(format_duration(86399), "23h ago");
    }

    #[test]
    fn test_format_duration_days() {
        assert_eq!(format_duration(86400), "1d ago");
        assert_eq!(format_duration(172800), "2d ago");
    }
}
