// crates/mira-server/src/background/code_health.rs
// Background worker for detecting code health issues using concrete signals

use crate::db::Database;
use rusqlite::params;
use serde::Deserialize;
use std::collections::HashSet;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;

/// Check code health for all indexed projects
pub async fn process_code_health(db: &Arc<Database>) -> Result<usize, String> {
    tracing::debug!("Code health: checking for projects needing scan");
    let projects = get_projects_needing_health_check(db)?;
    if !projects.is_empty() {
        tracing::info!("Code health: found {} projects needing scan", projects.len());
    }
    let mut processed = 0;

    for (project_id, project_path) in projects {
        if !Path::new(&project_path).exists() {
            continue;
        }

        match scan_project_health(db, project_id, &project_path).await {
            Ok(count) => {
                tracing::info!(
                    "Found {} health issues for project {} ({})",
                    count,
                    project_id,
                    project_path
                );
                processed += count;
                mark_health_scanned(db, project_id)?;
            }
            Err(e) => {
                tracing::warn!("Failed to scan health for {}: {}", project_path, e);
            }
        }
    }

    Ok(processed)
}

/// Get projects that need health scanning (same rate limiting as capabilities)
fn get_projects_needing_health_check(db: &Database) -> Result<Vec<(i64, String)>, String> {
    // Get all indexed projects (in separate scope to release conn before calling needs_health_scan)
    let all_projects: Vec<(i64, String)> = {
        let conn = db.conn();
        let mut stmt = conn
            .prepare(
                "SELECT DISTINCT p.id, p.path
                 FROM projects p
                 JOIN codebase_modules m ON m.project_id = p.id",
            )
            .map_err(|e| e.to_string())?;

        stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect()
    }; // conn dropped here

    let mut needing_scan = Vec::new();

    for (project_id, project_path) in all_projects {
        if needs_health_scan(db, project_id)? {
            needing_scan.push((project_id, project_path));
            break; // One project per cycle
        }
    }

    Ok(needing_scan)
}

/// Check if project needs health scanning (> 1 day since last scan)
fn needs_health_scan(db: &Database, project_id: i64) -> Result<bool, String> {
    let conn = db.conn();

    let last_scan: Option<String> = conn
        .query_row(
            "SELECT updated_at FROM memory_facts
             WHERE project_id = ? AND key = 'health_scan_time'",
            [project_id],
            |row| row.get(0),
        )
        .ok();

    match last_scan {
        None => Ok(true), // Never scanned
        Some(scan_time) => {
            let older_than_1_day: bool = conn
                .query_row(
                    "SELECT datetime(?) < datetime('now', '-1 day')",
                    [&scan_time],
                    |row| row.get(0),
                )
                .unwrap_or(true);
            Ok(older_than_1_day)
        }
    }
}

/// Mark project as health-scanned
fn mark_health_scanned(db: &Database, project_id: i64) -> Result<(), String> {
    db.store_memory(
        Some(project_id),
        Some("health_scan_time"),
        "scanned",
        "system",
        Some("health"),
        1.0,
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Scan a project for health issues
async fn scan_project_health(
    db: &Arc<Database>,
    project_id: i64,
    project_path: &str,
) -> Result<usize, String> {
    tracing::info!("Code health: scanning project {}", project_path);

    // Clear old health issues
    clear_old_health_issues(db, project_id)?;

    let mut total = 0;

    // 1. Cargo check warnings (most important)
    tracing::debug!("Code health: running cargo check for {}", project_path);
    let warnings = scan_cargo_warnings(db, project_id, project_path)?;
    if warnings > 0 {
        tracing::info!("Code health: found {} cargo warnings", warnings);
    }
    total += warnings;

    // 2. TODO/FIXME comments
    let todos = scan_todo_comments(db, project_id, project_path)?;
    if todos > 0 {
        tracing::info!("Code health: found {} TODOs", todos);
    }
    total += todos;

    // 3. Unimplemented macros
    let unimpl = scan_unimplemented(db, project_id, project_path)?;
    if unimpl > 0 {
        tracing::info!("Code health: found {} unimplemented! macros", unimpl);
    }
    total += unimpl;

    // 4. Unused functions (from call graph)
    let unused = scan_unused_functions(db, project_id)?;
    if unused > 0 {
        tracing::info!("Code health: found {} potentially unused functions", unused);
    }
    total += unused;

    Ok(total)
}

/// Clear old health issues before refresh
fn clear_old_health_issues(db: &Database, project_id: i64) -> Result<(), String> {
    let conn = db.conn();
    conn.execute(
        "DELETE FROM memory_facts WHERE project_id = ? AND fact_type = 'health'",
        [project_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

// ═══════════════════════════════════════
// CARGO CHECK WARNINGS
// ═══════════════════════════════════════

/// Cargo message format
#[derive(Deserialize)]
struct CargoMessage {
    reason: String,
    message: Option<CompilerMessage>,
}

#[derive(Deserialize)]
struct CompilerMessage {
    level: String,
    message: String,
    spans: Vec<Span>,
    #[allow(dead_code)]
    rendered: Option<String>,
}

#[derive(Deserialize)]
struct Span {
    file_name: String,
    line_start: u32,
}

/// Run cargo check and parse warnings
fn scan_cargo_warnings(db: &Database, project_id: i64, project_path: &str) -> Result<usize, String> {
    // Check if it's a Rust project
    let cargo_toml = Path::new(project_path).join("Cargo.toml");
    if !cargo_toml.exists() {
        return Ok(0);
    }

    let output = Command::new("cargo")
        .args(["check", "--message-format=json", "--quiet"])
        .current_dir(project_path)
        .output()
        .map_err(|e| format!("Failed to run cargo check: {}", e))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut stored = 0;
    let mut seen_warnings = HashSet::new();

    for line in stdout.lines() {
        if let Ok(msg) = serde_json::from_str::<CargoMessage>(line) {
            if msg.reason == "compiler-message" {
                if let Some(compiler_msg) = msg.message {
                    if compiler_msg.level == "warning" {
                        // Get location from first span
                        let location = compiler_msg.spans.first().map(|s| {
                            format!("{}:{}", s.file_name, s.line_start)
                        }).unwrap_or_default();

                        // Deduplicate by location + message
                        let dedup_key = format!("{}:{}", location, compiler_msg.message);
                        if seen_warnings.contains(&dedup_key) {
                            continue;
                        }
                        seen_warnings.insert(dedup_key);

                        // Format the issue
                        let content = if location.is_empty() {
                            format!("[warning] {}", compiler_msg.message)
                        } else {
                            format!("[warning] {} at {}", compiler_msg.message, location)
                        };

                        let key = format!("health:warning:{}:{}", location, stored);
                        db.store_memory(
                            Some(project_id),
                            Some(&key),
                            &content,
                            "health",
                            Some("warning"),
                            0.9,
                        )
                        .map_err(|e| e.to_string())?;

                        stored += 1;
                    }
                }
            }
        }
    }

    Ok(stored)
}

// ═══════════════════════════════════════
// TODO/FIXME COMMENTS
// ═══════════════════════════════════════

/// Scan for TODO/FIXME/HACK comments
fn scan_todo_comments(db: &Database, project_id: i64, project_path: &str) -> Result<usize, String> {
    let output = Command::new("grep")
        .args([
            "-rn",
            "--include=*.rs",
            "-E",
            r"(TODO|FIXME|HACK|XXX)(\([^)]+\))?:",
            ".",
        ])
        .current_dir(project_path)
        .output()
        .map_err(|e| format!("Failed to run grep: {}", e))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut stored = 0;

    for line in stdout.lines() {
        // Format: ./path/file.rs:123:    // TODO: description
        if let Some((location, rest)) = line.split_once(':') {
            if let Some((line_num, comment)) = rest.split_once(':') {
                let file = location.trim_start_matches("./");
                let comment = comment.trim();

                // Extract the TODO type and message
                let content = format!("[todo] {}:{} - {}", file, line_num, comment);
                let key = format!("health:todo:{}:{}", file, line_num);

                db.store_memory(
                    Some(project_id),
                    Some(&key),
                    &content,
                    "health",
                    Some("todo"),
                    0.7, // Lower confidence - TODOs are informational
                )
                .map_err(|e| e.to_string())?;

                stored += 1;

                // Limit to prevent flooding
                if stored >= 50 {
                    break;
                }
            }
        }
    }

    Ok(stored)
}

// ═══════════════════════════════════════
// UNIMPLEMENTED MACROS
// ═══════════════════════════════════════

/// Scan for unimplemented!() and todo!() macros
fn scan_unimplemented(db: &Database, project_id: i64, project_path: &str) -> Result<usize, String> {
    let output = Command::new("grep")
        .args([
            "-rn",
            "--include=*.rs",
            "-E",
            r"(unimplemented!|todo!)\s*\(",
            ".",
        ])
        .current_dir(project_path)
        .output()
        .map_err(|e| format!("Failed to run grep: {}", e))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut stored = 0;

    for line in stdout.lines() {
        if let Some((location, rest)) = line.split_once(':') {
            if let Some((line_num, code)) = rest.split_once(':') {
                let file = location.trim_start_matches("./");
                let code = code.trim();

                let content = format!("[unimplemented] {}:{} - {}", file, line_num, code);
                let key = format!("health:unimplemented:{}:{}", file, line_num);

                db.store_memory(
                    Some(project_id),
                    Some(&key),
                    &content,
                    "health",
                    Some("unimplemented"),
                    0.8,
                )
                .map_err(|e| e.to_string())?;

                stored += 1;

                if stored >= 20 {
                    break;
                }
            }
        }
    }

    Ok(stored)
}

// ═══════════════════════════════════════
// UNUSED FUNCTIONS (from call graph)
// ═══════════════════════════════════════

/// Find functions that are never called (using indexed call graph)
/// Note: This is heuristic-based since the call graph doesn't capture self.method() calls
fn scan_unused_functions(db: &Database, project_id: i64) -> Result<usize, String> {
    // Query unused functions (release connection before storing)
    let unused: Vec<(String, String, i64)> = {
        let conn = db.conn();

        // Find functions that are defined but never appear as callees
        // The call graph doesn't capture self.method() calls, so we use heuristics:
        // - Exclude common method patterns (process_*, handle_*, get_*, etc.)
        // - Exclude trait implementations and common entry points
        // - Exclude test functions
        let mut stmt = conn
            .prepare(
                "SELECT s.name, s.file_path, s.start_line
                 FROM code_symbols s
                 WHERE s.project_id = ?
                   AND s.symbol_type = 'function'
                   -- Not called anywhere in the call graph
                   AND s.name NOT IN (SELECT DISTINCT callee_name FROM call_graph)
                   -- Exclude test functions
                   AND s.name NOT LIKE 'test_%'
                   AND s.name NOT LIKE '%_test'
                   AND s.name NOT LIKE '%_tests'
                   AND s.file_path NOT LIKE '%/tests/%'
                   AND s.file_path NOT LIKE '%_test.rs'
                   -- Exclude common entry points and trait methods
                   AND s.name NOT IN ('main', 'run', 'new', 'default', 'from', 'into', 'drop', 'clone', 'fmt', 'eq', 'hash', 'cmp', 'partial_cmp')
                   -- Exclude common method patterns (likely called via self.*)
                   AND s.name NOT LIKE 'process_%'
                   AND s.name NOT LIKE 'handle_%'
                   AND s.name NOT LIKE 'on_%'
                   AND s.name NOT LIKE 'do_%'
                   AND s.name NOT LIKE 'try_%'
                   AND s.name NOT LIKE 'get_%'
                   AND s.name NOT LIKE 'set_%'
                   AND s.name NOT LIKE 'is_%'
                   AND s.name NOT LIKE 'has_%'
                   AND s.name NOT LIKE 'with_%'
                   AND s.name NOT LIKE 'to_%'
                   AND s.name NOT LIKE 'as_%'
                   AND s.name NOT LIKE 'into_%'
                   AND s.name NOT LIKE 'from_%'
                   AND s.name NOT LIKE 'parse_%'
                   AND s.name NOT LIKE 'build_%'
                   AND s.name NOT LIKE 'create_%'
                   AND s.name NOT LIKE 'make_%'
                   AND s.name NOT LIKE 'init_%'
                   AND s.name NOT LIKE 'setup_%'
                   AND s.name NOT LIKE 'check_%'
                   AND s.name NOT LIKE 'validate_%'
                   AND s.name NOT LIKE 'clear_%'
                   AND s.name NOT LIKE 'reset_%'
                   AND s.name NOT LIKE 'update_%'
                   AND s.name NOT LIKE 'delete_%'
                   AND s.name NOT LIKE 'remove_%'
                   AND s.name NOT LIKE 'add_%'
                   AND s.name NOT LIKE 'insert_%'
                   AND s.name NOT LIKE 'find_%'
                   AND s.name NOT LIKE 'search_%'
                   AND s.name NOT LIKE 'load_%'
                   AND s.name NOT LIKE 'save_%'
                   AND s.name NOT LIKE 'store_%'
                   AND s.name NOT LIKE 'read_%'
                   AND s.name NOT LIKE 'write_%'
                   AND s.name NOT LIKE 'send_%'
                   AND s.name NOT LIKE 'receive_%'
                   AND s.name NOT LIKE 'start_%'
                   AND s.name NOT LIKE 'stop_%'
                   AND s.name NOT LIKE 'spawn_%'
                   AND s.name NOT LIKE 'run_%'
                   AND s.name NOT LIKE 'execute_%'
                   AND s.name NOT LIKE 'render_%'
                   AND s.name NOT LIKE 'format_%'
                   AND s.name NOT LIKE 'generate_%'
                   AND s.name NOT LIKE 'compute_%'
                   AND s.name NOT LIKE 'calculate_%'
                   AND s.name NOT LIKE 'mark_%'
                   AND s.name NOT LIKE 'scan_%'
                   AND s.name NOT LIKE 'index_%'
                   AND s.name NOT LIKE 'register_%'
                   AND s.name NOT LIKE 'unregister_%'
                   AND s.name NOT LIKE 'connect_%'
                   AND s.name NOT LIKE 'disconnect_%'
                   AND s.name NOT LIKE 'open_%'
                   AND s.name NOT LIKE 'close_%'
                   AND s.name NOT LIKE 'lock_%'
                   AND s.name NOT LIKE 'unlock_%'
                   AND s.name NOT LIKE 'acquire_%'
                   AND s.name NOT LIKE 'release_%'
                   -- Exclude private helpers (underscore prefix)
                   AND s.name NOT LIKE '_%'
                 LIMIT 20",
            )
            .map_err(|e| e.to_string())?;

        stmt.query_map(params![project_id], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect()
    }; // conn dropped here

    let mut stored = 0;

    for (name, file_path, line) in unused {
        let content = format!("[unused] Function `{}` at {}:{} appears to have no callers", name, file_path, line);
        let key = format!("health:unused:{}:{}", file_path, name);

        db.store_memory(
            Some(project_id),
            Some(&key),
            &content,
            "health",
            Some("unused"),
            0.5, // Low confidence - call graph doesn't capture self.method() calls
        )
        .map_err(|e| e.to_string())?;

        stored += 1;
    }

    Ok(stored)
}
