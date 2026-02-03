// crates/mira-server/src/tools/core/claude_local.rs
// CLAUDE.local.md integration - bidirectional sync with Mira memories

use crate::db::{
    RankedMemory, fetch_ranked_memories_for_export_sync, import_confirmed_memory_sync,
    pool::DatabasePool, search_memories_sync,
};
use crate::tools::core::ToolContext;
use crate::utils::ResultExt;
use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

/// Total byte budget for CLAUDE.local.md content (~2K tokens)
const CLAUDE_LOCAL_BYTE_BUDGET: usize = 8192;

/// Max bytes per individual memory entry (truncate verbose ones)
const MAX_MEMORY_BYTES: usize = 500;

/// Max ranked memories to fetch from DB (more than budget allows, gives room for packing)
const RANKED_FETCH_LIMIT: usize = 200;

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
        .run(move |conn| write_claude_local_md_sync(conn, project_id, &project_path))
        .await?;

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
    pool.run(move |conn| import_claude_local_md_sync(conn, project_id, &project_path))
        .await
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
    let existing =
        search_memories_sync(conn, Some(project_id), "", None, 1000).str_err()?;
    let existing_content: HashSet<_> = existing.iter().map(|m| m.content.as_str()).collect();

    let mut imported = 0;
    for (entry_content, category) in entries {
        // Skip if content already exists (fuzzy match - normalize whitespace)
        let normalized = entry_content
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if existing_content.iter().any(|e| {
            let e_normalized = e.split_whitespace().collect::<Vec<_>>().join(" ");
            e_normalized == normalized
        }) {
            continue;
        }

        // Store as memory with source key for tracking
        let key = format!(
            "claude_local:{}",
            &entry_content[..entry_content.len().min(50)]
        );
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
        )
        .str_err()?;

        imported += 1;
    }

    Ok(imported)
}

/// Export Mira memories to CLAUDE.local.md format using ranked memories
/// Returns the markdown content
fn export_to_claude_local_md_sync(
    conn: &rusqlite::Connection,
    project_id: i64,
) -> Result<String, String> {
    let memories =
        fetch_ranked_memories_for_export_sync(conn, project_id, RANKED_FETCH_LIMIT).str_err()?;

    if memories.is_empty() {
        return Ok(String::new());
    }

    Ok(build_budgeted_export(&memories))
}

/// Classify a memory into a section bucket based on fact_type and category
fn classify_memory(mem: &RankedMemory) -> &'static str {
    match mem.fact_type.as_str() {
        "preference" => "Preferences",
        "decision" => "Decisions",
        "pattern" | "convention" => "Patterns",
        _ => match mem.category.as_deref() {
            Some("preference") => "Preferences",
            Some("decision") => "Decisions",
            Some("pattern") | Some("convention") => "Patterns",
            _ => "General",
        },
    }
}

/// Truncate a string to max_bytes at a char boundary, appending "..." if truncated
fn truncate_content(content: &str, max_bytes: usize) -> String {
    if content.len() <= max_bytes {
        return content.to_string();
    }
    // Find the last char boundary at or before max_bytes - 3 (for "...")
    let truncate_at = max_bytes.saturating_sub(3);
    let mut end = truncate_at;
    while end > 0 && !content.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &content[..end])
}

/// Build budget-aware markdown export from ranked memories
///
/// Greedy knapsack by hotness: iterate ranked memories, add to section buckets
/// until byte budget is exhausted. Memories > MAX_MEMORY_BYTES are truncated.
/// Skip-don't-break: a long memory may not fit but a shorter one after it might
/// (max 10 consecutive skips before giving up).
pub fn build_budgeted_export(memories: &[RankedMemory]) -> String {
    let header = "# CLAUDE.local.md\n\n<!-- Auto-generated from Mira memories. Manual edits will be imported back. -->\n\n";
    let mut budget_remaining = CLAUDE_LOCAL_BYTE_BUDGET.saturating_sub(header.len());

    // Section ordering: Preferences > Decisions > Patterns > General
    let section_order = ["Preferences", "Decisions", "Patterns", "General"];

    // Collect entries per section, respecting budget
    let mut sections: Vec<(&str, Vec<String>)> = section_order
        .iter()
        .map(|&name| (name, Vec::new()))
        .collect();

    // Reserve bytes for section headers (## Name\n\n + trailing \n)
    // We'll account for headers only for sections that end up non-empty,
    // so track header costs separately
    let mut consecutive_skips = 0;

    for mem in memories {
        if consecutive_skips >= 10 {
            break;
        }

        let content = truncate_content(&mem.content, MAX_MEMORY_BYTES);
        let entry_line = format!("- {}\n", content);
        let entry_bytes = entry_line.len();

        let section_name = classify_memory(mem);

        // Find the section bucket
        let section = sections.iter_mut().find(|(name, _)| *name == section_name);
        let Some((_, entries)) = section else {
            continue;
        };

        // Calculate header cost if this is the first entry in the section
        let header_cost = if entries.is_empty() {
            format!("## {}\n\n", section_name).len() + 1 // +1 for trailing \n after section
        } else {
            0
        };

        let total_cost = entry_bytes + header_cost;

        if total_cost <= budget_remaining {
            budget_remaining -= total_cost;
            entries.push(entry_line);
            consecutive_skips = 0;
        } else if entry_bytes > budget_remaining {
            // This entry won't fit even without header cost — skip it
            consecutive_skips += 1;
        } else {
            // Header + entry doesn't fit, but maybe just entry doesn't fit either
            consecutive_skips += 1;
        }
    }

    // Render output in fixed section order
    let mut output = String::from(header);

    for (name, entries) in &sections {
        if entries.is_empty() {
            continue;
        }
        output.push_str(&format!("## {}\n\n", name));
        for entry in entries {
            output.push_str(entry);
        }
        output.push('\n');
    }

    // If no entries were added, return empty
    if sections.iter().all(|(_, entries)| entries.is_empty()) {
        return String::new();
    }

    output
}

/// Write exported memories to CLAUDE.local.md file (sync version for run_blocking)
/// Public so the stop hook can call it directly for auto-export.
pub fn write_claude_local_md_sync(
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

    // ============================================================================
    // build_budgeted_export tests
    // ============================================================================

    fn make_memory(
        content: &str,
        fact_type: &str,
        category: Option<&str>,
        hotness: f64,
    ) -> RankedMemory {
        RankedMemory {
            content: content.to_string(),
            fact_type: fact_type.to_string(),
            category: category.map(|s| s.to_string()),
            hotness,
        }
    }

    #[test]
    fn test_budgeted_export_empty_input() {
        let result = build_budgeted_export(&[]);
        assert!(result.is_empty());
    }

    #[test]
    fn test_budgeted_export_basic_sections() {
        let memories = vec![
            make_memory("Use tabs", "preference", Some("preference"), 10.0),
            make_memory("SQLite for storage", "decision", Some("decision"), 8.0),
            make_memory("Builder pattern", "pattern", Some("pattern"), 6.0),
            make_memory("Run tests often", "general", Some("general"), 4.0),
        ];
        let result = build_budgeted_export(&memories);

        assert!(result.contains("## Preferences"));
        assert!(result.contains("- Use tabs"));
        assert!(result.contains("## Decisions"));
        assert!(result.contains("- SQLite for storage"));
        assert!(result.contains("## Patterns"));
        assert!(result.contains("- Builder pattern"));
        assert!(result.contains("## General"));
        assert!(result.contains("- Run tests often"));
    }

    #[test]
    fn test_budgeted_export_hotness_ordering_within_section() {
        // Both are preferences, higher hotness should come first
        let memories = vec![
            make_memory("High priority pref", "preference", Some("preference"), 10.0),
            make_memory("Low priority pref", "preference", Some("preference"), 2.0),
        ];
        let result = build_budgeted_export(&memories);
        let high_pos = result.find("High priority pref").unwrap();
        let low_pos = result.find("Low priority pref").unwrap();
        assert!(high_pos < low_pos);
    }

    #[test]
    fn test_budgeted_export_truncates_long_memory() {
        let long_content = "x".repeat(600);
        let memories = vec![make_memory(&long_content, "general", None, 5.0)];
        let result = build_budgeted_export(&memories);

        // Each entry line is "- {content}\n", content should be truncated
        for line in result.lines() {
            if let Some(entry) = line.strip_prefix("- ") {
                assert!(entry.len() <= MAX_MEMORY_BYTES + 3); // +3 for "..."
                assert!(entry.ends_with("..."));
            }
        }
    }

    #[test]
    fn test_budgeted_export_respects_budget() {
        // Create many memories that would exceed the budget
        let memories: Vec<RankedMemory> = (0..300)
            .map(|i| {
                make_memory(
                    &format!(
                        "Memory number {} with some padding text to take up space",
                        i
                    ),
                    "general",
                    None,
                    300.0 - i as f64,
                )
            })
            .collect();

        let result = build_budgeted_export(&memories);
        assert!(result.len() <= CLAUDE_LOCAL_BYTE_BUDGET + 200); // small tolerance for final section newline
        // Should have some entries but not all 300
        let entry_count = result.lines().filter(|l| l.starts_with("- ")).count();
        assert!(entry_count > 0);
        assert!(entry_count < 300);
    }

    #[test]
    fn test_budgeted_export_single_massive_memory() {
        // A single memory larger than the entire budget
        let huge = "x".repeat(10000);
        let memories = vec![make_memory(&huge, "general", None, 100.0)];
        let result = build_budgeted_export(&memories);

        // Should still produce output — the memory gets truncated to MAX_MEMORY_BYTES
        assert!(result.contains("## General"));
        assert!(result.contains("..."));
    }

    #[test]
    fn test_budgeted_export_skip_dont_break() {
        // First memory is moderately large, second is small — both should fit
        // even if we need to skip some in between
        let memories = vec![
            make_memory(&"a".repeat(400), "preference", Some("preference"), 10.0),
            make_memory("small", "general", None, 5.0),
        ];
        let result = build_budgeted_export(&memories);

        assert!(result.contains("## Preferences"));
        assert!(result.contains("## General"));
        assert!(result.contains("- small"));
    }

    #[test]
    fn test_budgeted_export_category_fallback_classification() {
        // fact_type is "general" but category is "decision" — should go to Decisions
        let memories = vec![make_memory(
            "Chose REST over GraphQL",
            "general",
            Some("decision"),
            8.0,
        )];
        let result = build_budgeted_export(&memories);
        assert!(result.contains("## Decisions"));
        assert!(result.contains("- Chose REST over GraphQL"));
    }

    #[test]
    fn test_budgeted_export_convention_in_patterns() {
        let memories = vec![make_memory(
            "Use snake_case",
            "convention",
            Some("convention"),
            5.0,
        )];
        let result = build_budgeted_export(&memories);
        assert!(result.contains("## Patterns"));
    }

    #[test]
    fn test_truncate_content_short() {
        assert_eq!(truncate_content("hello", 500), "hello");
    }

    #[test]
    fn test_truncate_content_exact_boundary() {
        let s = "a".repeat(500);
        assert_eq!(truncate_content(&s, 500), s);
    }

    #[test]
    fn test_truncate_content_over() {
        let s = "a".repeat(600);
        let result = truncate_content(&s, 500);
        assert!(result.ends_with("..."));
        assert!(result.len() <= 500);
    }

    #[test]
    fn test_truncate_content_multibyte() {
        // Ensure truncation doesn't split a multi-byte char
        // 498 'a's + 4-byte emoji = 502 bytes, over the 500 limit
        let s = "a".repeat(498) + "\u{1F600}";
        assert!(s.len() > 500);
        let result = truncate_content(&s, 500);
        assert!(result.ends_with("..."));
        assert!(result.is_char_boundary(result.len()));
        assert!(result.len() <= 500);
    }
}
