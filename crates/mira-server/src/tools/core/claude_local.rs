// crates/mira-server/src/tools/core/claude_local.rs
// CLAUDE.local.md integration - bidirectional sync with Mira memories

use crate::db::{
    RankedMemory, fetch_ranked_memories_for_export_sync, import_confirmed_memory_sync,
    pool::DatabasePool, search_memories_sync,
};
use crate::tools::core::ToolContext;
use crate::utils::{ResultExt, truncate_at_boundary};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Total byte budget for CLAUDE.local.md content (~2K tokens)
const CLAUDE_LOCAL_BYTE_BUDGET: usize = 8192;

/// Max bytes per individual memory entry (truncate verbose ones)
const MAX_MEMORY_BYTES: usize = 500;

/// Max ranked memories to fetch from DB (more than budget allows, gives room for packing)
const RANKED_FETCH_LIMIT: usize = 200;

// ============================================================================
// Auto Memory Constants (stricter thresholds for hot cache)
// ============================================================================

/// Line budget for auto memory export (Claude Code truncates after 200 lines)
const AUTO_MEMORY_LINE_BUDGET: usize = 150;

/// Max bytes per memory entry in auto memory (shorter for line budget)
const AUTO_MEMORY_MAX_BYTES: usize = 400;

/// Category line quotas within AUTO_MEMORY_LINE_BUDGET
const AUTO_MEMORY_PREF_QUOTA: usize = 60; // 40%
const AUTO_MEMORY_DECISION_QUOTA: usize = 45; // 30%
const AUTO_MEMORY_PATTERN_QUOTA: usize = 30; // 20%
const AUTO_MEMORY_GENERAL_QUOTA: usize = 15; // 10%

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

    // Get existing memories and pre-normalize for O(1) duplicate checks
    let existing = search_memories_sync(conn, Some(project_id), "", None, 1000).str_err()?;
    let existing_normalized: HashSet<String> = existing
        .iter()
        .map(|m| m.content.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect();

    let mut imported = 0;
    for (entry_content, category) in entries {
        let normalized = entry_content
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if existing_normalized.contains(&normalized) {
            continue;
        }

        // Store as memory with source key for tracking
        let key = format!("claude_local:{}", truncate_at_boundary(&entry_content, 50));
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

// ============================================================================
// Auto Memory Integration (Claude Code's ~/.claude/projects/<path>/memory/)
// ============================================================================

/// Get the auto memory directory path for a project.
///
/// Claude Code uses: `~/.claude/projects/-home-peter-Mira/memory/`
/// The path is sanitized by replacing `/` with `-`.
pub fn get_auto_memory_dir(project_path: &str) -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    // Claude Code replaces / with - in path
    // /home/peter/Mira -> -home-peter-Mira
    let sanitized = project_path.replace('/', "-");
    home.join(".claude/projects")
        .join(&sanitized)
        .join("memory")
}

/// Check if auto memory feature is available (directory exists).
///
/// Non-invasive detection: only returns true if Claude Code has created the directory.
/// We never create it ourselves.
pub fn auto_memory_dir_exists(project_path: &str) -> bool {
    get_auto_memory_dir(project_path).exists()
}

/// Extended RankedMemory with additional fields for stricter filtering.
/// Some fields are fetched for potential debugging/logging but not directly read.
#[allow(dead_code)]
struct AutoMemoryCandidate {
    content: String,
    fact_type: String,
    category: Option<String>,
    hotness: f64,
    confidence: f64,
    session_count: i64,
    status: String,
}

/// Fetch memories with stricter thresholds for auto memory export.
///
/// Filters: confidence >= 0.8, session_count >= 3, status = 'confirmed'
fn fetch_auto_memory_candidates_sync(
    conn: &rusqlite::Connection,
    project_id: i64,
    limit: usize,
) -> Result<Vec<AutoMemoryCandidate>, String> {
    let sql = r#"
        SELECT content, fact_type, category,
            (
                session_count
                * confidence
                * CASE
                    WHEN category = 'preference' THEN 1.4
                    WHEN category = 'decision' THEN 1.3
                    WHEN category IN ('pattern', 'convention') THEN 1.1
                    WHEN category = 'context' THEN 1.0
                    ELSE 0.9
                  END
                / (1.0 + (CAST(julianday('now') - julianday(COALESCE(updated_at, created_at)) AS REAL) / 90.0))
            ) AS hotness,
            confidence,
            session_count,
            status
        FROM memory_facts
        WHERE project_id = ?1
          AND scope = 'project'
          AND confidence >= 0.8
          AND session_count >= 3
          AND status = 'confirmed'
          AND fact_type NOT IN ('health', 'persona')
        ORDER BY hotness DESC
        LIMIT ?2
    "#;

    let mut stmt = conn
        .prepare(sql)
        .map_err(|e| format!("Failed to prepare auto memory query: {}", e))?;

    let rows = stmt
        .query_map(rusqlite::params![project_id, limit as i64], |row| {
            Ok(AutoMemoryCandidate {
                content: row.get(0)?,
                fact_type: row.get(1)?,
                category: row.get(2)?,
                hotness: row.get(3)?,
                confidence: row.get(4)?,
                session_count: row.get(5)?,
                status: row.get(6)?,
            })
        })
        .map_err(|e| format!("Failed to execute auto memory query: {}", e))?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Failed to collect auto memory results: {}", e))
}

/// Build line-budgeted export for auto memory with category quotas.
fn build_auto_memory_export(candidates: &[AutoMemoryCandidate]) -> String {
    if candidates.is_empty() {
        return String::new();
    }

    let header = "# Mira Memory Cache\n\n<!-- Auto-generated from Mira. High-confidence memories only. -->\n\n";

    // Track line counts per category
    let mut pref_lines = 0usize;
    let mut decision_lines = 0usize;
    let mut pattern_lines = 0usize;
    let mut general_lines = 0usize;

    // Collect entries per section
    let mut preferences: Vec<String> = Vec::new();
    let mut decisions: Vec<String> = Vec::new();
    let mut patterns: Vec<String> = Vec::new();
    let mut general: Vec<String> = Vec::new();

    for mem in candidates {
        let content = truncate_content(&mem.content, AUTO_MEMORY_MAX_BYTES);
        let entry_line = format!("- {}\n", content);
        let line_count = entry_line.lines().count();

        // Classify and check quota
        let section = classify_auto_memory(mem);

        match section {
            "Preferences" if pref_lines + line_count <= AUTO_MEMORY_PREF_QUOTA => {
                pref_lines += line_count;
                preferences.push(entry_line);
            }
            "Decisions" if decision_lines + line_count <= AUTO_MEMORY_DECISION_QUOTA => {
                decision_lines += line_count;
                decisions.push(entry_line);
            }
            "Patterns" if pattern_lines + line_count <= AUTO_MEMORY_PATTERN_QUOTA => {
                pattern_lines += line_count;
                patterns.push(entry_line);
            }
            "General" if general_lines + line_count <= AUTO_MEMORY_GENERAL_QUOTA => {
                general_lines += line_count;
                general.push(entry_line);
            }
            _ => {
                // Quota exceeded for this category, skip
            }
        }

        // Check total line budget
        let total = pref_lines + decision_lines + pattern_lines + general_lines;
        if total >= AUTO_MEMORY_LINE_BUDGET {
            break;
        }
    }

    // Check if we have any content
    if preferences.is_empty() && decisions.is_empty() && patterns.is_empty() && general.is_empty() {
        return String::new();
    }

    // Build output in section order
    let mut output = String::from(header);

    if !preferences.is_empty() {
        output.push_str("## Preferences\n\n");
        for entry in &preferences {
            output.push_str(entry);
        }
        output.push('\n');
    }

    if !decisions.is_empty() {
        output.push_str("## Decisions\n\n");
        for entry in &decisions {
            output.push_str(entry);
        }
        output.push('\n');
    }

    if !patterns.is_empty() {
        output.push_str("## Patterns\n\n");
        for entry in &patterns {
            output.push_str(entry);
        }
        output.push('\n');
    }

    if !general.is_empty() {
        output.push_str("## General\n\n");
        for entry in &general {
            output.push_str(entry);
        }
        output.push('\n');
    }

    output
}

/// Classify an auto memory candidate into a section bucket
fn classify_auto_memory(mem: &AutoMemoryCandidate) -> &'static str {
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

/// Write high-confidence memories to Claude Code's auto memory directory.
///
/// Only writes if the directory exists (feature detection).
/// Writes to MEMORY.mira.md to avoid conflicts with user's MEMORY.md.
/// Uses atomic writes (temp file + rename).
///
/// Returns the number of memories exported, or 0 if:
/// - Auto memory directory doesn't exist
/// - No memories meet the stricter thresholds
pub fn write_auto_memory_sync(
    conn: &rusqlite::Connection,
    project_id: i64,
    project_path: &str,
) -> Result<usize, String> {
    let dir = get_auto_memory_dir(project_path);

    // Feature detection: only write if directory exists
    if !dir.exists() {
        return Ok(0);
    }

    // Fetch candidates with stricter thresholds
    let candidates = fetch_auto_memory_candidates_sync(conn, project_id, RANKED_FETCH_LIMIT)?;

    if candidates.is_empty() {
        return Ok(0);
    }

    let content = build_auto_memory_export(&candidates);
    if content.is_empty() {
        return Ok(0);
    }

    // Write to MEMORY.mira.md (Mira-owned), not MEMORY.md (user-owned)
    let memory_path = dir.join("MEMORY.mira.md");
    let temp_path = dir.join(".MEMORY.mira.md.tmp");

    // Atomic write: temp file + rename
    if let Err(e) = std::fs::write(&temp_path, &content) {
        // Clean up temp file on error
        let _ = std::fs::remove_file(&temp_path);
        return Err(format!("Failed to write temp file: {}", e));
    }

    if let Err(e) = std::fs::rename(&temp_path, &memory_path) {
        // Clean up temp file on rename failure
        let _ = std::fs::remove_file(&temp_path);
        return Err(format!("Failed to rename temp file: {}", e));
    }

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

    // ============================================================================
    // Auto Memory tests
    // ============================================================================

    #[test]
    fn test_get_auto_memory_dir() {
        // Test path calculation: slashes become dashes
        let dir = get_auto_memory_dir("/home/peter/Mira");
        let path_str = dir.to_string_lossy();

        // Should contain the sanitized path
        assert!(path_str.contains("-home-peter-Mira"));
        assert!(path_str.contains(".claude/projects"));
        assert!(path_str.ends_with("memory"));
    }

    #[test]
    fn test_get_auto_memory_dir_various_paths() {
        // Root path
        let dir = get_auto_memory_dir("/");
        assert!(dir.to_string_lossy().contains("/-/memory"));

        // Nested path
        let dir = get_auto_memory_dir("/usr/local/src/myproject");
        assert!(dir.to_string_lossy().contains("-usr-local-src-myproject"));

        // Path with trailing slash (shouldn't happen but handle gracefully)
        let dir = get_auto_memory_dir("/home/user/project/");
        assert!(dir.to_string_lossy().contains("-home-user-project-"));
    }

    #[test]
    fn test_auto_memory_dir_exists_nonexistent() {
        // A path that definitely doesn't have an auto memory dir
        assert!(!auto_memory_dir_exists("/nonexistent/path/that/does/not/exist"));
    }

    fn make_auto_memory_candidate(
        content: &str,
        fact_type: &str,
        category: Option<&str>,
        hotness: f64,
    ) -> AutoMemoryCandidate {
        AutoMemoryCandidate {
            content: content.to_string(),
            fact_type: fact_type.to_string(),
            category: category.map(|s| s.to_string()),
            hotness,
            confidence: 0.9,
            session_count: 5,
            status: "confirmed".to_string(),
        }
    }

    #[test]
    fn test_build_auto_memory_export_empty() {
        let result = build_auto_memory_export(&[]);
        assert!(result.is_empty());
    }

    #[test]
    fn test_build_auto_memory_export_basic_sections() {
        let candidates = vec![
            make_auto_memory_candidate("User prefers tabs", "preference", Some("preference"), 10.0),
            make_auto_memory_candidate("Using SQLite", "decision", Some("decision"), 8.0),
            make_auto_memory_candidate("Builder pattern", "pattern", Some("pattern"), 6.0),
            make_auto_memory_candidate("General fact", "general", Some("general"), 4.0),
        ];
        let result = build_auto_memory_export(&candidates);

        assert!(result.contains("# Mira Memory Cache"));
        assert!(result.contains("## Preferences"));
        assert!(result.contains("- User prefers tabs"));
        assert!(result.contains("## Decisions"));
        assert!(result.contains("- Using SQLite"));
        assert!(result.contains("## Patterns"));
        assert!(result.contains("- Builder pattern"));
        assert!(result.contains("## General"));
        assert!(result.contains("- General fact"));
    }

    #[test]
    fn test_build_auto_memory_line_budget() {
        // Create many candidates that would exceed the line budget
        let candidates: Vec<AutoMemoryCandidate> = (0..200)
            .map(|i| {
                make_auto_memory_candidate(
                    &format!("Memory item number {} with some content", i),
                    "general",
                    Some("general"),
                    200.0 - i as f64,
                )
            })
            .collect();

        let result = build_auto_memory_export(&candidates);

        // Count total lines (excluding empty lines and headers)
        let entry_count = result.lines().filter(|l| l.starts_with("- ")).count();

        // Should be limited by the general quota (15 lines)
        assert!(entry_count <= AUTO_MEMORY_GENERAL_QUOTA);
    }

    #[test]
    fn test_build_auto_memory_category_quotas() {
        // Create many preferences to test quota enforcement
        let mut candidates: Vec<AutoMemoryCandidate> = (0..100)
            .map(|i| {
                make_auto_memory_candidate(
                    &format!("Preference {}", i),
                    "preference",
                    Some("preference"),
                    100.0 - i as f64,
                )
            })
            .collect();

        // Add some decisions too
        candidates.extend((0..50).map(|i| {
            make_auto_memory_candidate(
                &format!("Decision {}", i),
                "decision",
                Some("decision"),
                50.0 - i as f64,
            )
        }));

        let result = build_auto_memory_export(&candidates);

        // Count entries per section
        let pref_count = result
            .lines()
            .filter(|l| l.starts_with("- Preference"))
            .count();
        let decision_count = result
            .lines()
            .filter(|l| l.starts_with("- Decision"))
            .count();

        // Should be limited by quotas
        assert!(pref_count <= AUTO_MEMORY_PREF_QUOTA);
        assert!(decision_count <= AUTO_MEMORY_DECISION_QUOTA);
    }

    #[test]
    fn test_classify_auto_memory() {
        // Test fact_type classification
        let pref = make_auto_memory_candidate("test", "preference", None, 1.0);
        assert_eq!(classify_auto_memory(&pref), "Preferences");

        let decision = make_auto_memory_candidate("test", "decision", None, 1.0);
        assert_eq!(classify_auto_memory(&decision), "Decisions");

        let pattern = make_auto_memory_candidate("test", "pattern", None, 1.0);
        assert_eq!(classify_auto_memory(&pattern), "Patterns");

        let convention = make_auto_memory_candidate("test", "convention", None, 1.0);
        assert_eq!(classify_auto_memory(&convention), "Patterns");

        // Test category fallback
        let cat_pref = make_auto_memory_candidate("test", "general", Some("preference"), 1.0);
        assert_eq!(classify_auto_memory(&cat_pref), "Preferences");

        let cat_decision = make_auto_memory_candidate("test", "general", Some("decision"), 1.0);
        assert_eq!(classify_auto_memory(&cat_decision), "Decisions");

        // Default to General
        let general = make_auto_memory_candidate("test", "general", Some("other"), 1.0);
        assert_eq!(classify_auto_memory(&general), "General");
    }

    #[test]
    fn test_auto_memory_truncates_long_content() {
        let long_content = "x".repeat(600);
        let candidates = vec![make_auto_memory_candidate(
            &long_content,
            "general",
            None,
            5.0,
        )];
        let result = build_auto_memory_export(&candidates);

        // Find the entry line and check it's truncated
        for line in result.lines() {
            if let Some(entry) = line.strip_prefix("- ") {
                assert!(entry.len() <= AUTO_MEMORY_MAX_BYTES + 3); // +3 for "..."
                assert!(entry.ends_with("..."));
            }
        }
    }
}
