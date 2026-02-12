// crates/mira-server/src/cli/statusline.rs
// Status line output for Claude Code — queries Mira databases for live stats.
// Designed to be fast (<50ms). All queries use indexed columns.

use anyhow::Result;
use mira::config::MiraConfig;
use mira::llm::Provider;
use rusqlite::Connection;
use std::io::BufRead;
use std::path::PathBuf;

/// ANSI escape codes
const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const MAGENTA: &str = "\x1b[35m";

/// Rainbow colors for "Mira"
const RAINBOW: [&str; 4] = [
    "\x1b[31m", // red
    "\x1b[33m", // yellow
    "\x1b[32m", // green
    "\x1b[36m", // cyan
];

const DOT: &str = " \x1b[2m\u{00b7}\x1b[0m ";

/// Build the rainbow-colored "Mira" string.
fn rainbow_mira() -> String {
    let chars = ['M', 'i', 'r', 'a'];
    let mut s = String::new();
    for (i, ch) in chars.iter().enumerate() {
        s.push_str(RAINBOW[i % RAINBOW.len()]);
        s.push(*ch);
    }
    s.push_str(RESET);
    s
}

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

/// Determine the active background LLM provider and its model name.
/// Reads from config.toml + checks env vars for API keys to verify the provider is usable.
fn get_llm_info() -> Option<(String, String)> {
    let config = MiraConfig::load();

    // Determine which provider would be used for background tasks
    let provider = config.background_provider().or_else(|| {
        config.default_provider().or_else(|| {
            std::env::var("DEFAULT_LLM_PROVIDER")
                .ok()
                .and_then(|s| Provider::from_str(&s))
        })
    });

    let provider = provider?;

    // Check if the provider actually has credentials configured
    let has_key = match provider {
        Provider::DeepSeek => std::env::var("DEEPSEEK_API_KEY").is_ok(),
        Provider::Zhipu => std::env::var("ZHIPU_API_KEY").is_ok(),
        Provider::Ollama => std::env::var("OLLAMA_HOST").is_ok(),
        Provider::Sampling => true, // always available when MCP is running
    };

    if !has_key {
        return None;
    }

    let model = match provider {
        Provider::Ollama => std::env::var("OLLAMA_MODEL")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| provider.default_model().to_string()),
        _ => provider.default_model().to_string(),
    };

    Some((provider.to_string(), model))
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

    let mira_label = rainbow_mira();

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
            println!("{mira_label} {DIM}no project{RESET}");
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

    // Build output: Mira project: Name · background: zhipu/glm-5 · 3 goals · indexed 2h ago · ...
    let mut parts = Vec::new();

    // 1. Stable context
    if !project_name.is_empty() {
        parts.push(format!("{DIM}project:{RESET} {project_name}"));
    }

    // 2. LLM provider info
    if let Some((provider, model)) = get_llm_info() {
        parts.push(format!("{DIM}background:{RESET} {DIM}{provider}/{model}{RESET}"));
    }

    // 3. Active workload
    if goals > 0 {
        parts.push(format!("{GREEN}{goals}{RESET} goals"));
    }

    // 4. Index age
    if let Some(age) = &index_age {
        parts.push(format!("{DIM}indexed {age}{RESET}"));
    }

    // 5. Actionable items
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
        parts.push(format!(
            "{YELLOW}{stale_docs} stale docs{RESET} {DIM}(/mira:insights){RESET}"
        ));
    }

    // 6. Alerts
    if bg_stalled {
        parts.push(format!("{YELLOW}background processing stopped{RESET}"));
    }

    // 7. Transient activity
    if pending > 0 {
        parts.push(format!("{YELLOW}indexing ({pending} pending){RESET}"));
    }

    let joined = parts.join(DOT);
    println!("{mira_label} {joined}");

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

    #[test]
    fn test_rainbow_mira_contains_all_chars() {
        let result = rainbow_mira();
        // Strip ANSI codes and check content
        let stripped: String = result
            .replace(RESET, "")
            .chars()
            .filter(|c| !c.is_ascii_control() && *c != '[')
            .collect();
        // After stripping ANSI, should contain M, i, r, a (and color codes like "31m" etc.)
        assert!(stripped.contains('M'));
        assert!(stripped.contains('i'));
        assert!(stripped.contains('r'));
        assert!(stripped.contains('a'));
    }
}
