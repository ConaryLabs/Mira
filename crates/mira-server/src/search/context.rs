// crates/mira-server/src/search/context.rs
// Context expansion for search results

use crate::db::Database;
use std::path::Path;

/// Parse symbol name and kind from chunk header
/// Headers look like: "// function foo", "// function foo: sig", "// function foo (continued)"
fn parse_symbol_header(chunk_content: &str) -> Option<(String, String)> {
    let first_line = chunk_content.lines().next()?;
    if !first_line.starts_with("// ") {
        return None;
    }

    let rest = first_line.strip_prefix("// ")?;

    // Skip "module-level code" - no symbol to look up
    if rest.starts_with("module") {
        return None;
    }

    // Split on first space to get kind
    let (kind, remainder) = rest.split_once(' ')?;

    // Get name: everything before ":" or " (continued)" or end of string
    let name = if let Some(idx) = remainder.find(':') {
        &remainder[..idx]
    } else if let Some(idx) = remainder.find(" (continued)") {
        &remainder[..idx]
    } else {
        remainder
    };

    Some((kind.to_string(), name.trim().to_string()))
}

/// Look up symbol bounds from code_symbols table
fn lookup_symbol_bounds(
    db: &Database,
    project_id: Option<i64>,
    file_path: &str,
    symbol_name: &str,
) -> Option<(u32, u32)> {
    let conn = db.conn();
    let query = if project_id.is_some() {
        "SELECT start_line, end_line FROM code_symbols
         WHERE project_id = ?1 AND file_path = ?2 AND name = ?3
         LIMIT 1"
    } else {
        "SELECT start_line, end_line FROM code_symbols
         WHERE file_path = ?1 AND name = ?2
         LIMIT 1"
    };

    let result: Option<(u32, u32)> = if let Some(pid) = project_id {
        conn.query_row(query, rusqlite::params![pid, file_path, symbol_name], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .ok()
    } else {
        conn.query_row(query, rusqlite::params![file_path, symbol_name], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .ok()
    };

    result
}

/// Expand search result to full symbol using code_symbols table
pub fn expand_context(
    file_path: &str,
    chunk_content: &str,
    project_path: Option<&str>,
) -> Option<(Option<String>, String)> {
    expand_context_with_db(file_path, chunk_content, project_path, None, None)
}

/// Expand search result to full symbol using code_symbols table (with DB access)
pub fn expand_context_with_db(
    file_path: &str,
    chunk_content: &str,
    project_path: Option<&str>,
    db: Option<&Database>,
    project_id: Option<i64>,
) -> Option<(Option<String>, String)> {
    // Extract symbol info from header
    let symbol_info = if chunk_content.starts_with("// ") {
        chunk_content.lines().next().map(|s| s.to_string())
    } else {
        None
    };

    // Try to expand using symbol bounds from DB
    if let (Some(db), Some(proj_path)) = (db, project_path) {
        if let Some((kind, name)) = parse_symbol_header(chunk_content) {
            if let Some((start_line, end_line)) = lookup_symbol_bounds(db, project_id, file_path, &name) {
                let full_path = Path::new(proj_path).join(file_path);
                if let Ok(file_content) = std::fs::read_to_string(&full_path) {
                    let all_lines: Vec<&str> = file_content.lines().collect();

                    // Convert 1-indexed lines to 0-indexed
                    let start = (start_line.saturating_sub(1)) as usize;
                    let end = std::cmp::min(end_line as usize, all_lines.len());

                    if start < all_lines.len() {
                        let full_symbol = all_lines[start..end].join("\n");
                        let header = format!("// {} {} (lines {}-{})", kind, name, start_line, end_line);
                        return Some((Some(header), full_symbol));
                    }
                }
            }
        }
    }

    // Fallback: use original +-5 line approach
    if let Some(proj_path) = project_path {
        let full_path = Path::new(proj_path).join(file_path);
        if let Ok(file_content) = std::fs::read_to_string(&full_path) {
            let search_content = if chunk_content.starts_with("// ") {
                chunk_content.lines().skip(1).collect::<Vec<_>>().join("\n")
            } else {
                chunk_content.to_string()
            };

            if let Some(pos) = file_content.find(&search_content) {
                let lines_before = file_content[..pos].matches('\n').count();
                let all_lines: Vec<&str> = file_content.lines().collect();
                let match_lines = search_content.matches('\n').count() + 1;

                let start_line = lines_before.saturating_sub(5);
                let end_line = std::cmp::min(lines_before + match_lines + 5, all_lines.len());

                let context_code = all_lines[start_line..end_line].join("\n");
                return Some((symbol_info, context_code));
            }
        }
    }

    // Final fallback: return chunk as-is
    Some((symbol_info, chunk_content.to_string()))
}
