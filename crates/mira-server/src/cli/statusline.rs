// crates/mira-server/src/cli/statusline.rs
// Status line output for Claude Code — queries Mira databases for live stats.
// Designed to be fast (<50ms). All queries use indexed columns.

use anyhow::Result;
use std::path::PathBuf;

/// ANSI escape codes
const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";
const CYAN: &str = "\x1b[36m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";

/// Format a count for compact display: 1234 -> "1.2k"
fn fmt_count(n: i64) -> String {
    if n >= 1000 {
        format!("{}.{}k", n / 1000, (n % 1000) / 100)
    } else {
        n.to_string()
    }
}

/// Query stats from a database, returning 0s on any error.
fn query_main_stats(db_path: &PathBuf) -> (i64, i64) {
    let conn = match rusqlite::Connection::open_with_flags(
        db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) {
        Ok(c) => c,
        Err(_) => return (0, 0),
    };

    let memories: i64 = conn
        .query_row("SELECT COUNT(*) FROM memory_facts", [], |r| r.get(0))
        .unwrap_or(0);

    let goals: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM goals WHERE status NOT IN ('completed', 'abandoned')",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);

    (memories, goals)
}

fn query_code_stats(db_path: &PathBuf) -> (i64, i64) {
    let conn = match rusqlite::Connection::open_with_flags(
        db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) {
        Ok(c) => c,
        Err(_) => return (0, 0),
    };

    let symbols: i64 = conn
        .query_row("SELECT COUNT(*) FROM code_symbols", [], |r| r.get(0))
        .unwrap_or(0);

    let pending: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM pending_embeddings WHERE status = 'pending'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);

    (symbols, pending)
}

/// Print the status line to stdout.
pub fn run() -> Result<()> {
    // Consume stdin (Claude Code sends JSON context, we don't need it)
    let _ = std::io::read_to_string(std::io::stdin());

    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let mira_dir = home.join(".mira");
    let main_db = mira_dir.join("mira.db");
    let code_db = mira_dir.join("mira-code.db");

    if !main_db.exists() {
        return Ok(());
    }

    let (memories, goals) = query_main_stats(&main_db);
    let (symbols, pending) = if code_db.exists() {
        query_code_stats(&code_db)
    } else {
        (0, 0)
    };

    // Build output
    let mut parts = format!("{CYAN}{}{RESET} memories", fmt_count(memories));

    if goals > 0 {
        parts.push_str(&format!(" {DIM}\u{00b7}{RESET} {GREEN}{goals}{RESET} goals"));
    }

    if symbols > 0 {
        parts.push_str(&format!(
            " {DIM}\u{00b7}{RESET} {} symbols",
            fmt_count(symbols)
        ));
    }

    if pending > 0 {
        parts.push_str(&format!(
            " {DIM}\u{00b7}{RESET} {YELLOW}embedding {pending} chunks{RESET}"
        ));
    }

    println!("{DIM}Mira:{RESET} {parts}");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fmt_count_small() {
        assert_eq!(fmt_count(0), "0");
        assert_eq!(fmt_count(42), "42");
        assert_eq!(fmt_count(999), "999");
    }

    #[test]
    fn test_fmt_count_thousands() {
        assert_eq!(fmt_count(1000), "1.0k");
        assert_eq!(fmt_count(1234), "1.2k");
        assert_eq!(fmt_count(5700), "5.7k");
        assert_eq!(fmt_count(10500), "10.5k");
    }
}
