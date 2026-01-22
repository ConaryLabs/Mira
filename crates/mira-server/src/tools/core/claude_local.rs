// crates/mira-server/src/tools/core/claude_local.rs
// CLAUDE.local.md integration - bidirectional sync with Mira memories

use crate::db::pool::DatabasePool;
use crate::tools::core::ToolContext;
use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

/// Export Mira memories to CLAUDE.local.md (MCP tool wrapper)
pub async fn export_claude_local<C: ToolContext>(ctx: &C) -> Result<String, String> {
    let project = ctx.get_project().await;
    let Some(project) = project else {
        return Err("No active project. Call session_start first.".to_string());
    };

    let project_id = project.id;
    let project_path = project.path.clone();
    let count = ctx
        .pool()
        .interact(move |conn| {
            write_claude_local_md_sync(conn, project_id, &project_path)
                .map_err(|e| anyhow::anyhow!(e))
        })
        .await
        .map_err(|e| e.to_string())?;

    if count == 0 {
        Ok("No memories to export (or all memories are low-confidence).".to_string())
    } else {
        Ok(format!(
            "Exported {} memories to {}/CLAUDE.local.md",
            count, project.path
        ))
    }
}

/// Async wrapper for importing CLAUDE.local.md entries
pub async fn import_claude_local_md_async(
    pool: &Arc<DatabasePool>,
    project_id: i64,
    project_path: &str,
) -> Result<usize, String> {
    let project_path = project_path.to_string();
    pool.interact(move |conn| {
        import_claude_local_md_sync(conn, project_id, &project_path)
            .map_err(|e| anyhow::anyhow!(e))
    })
    .await
    .map_err(|e| e.to_string())
}

/// Parse CLAUDE.local.md and extract memory entries
/// Returns Vec of (content, category) tuples
pub fn parse_claude_local_md(content: &str) -> Vec<(String, Option<String>)> {
    let mut entries = Vec::new();
    let mut current_section: Option<String> = None;

    for line in content.lines() {
        let trimmed = line.trim();

        // Track section headers (## or ###)
        if trimmed.starts_with("## ") || trimmed.starts_with("### ") {
            let header = trimmed
                .trim_start_matches('#')
                .trim()
                .to_lowercase()
                .replace(' ', "_");
            current_section = Some(header);
            continue;
        }

        // Extract bullet points as entries
        if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
            let entry = trimmed
                .trim_start_matches("- ")
                .trim_start_matches("* ")
                .trim();

            if !entry.is_empty() {
                // Map section names to Mira categories
                let category = current_section.as_ref().map(|s| {
                    match s.as_str() {
                        "preferences" | "user_preferences" => "preference",
                        "decisions" | "architectural_decisions" => "decision",
                        "patterns" | "code_patterns" => "pattern",
                        "conventions" | "coding_conventions" => "convention",
                        "mistakes" | "common_mistakes" | "avoid" => "mistake",
                        "workflows" | "workflow" => "workflow",
                        _ => "general",
                    }
                    .to_string()
                });

                entries.push((entry.to_string(), category));
            }
        }
    }

    entries
}

/// Import entries from CLAUDE.local.md into Mira memory (sync version for run_blocking)
/// Returns count of new entries imported (skips duplicates)
fn import_claude_local_md_sync(
    conn: &rusqlite::Connection,
    project_id: i64,
    project_path: &str,
) -> Result<usize, String> {
    use rusqlite::params;

    let claude_local_path = Path::new(project_path).join("CLAUDE.local.md");

    if !claude_local_path.exists() {
        return Ok(0);
    }

    let content = std::fs::read_to_string(&claude_local_path)
        .map_err(|e| format!("Failed to read CLAUDE.local.md: {}", e))?;

    let entries = parse_claude_local_md(&content);
    if entries.is_empty() {
        return Ok(0);
    }

    // Get existing memories to check for duplicates
    let existing = search_memories_sync(conn, Some(project_id), "", 1000)?;
    let existing_content: HashSet<_> = existing.iter().map(|m| m.content.as_str()).collect();

    let mut imported = 0;
    for (entry_content, category) in entries {
        // Skip if content already exists (fuzzy match - normalize whitespace)
        let normalized = entry_content.split_whitespace().collect::<Vec<_>>().join(" ");
        if existing_content.iter().any(|e| {
            let e_normalized = e.split_whitespace().collect::<Vec<_>>().join(" ");
            e_normalized == normalized
        }) {
            continue;
        }

        // Store as memory with source key for tracking
        let key = format!("claude_local:{}", &entry_content[..entry_content.len().min(50)]);
        let fact_type = match category.as_deref() {
            Some("preference") => "preference",
            Some("decision") => "decision",
            _ => "general",
        };

        // Store memory directly using connection
        let initial_confidence = 0.9; // High confidence since user explicitly wrote it
        conn.execute(
            "INSERT INTO memory_facts (project_id, key, content, fact_type, category, confidence,
             session_count, first_session_id, last_session_id, status)
             VALUES (?, ?, ?, ?, ?, ?, 1, NULL, NULL, 'confirmed')",
            params![Some(project_id), Some(&key), &entry_content, fact_type, category, initial_confidence],
        ).map_err(|e| e.to_string())?;

        imported += 1;
    }

    Ok(imported)
}

/// Sync helper: search memories by text (for use inside run_blocking)
fn search_memories_sync(
    conn: &rusqlite::Connection,
    project_id: Option<i64>,
    query: &str,
    limit: usize,
) -> Result<Vec<mira_types::MemoryFact>, String> {
    use rusqlite::params;

    let escaped = query
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    let pattern = format!("%{}%", escaped);

    let mut stmt = conn
        .prepare(
            "SELECT id, project_id, key, content, fact_type, category, confidence, created_at,
                    session_count, first_session_id, last_session_id, status
             FROM memory_facts
             WHERE (project_id = ? OR project_id IS NULL) AND content LIKE ? ESCAPE '\\'
             ORDER BY updated_at DESC
             LIMIT ?",
        )
        .map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map(params![project_id, pattern, limit as i64], |row| {
            Ok(mira_types::MemoryFact {
                id: row.get(0)?,
                project_id: row.get(1)?,
                key: row.get(2)?,
                content: row.get(3)?,
                fact_type: row.get(4)?,
                category: row.get(5)?,
                confidence: row.get(6)?,
                created_at: row.get(7)?,
                session_count: row.get(8).unwrap_or(1),
                first_session_id: row.get(9).ok(),
                last_session_id: row.get(10).ok(),
                status: row.get(11).unwrap_or_else(|_| "candidate".to_string()),
            })
        })
        .map_err(|e| e.to_string())?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

/// Export Mira memories to CLAUDE.local.md format (sync version for run_blocking)
/// Returns the markdown content
fn export_to_claude_local_md_sync(
    conn: &rusqlite::Connection,
    project_id: i64,
) -> Result<String, String> {
    // Get all high-confidence memories for this project
    let memories = search_memories_sync(conn, Some(project_id), "", 100)?;

    if memories.is_empty() {
        return Ok(String::new());
    }

    // Group by category/type
    let mut preferences = Vec::new();
    let mut decisions = Vec::new();
    let mut patterns = Vec::new();
    let mut general = Vec::new();

    for mem in &memories {
        // Skip low-confidence or system-generated
        if mem.confidence < 0.7 {
            continue;
        }

        match mem.fact_type.as_str() {
            "preference" => preferences.push(&mem.content),
            "decision" => decisions.push(&mem.content),
            "pattern" | "convention" => patterns.push(&mem.content),
            _ => {
                // Check category as fallback
                match mem.category.as_deref() {
                    Some("preference") => preferences.push(&mem.content),
                    Some("decision") => decisions.push(&mem.content),
                    Some("pattern") | Some("convention") => patterns.push(&mem.content),
                    _ => general.push(&mem.content),
                }
            }
        }
    }

    let mut output = String::from("# CLAUDE.local.md\n\n");
    output.push_str("<!-- Auto-generated from Mira memories. Manual edits will be imported back. -->\n\n");

    if !preferences.is_empty() {
        output.push_str("## Preferences\n\n");
        for p in &preferences {
            output.push_str(&format!("- {}\n", p));
        }
        output.push('\n');
    }

    if !decisions.is_empty() {
        output.push_str("## Decisions\n\n");
        for d in &decisions {
            output.push_str(&format!("- {}\n", d));
        }
        output.push('\n');
    }

    if !patterns.is_empty() {
        output.push_str("## Patterns\n\n");
        for p in &patterns {
            output.push_str(&format!("- {}\n", p));
        }
        output.push('\n');
    }

    if !general.is_empty() {
        output.push_str("## General\n\n");
        for g in &general {
            output.push_str(&format!("- {}\n", g));
        }
        output.push('\n');
    }

    Ok(output)
}

/// Write exported memories to CLAUDE.local.md file (sync version for run_blocking)
fn write_claude_local_md_sync(
    conn: &rusqlite::Connection,
    project_id: i64,
    project_path: &str,
) -> Result<usize, String> {
    let content = export_to_claude_local_md_sync(conn, project_id)?;
    if content.is_empty() {
        return Ok(0);
    }

    let claude_local_path = Path::new(project_path).join("CLAUDE.local.md");
    std::fs::write(&claude_local_path, &content)
        .map_err(|e| format!("Failed to write CLAUDE.local.md: {}", e))?;

    // Count entries (lines starting with "- ")
    let count = content.lines().filter(|l| l.starts_with("- ")).count();
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_claude_local_md() {
        let content = r#"# CLAUDE.local.md

## Preferences

- Use tabs for indentation
- Prefer async/await over callbacks

## Decisions

- Using SQLite for persistence
- Builder pattern for Config struct

## General

- Remember to run tests before committing
"#;

        let entries = parse_claude_local_md(content);
        assert_eq!(entries.len(), 5);

        assert_eq!(entries[0].0, "Use tabs for indentation");
        assert_eq!(entries[0].1, Some("preference".to_string()));

        assert_eq!(entries[2].0, "Using SQLite for persistence");
        assert_eq!(entries[2].1, Some("decision".to_string()));
    }

    #[test]
    fn test_parse_empty() {
        let entries = parse_claude_local_md("");
        assert!(entries.is_empty());
    }

    #[test]
    fn test_parse_no_bullets() {
        let content = "# Just a header\n\nSome text without bullets\n";
        let entries = parse_claude_local_md(content);
        assert!(entries.is_empty());
    }
}
