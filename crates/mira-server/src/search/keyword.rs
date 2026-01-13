// crates/mira-server/src/search/keyword.rs
// Keyword-based code search fallback

use rusqlite::{params, Connection};
use std::path::Path;

/// Result from keyword search: (file_path, content, score)
pub type KeywordResult = (String, String, f32);

/// Keyword-based code search
/// Searches chunk content and symbol names using LIKE matching
pub fn keyword_search(
    conn: &Connection,
    query: &str,
    project_id: Option<i64>,
    project_path: Option<&str>,
    limit: usize,
) -> Vec<KeywordResult> {
    let mut results = Vec::new();

    // Split query into terms for flexible matching
    let terms: Vec<&str> = query.split_whitespace().collect();
    if terms.is_empty() {
        return results;
    }

    // Build LIKE patterns - match any term
    let like_patterns: Vec<String> = terms
        .iter()
        .map(|t| format!("%{}%", t.to_lowercase()))
        .collect();

    // Search vec_code chunk_content
    if let Some(pid) = project_id {
        for pattern in &like_patterns {
            let sql = "SELECT file_path, chunk_content FROM vec_code
                       WHERE project_id = ? AND LOWER(chunk_content) LIKE ?
                       LIMIT ?";
            if let Ok(mut stmt) = conn.prepare(sql) {
                if let Ok(rows) = stmt.query_map(params![pid, pattern, limit as i64], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                }) {
                    for row in rows.flatten() {
                        // Avoid duplicates
                        if !results.iter().any(|(f, c, _)| f == &row.0 && c == &row.1) {
                            results.push((row.0, row.1, 0.5)); // Fixed score for chunk matches
                        }
                    }
                }
            }
            if results.len() >= limit {
                break;
            }
        }
    }

    // Also search symbol names for direct matches
    if let Some(pid) = project_id {
        for pattern in &like_patterns {
            let sql = "SELECT file_path, name, signature, start_line, end_line
                       FROM code_symbols
                       WHERE project_id = ? AND LOWER(name) LIKE ?
                       LIMIT ?";
            if let Ok(mut stmt) = conn.prepare(sql) {
                if let Ok(rows) = stmt.query_map(params![pid, pattern, limit as i64], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, Option<i64>>(3)?,
                        row.get::<_, Option<i64>>(4)?,
                    ))
                }) {
                    for row in rows.flatten() {
                        let (file_path, name, signature, start_line, end_line) = row;

                        // Try to read the actual code from file
                        let content = if let (Some(proj_path), Some(start), Some(end)) =
                            (project_path, start_line, end_line)
                        {
                            let full_path = Path::new(proj_path).join(&file_path);
                            if let Ok(file_content) = std::fs::read_to_string(&full_path) {
                                let lines: Vec<&str> = file_content.lines().collect();
                                let start_idx = (start as usize).saturating_sub(1).min(lines.len());
                                let end_idx = (end as usize).min(lines.len());
                                if start_idx < end_idx {
                                    lines[start_idx..end_idx].join("\n")
                                } else {
                                    signature.clone().unwrap_or_else(|| name.clone())
                                }
                            } else {
                                signature.unwrap_or_else(|| name.clone())
                            }
                        } else {
                            signature.unwrap_or_else(|| name.clone())
                        };

                        // Avoid duplicates
                        if !results
                            .iter()
                            .any(|(f, _, _)| f == &file_path && content.contains(&name))
                        {
                            results.push((file_path, content, 0.6)); // Higher score for symbol matches
                        }
                    }
                }
            }
            if results.len() >= limit {
                break;
            }
        }
    }

    results.truncate(limit);
    results
}
