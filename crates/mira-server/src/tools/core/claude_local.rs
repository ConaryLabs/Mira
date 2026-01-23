// crates/mira-server/src/tools/core/claude_local.rs
// CLAUDE.local.md integration - bidirectional sync with Mira memories

use crate::db::{import_confirmed_memory_sync, pool::DatabasePool, search_memories_sync};
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
    let existing = search_memories_sync(conn, Some(project_id), "", None, 1000)
        .map_err(|e| e.to_string())?;
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

        // Store confirmed memory (high confidence since user explicitly wrote it)
        import_confirmed_memory_sync(
            conn,
            project_id,
            &key,
            &entry_content,
            fact_type,
            category.as_deref(),
            0.9,
        ).map_err(|e| e.to_string())?;

        imported += 1;
    }

    Ok(imported)
}

/// Export Mira memories to CLAUDE.local.md format (sync version for run_blocking)
/// Returns the markdown content
fn export_to_claude_local_md_sync(
    conn: &rusqlite::Connection,
    project_id: i64,
) -> Result<String, String> {
    // Get all high-confidence memories for this project
    let memories = search_memories_sync(conn, Some(project_id), "", None, 100)
        .map_err(|e| e.to_string())?;

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

    // ============================================================================
    // parse_claude_local_md tests
    // ============================================================================

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

    #[test]
    fn test_parse_asterisk_bullets() {
        let content = "## Patterns\n\n* Pattern one\n* Pattern two\n";
        let entries = parse_claude_local_md(content);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].0, "Pattern one");
        assert_eq!(entries[0].1, Some("pattern".to_string()));
    }

    #[test]
    fn test_parse_triple_hash_headers() {
        let content = "### Conventions\n\n- Follow naming conventions\n";
        let entries = parse_claude_local_md(content);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].1, Some("convention".to_string()));
    }

    #[test]
    fn test_parse_all_section_types() {
        let content = r#"
## User Preferences
- Pref item

## Architectural Decisions
- Decision item

## Code Patterns
- Pattern item

## Coding Conventions
- Convention item

## Common Mistakes
- Mistake item

## Workflows
- Workflow item

## Something Else
- General item
"#;
        let entries = parse_claude_local_md(content);
        assert_eq!(entries.len(), 7);

        assert_eq!(entries[0].1, Some("preference".to_string()));
        assert_eq!(entries[1].1, Some("decision".to_string()));
        assert_eq!(entries[2].1, Some("pattern".to_string()));
        assert_eq!(entries[3].1, Some("convention".to_string()));
        assert_eq!(entries[4].1, Some("mistake".to_string()));
        assert_eq!(entries[5].1, Some("workflow".to_string()));
        assert_eq!(entries[6].1, Some("general".to_string()));
    }

    #[test]
    fn test_parse_empty_bullets_skipped() {
        let content = "## General\n\n- Valid entry\n- \n- Another valid\n";
        let entries = parse_claude_local_md(content);
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn test_parse_whitespace_in_bullets() {
        let content = "## General\n\n-    Lots of leading spaces   \n";
        let entries = parse_claude_local_md(content);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0, "Lots of leading spaces");
    }

    #[test]
    fn test_parse_no_section() {
        let content = "- Item without section\n- Another item\n";
        let entries = parse_claude_local_md(content);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].1, None);
    }

    #[test]
    fn test_parse_decisions_section() {
        let content = "## Decisions\n\n- Use builder pattern\n";
        let entries = parse_claude_local_md(content);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].1, Some("decision".to_string()));
    }

    #[test]
    fn test_parse_avoid_section() {
        let content = "## Avoid\n\n- Don't use var\n";
        let entries = parse_claude_local_md(content);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].1, Some("mistake".to_string()));
    }

    #[test]
    fn test_parse_workflow_singular() {
        let content = "## Workflow\n\n- Run tests first\n";
        let entries = parse_claude_local_md(content);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].1, Some("workflow".to_string()));
    }
}
