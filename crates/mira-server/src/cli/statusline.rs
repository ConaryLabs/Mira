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
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";

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

/// Count distinct indexed files for a project (uses code DB).
fn query_indexed_files(conn: &Connection, project_id: i64) -> i64 {
    conn.query_row(
        "SELECT COUNT(DISTINCT file_path) FROM code_symbols WHERE project_id = ?1",
        [project_id],
        |r| r.get(0),
    )
    .unwrap_or(0)
}

/// Count high-priority alerts: recurring errors and revert clusters only.
fn query_alerts(conn: &Connection, project_id: i64) -> i64 {
    conn.query_row(
        "SELECT COUNT(*) FROM behavior_patterns \
         WHERE project_id = ?1 \
           AND pattern_type IN ('insight_recurring_error', 'insight_revert_cluster') \
           AND (dismissed IS NULL OR dismissed = 0) \
           AND first_seen_at > datetime('now', '-7 days')",
        [project_id],
        |r| r.get(0),
    )
    .unwrap_or(0)
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

/// Get the active session ID from server_state.
fn query_active_session(conn: &Connection) -> Option<String> {
    conn.query_row(
        "SELECT value FROM server_state WHERE key = 'active_session_id'",
        [],
        |r| r.get(0),
    )
    .ok()
}

/// Count non-deduped context injections (assists).
/// If session_id is Some, scopes to that session; otherwise uses project_id.
fn query_assists(conn: &Connection, session_id: Option<&str>, project_id: i64) -> i64 {
    if let Some(sid) = session_id {
        conn.query_row(
            "SELECT COUNT(*) FROM context_injections \
             WHERE session_id = ?1 AND was_deduped = 0",
            [sid],
            |r| r.get(0),
        )
        .unwrap_or(0)
    } else {
        conn.query_row(
            "SELECT COUNT(*) FROM context_injections \
             WHERE project_id = ?1 AND was_deduped = 0",
            [project_id],
            |r| r.get(0),
        )
        .unwrap_or(0)
    }
}

/// Query subagent context hit rate.
/// Returns (loads_with_context, total_subagent_starts).
fn query_subagent_stats(
    conn: &Connection,
    session_id: Option<&str>,
    project_id: i64,
) -> (i64, i64) {
    if let Some(sid) = session_id {
        let loads = conn
            .query_row(
                "SELECT COUNT(*) FROM context_injections \
                 WHERE hook_name = 'SubagentStart' AND chars_injected > 0 AND session_id = ?1",
                [sid],
                |r| r.get::<_, i64>(0),
            )
            .unwrap_or(0);
        let total = conn
            .query_row(
                "SELECT COUNT(*) FROM context_injections \
                 WHERE hook_name = 'SubagentStart' AND session_id = ?1",
                [sid],
                |r| r.get::<_, i64>(0),
            )
            .unwrap_or(0);
        (loads, total)
    } else {
        let loads = conn
            .query_row(
                "SELECT COUNT(*) FROM context_injections \
                 WHERE hook_name = 'SubagentStart' AND chars_injected > 0 AND project_id = ?1",
                [project_id],
                |r| r.get::<_, i64>(0),
            )
            .unwrap_or(0);
        let total = conn
            .query_row(
                "SELECT COUNT(*) FROM context_injections \
                 WHERE hook_name = 'SubagentStart' AND project_id = ?1",
                [project_id],
                |r| r.get::<_, i64>(0),
            )
            .unwrap_or(0);
        (loads, total)
    }
}

/// Format seconds into a compact human-readable duration.
#[cfg(test)]
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

    let mira_label = format!("{DIM}Mira{RESET}");

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

    let (project_id, _) = match project {
        Some((id, name)) => (id, name),
        None => {
            // No project found — show minimal line
            println!("{mira_label} {DIM}no project{RESET}");
            return Ok(());
        }
    };

    // Get active session for session-scoped queries
    let session_id = query_active_session(&main_conn);
    let sid_ref = session_id.as_deref();

    // Query value metrics from main DB
    let assists = query_assists(&main_conn, sid_ref, project_id);
    let (subagent_loads, subagent_total) = query_subagent_stats(&main_conn, sid_ref, project_id);
    let goals = query_goals(&main_conn, project_id);
    let alerts = query_alerts(&main_conn, project_id);

    // Query stats from code DB
    let code_conn = open_readonly(&code_db);
    let indexed = code_conn
        .as_ref()
        .map(|c| query_indexed_files(c, project_id))
        .unwrap_or(0);
    let pending = code_conn
        .as_ref()
        .map(|c| query_pending(c, project_id))
        .unwrap_or(0);

    // Build output segments: assists, subagent ctx, goals, indexed, pending, alerts
    let mut parts = Vec::new();

    if assists > 0 {
        parts.push(format!("{GREEN}{assists}{RESET} assists"));
    }

    if subagent_total > 0 {
        let pct = (subagent_loads * 100 + subagent_total / 2) / subagent_total;
        parts.push(format!("{GREEN}{pct}%{RESET} subagent ctx"));
    }

    if goals > 0 {
        parts.push(format!("{GREEN}{goals}{RESET} goals"));
    }

    if indexed > 0 {
        parts.push(format!("{DIM}{indexed}{RESET} indexed"));
    }

    if pending > 0 {
        parts.push(format!("{YELLOW}{pending} pending{RESET}"));
    }

    if alerts > 0 {
        parts.push(format!("{YELLOW}{alerts} alerts{RESET}"));
    }

    if parts.is_empty() {
        println!("{mira_label}");
    } else {
        let joined = parts.join(DOT);
        println!("{mira_label}{DOT}{joined}");
    }

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
