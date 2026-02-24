// crates/mira-server/src/tools/core/claude_local/export.rs
// CLAUDE.local.md export: budget-aware, deduplicated markdown generation

use crate::db::{fetch_ranked_memories_for_export_sync, RankedMemory};
use crate::error::MiraError;
use crate::tools::core::ToolContext;
use std::path::Path;

/// Total byte budget for CLAUDE.local.md content (~2K tokens)
const CLAUDE_LOCAL_BYTE_BUDGET: usize = 8192;

/// Hard cap on total output characters (applied after rendering)
const TOTAL_CHAR_CAP: usize = 4000;

/// Hard cap on total output lines (applied during packing)
const MAX_OUTPUT_LINES: usize = 150;

/// Per-type truncation limits (bytes)
const MAX_DECISION_BYTES: usize = 200;
const MAX_PREFERENCE_BYTES: usize = 150;
const MAX_GENERAL_BYTES: usize = 150;
const MAX_PATTERN_BYTES: usize = 150;

/// Get per-section truncation limit
fn max_bytes_for_section(section: &str) -> usize {
    match section {
        "Decisions" => MAX_DECISION_BYTES,
        "Preferences" => MAX_PREFERENCE_BYTES,
        "Patterns" => MAX_PATTERN_BYTES,
        _ => MAX_GENERAL_BYTES,
    }
}

/// Compute character-level similarity ratio of first 100 chars of two strings.
/// Returns a value in [0.0, 1.0] where 1.0 means identical prefixes.
fn prefix_similarity(a: &str, b: &str) -> f64 {
    let a_chars: Vec<char> = a.chars().take(100).collect();
    let b_chars: Vec<char> = b.chars().take(100).collect();
    let max_len = a_chars.len().max(b_chars.len());
    if max_len == 0 {
        return 1.0;
    }
    let matching = a_chars
        .iter()
        .zip(b_chars.iter())
        .filter(|(a, b)| a == b)
        .count();
    matching as f64 / max_len as f64
}

/// Export Mira memories to CLAUDE.local.md (MCP tool wrapper)
pub async fn export_claude_local<C: ToolContext>(ctx: &C) -> Result<String, MiraError> {
    let project = ctx.get_project().await;
    let Some(project) = project else {
        return Err(MiraError::ProjectNotSet);
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

/// Export Mira memories to CLAUDE.local.md format using ranked memories
/// Returns the markdown content
fn export_to_claude_local_md_sync(
    conn: &rusqlite::Connection,
    project_id: i64,
) -> Result<String, MiraError> {
    let memories =
        fetch_ranked_memories_for_export_sync(conn, project_id, super::RANKED_FETCH_LIMIT)?;

    if memories.is_empty() {
        return Ok(String::new());
    }

    Ok(build_budgeted_export(&memories))
}

/// Build budget-aware markdown export from ranked memories
///
/// Greedy knapsack by hotness: iterate ranked memories, add to section buckets
/// until byte budget is exhausted. Per-type truncation limits apply.
/// Cross-section deduplication: memories with >80% prefix similarity (first 100
/// chars) are deduplicated globally, keeping the higher-hotness entry.
/// Hard caps: 150 lines max, 4000 chars total after rendering.
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

    let mut consecutive_skips = 0;
    // Global dedup: store first-100-char prefixes of accepted entries
    let mut seen_prefixes: Vec<String> = Vec::new();
    // Track total line count (header lines + entry lines)
    let header_line_count = header.lines().count();
    let mut total_lines = header_line_count;

    for mem in memories {
        if consecutive_skips >= 10 {
            break;
        }

        // Line cap check
        if total_lines >= MAX_OUTPUT_LINES {
            break;
        }

        let section_name =
            super::classify_by_type_and_category(&mem.fact_type, mem.category.as_deref());

        // Per-type truncation limit
        let max_bytes = max_bytes_for_section(section_name);
        let content = super::truncate_content(&mem.content, max_bytes);

        // Cross-section deduplication: reject if >80% similar to any accepted entry
        let candidate_prefix: String = content.chars().take(100).collect();
        let is_duplicate = seen_prefixes
            .iter()
            .any(|existing| prefix_similarity(existing, &candidate_prefix) > 0.80);
        if is_duplicate {
            continue;
        }

        // Normalize newlines: collapse multiline content into a single line to prevent
        // a single memory from blowing the line budget.
        let content_oneline = content.replace('\n', " ");
        let entry_line = format!("- {}\n", content_oneline);
        let entry_bytes = entry_line.len();

        // Find the section bucket
        let section = sections.iter_mut().find(|(name, _)| *name == section_name);
        let Some((_, entries)) = section else {
            continue;
        };

        // Calculate header cost if this is the first entry in the section
        let is_new_section = entries.is_empty();
        let header_cost = if is_new_section {
            format!("## {}\n\n", section_name).len() + 1 // +1 for trailing \n after section
        } else {
            0
        };
        // Lines added: section header ("## X\n" + blank line) + trailing blank = 3; entry = 1
        // (entry is always 1 line after newline normalization above)
        let lines_cost = if is_new_section { 3 } else { 0 } + 1;

        let total_cost = entry_bytes + header_cost;

        if total_cost <= budget_remaining && (total_lines + lines_cost) <= MAX_OUTPUT_LINES {
            budget_remaining -= total_cost;
            entries.push(entry_line);
            total_lines += lines_cost;
            // Mark seen ONLY after successful budget add
            seen_prefixes.push(candidate_prefix);
            consecutive_skips = 0;
        } else if entry_bytes > budget_remaining {
            consecutive_skips += 1;
        } else {
            consecutive_skips += 1;
        }
    }

    let rendered = super::render_sections(header, &sections);

    // Hard character cap: if output exceeds TOTAL_CHAR_CAP, truncate from bottom
    enforce_char_cap(&rendered, TOTAL_CHAR_CAP)
}

/// Enforce a character cap on rendered markdown by removing trailing entries.
/// Keeps the header and as many complete lines as fit within the cap.
fn enforce_char_cap(rendered: &str, cap: usize) -> String {
    if rendered.len() <= cap {
        return rendered.to_string();
    }

    // Walk lines, accumulating until we would exceed the cap
    let mut output = String::with_capacity(cap);
    for line in rendered.lines() {
        // +1 for the newline we'll add back
        if output.len() + line.len() + 1 > cap {
            break;
        }
        output.push_str(line);
        output.push('\n');
    }

    // Ensure trailing newline
    if !output.ends_with('\n') {
        output.push('\n');
    }
    output
}

/// Write exported memories to CLAUDE.local.md file (sync version for run_blocking)
/// Public so the stop hook can call it directly for auto-export.
pub fn write_claude_local_md_sync(
    conn: &rusqlite::Connection,
    project_id: i64,
    project_path: &str,
) -> Result<usize, MiraError> {
    let content = export_to_claude_local_md_sync(conn, project_id)?;
    if content.is_empty() {
        return Ok(0);
    }

    let claude_local_path = Path::new(project_path).join("CLAUDE.local.md");
    let temp_path = Path::new(project_path).join("CLAUDE.local.md.tmp");

    // Atomic write: temp file + rename
    if let Err(e) = std::fs::write(&temp_path, &content) {
        let _ = std::fs::remove_file(&temp_path);
        return Err(MiraError::Io(e));
    }

    if let Err(e) = std::fs::rename(&temp_path, &claude_local_path) {
        let _ = std::fs::remove_file(&temp_path);
        return Err(MiraError::Io(e));
    }

    // Count entries (lines starting with "- ")
    let count = content.lines().filter(|l| l.starts_with("- ")).count();
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        // to MAX_GENERAL_BYTES (150)
        for line in result.lines() {
            if let Some(entry) = line.strip_prefix("- ") {
                assert!(entry.len() <= MAX_GENERAL_BYTES + 3); // +3 for "..."
                assert!(entry.ends_with("..."));
            }
        }
    }

    #[test]
    fn test_budgeted_export_per_type_truncation() {
        // Decision should truncate at 200, preference at 150, general at 150.
        // Use distinct content per type to avoid cross-section dedup.
        let pref_content = "P".repeat(300);
        let dec_content = "D".repeat(300);
        let gen_content = "G".repeat(300);
        let memories = vec![
            make_memory(&pref_content, "preference", Some("preference"), 10.0),
            make_memory(&dec_content, "decision", Some("decision"), 8.0),
            make_memory(&gen_content, "general", Some("general"), 6.0),
        ];
        let result = build_budgeted_export(&memories);

        let mut pref_len = 0;
        let mut dec_len = 0;
        let mut gen_len = 0;
        let mut in_section = "";
        for line in result.lines() {
            if line.starts_with("## Preferences") {
                in_section = "pref";
            } else if line.starts_with("## Decisions") {
                in_section = "dec";
            } else if line.starts_with("## General") {
                in_section = "gen";
            } else if let Some(entry) = line.strip_prefix("- ") {
                match in_section {
                    "pref" => pref_len = entry.len(),
                    "dec" => dec_len = entry.len(),
                    "gen" => gen_len = entry.len(),
                    _ => {}
                }
            }
        }

        assert!(
            pref_len <= MAX_PREFERENCE_BYTES + 3,
            "Preference entry too long: {}",
            pref_len
        );
        assert!(
            dec_len <= MAX_DECISION_BYTES + 3,
            "Decision entry too long: {}",
            dec_len
        );
        assert!(
            gen_len <= MAX_GENERAL_BYTES + 3,
            "General entry too long: {}",
            gen_len
        );
        // Decision limit (200) is higher than preference limit (150)
        assert!(
            dec_len > pref_len,
            "Decision ({}) should be longer than preference ({})",
            dec_len,
            pref_len
        );
    }

    #[test]
    fn test_budgeted_export_respects_budget() {
        // Create many memories that would exceed the budget
        let memories: Vec<RankedMemory> = (0..300)
            .map(|i| {
                make_memory(
                    &format!("Memory number {} with some padding text to take up space", i),
                    "general",
                    None,
                    300.0 - i as f64,
                )
            })
            .collect();

        let result = build_budgeted_export(&memories);
        // Total char cap is 4000
        assert!(
            result.len() <= TOTAL_CHAR_CAP,
            "Output {} exceeds char cap {}",
            result.len(),
            TOTAL_CHAR_CAP
        );
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

        // Should still produce output -- the memory gets truncated per-type
        assert!(result.contains("## General"));
        assert!(result.contains("..."));
    }

    #[test]
    fn test_budgeted_export_skip_dont_break() {
        // First memory is moderately large (gets truncated to 150), second is small
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
        // fact_type is "general" but category is "decision" -- should go to Decisions
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
    fn test_budgeted_export_deduplicates_identical_content() {
        let memories = vec![
            make_memory("Same content here", "general", None, 10.0),
            make_memory("Same content here", "general", None, 8.0),
            make_memory("Different content", "general", None, 6.0),
        ];
        let result = build_budgeted_export(&memories);
        // "Same content here" should appear exactly once
        let count = result.matches("Same content here").count();
        assert_eq!(count, 1, "Duplicate content should be deduplicated");
        assert!(result.contains("Different content"));
    }

    #[test]
    fn test_budgeted_export_deduplicates_truncated_variants() {
        // Two memories with same prefix but different suffixes beyond truncation limit
        // After truncation, they share the same first 100 chars
        let base = "A".repeat(100);
        let mem1 = format!("{}BBB", base);
        let mem2 = format!("{}CCC", base);
        let memories = vec![
            make_memory(&mem1, "decision", Some("decision"), 10.0),
            make_memory(&mem2, "decision", Some("decision"), 8.0),
        ];
        let result = build_budgeted_export(&memories);
        // Only the higher-hotness one should appear (mem1 with BBB)
        assert!(result.contains("BBB"));
        assert!(!result.contains("CCC"));
    }

    #[test]
    fn test_budgeted_export_dedup_is_cross_section() {
        // Two memories with identical first 100 chars in different sections
        // should be deduplicated globally -- only the higher-hotness one survives
        let content = "A".repeat(100);
        let memories = vec![
            make_memory(&content, "preference", Some("preference"), 10.0),
            make_memory(&content, "decision", Some("decision"), 8.0),
        ];
        let result = build_budgeted_export(&memories);
        // Only the preference entry (higher hotness) should appear
        let entry_count = result.lines().filter(|l| l.starts_with("- ")).count();
        assert_eq!(
            entry_count, 1,
            "Cross-section duplicates should be deduplicated"
        );
        assert!(
            result.contains("## Preferences"),
            "Higher-hotness entry should be kept"
        );
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
    fn test_line_cap_enforced() {
        // Create enough memories to exceed 150 lines
        let memories: Vec<RankedMemory> = (0..200)
            .map(|i| make_memory(&format!("Unique memory {}", i), "general", None, 200.0 - i as f64))
            .collect();

        let result = build_budgeted_export(&memories);
        let line_count = result.lines().count();
        assert!(
            line_count <= MAX_OUTPUT_LINES,
            "Output has {} lines, exceeds cap {}",
            line_count,
            MAX_OUTPUT_LINES
        );
    }

    #[test]
    fn test_char_cap_enforced() {
        // Create enough content to exceed 4000 chars
        let memories: Vec<RankedMemory> = (0..100)
            .map(|i| {
                make_memory(
                    &format!("Memory {} with enough text to consume chars quickly", i),
                    "general",
                    None,
                    100.0 - i as f64,
                )
            })
            .collect();

        let result = build_budgeted_export(&memories);
        assert!(
            result.len() <= TOTAL_CHAR_CAP,
            "Output {} chars exceeds cap {}",
            result.len(),
            TOTAL_CHAR_CAP
        );
    }

    #[test]
    fn test_prefix_similarity_identical() {
        assert!((prefix_similarity("hello world", "hello world") - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_prefix_similarity_completely_different() {
        assert!(prefix_similarity("aaaa", "bbbb") < 0.01);
    }

    #[test]
    fn test_prefix_similarity_partial_match() {
        // 8 out of 10 chars match
        let sim = prefix_similarity("abcdefghXX", "abcdefghYY");
        assert!(sim > 0.79 && sim < 0.81, "Expected ~0.8, got {}", sim);
    }

    #[test]
    fn test_prefix_similarity_empty() {
        assert!((prefix_similarity("", "") - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_enforce_char_cap_short_input() {
        let input = "short\n";
        assert_eq!(enforce_char_cap(input, 4000), input);
    }

    #[test]
    fn test_enforce_char_cap_truncates() {
        let input = "line1\nline2\nline3\nline4\n";
        let result = enforce_char_cap(input, 12);
        // Should fit "line1\nline2\n" = 12 chars
        assert!(result.len() <= 12);
        assert!(result.contains("line1"));
    }

    #[test]
    fn test_dedup_allows_sufficiently_different_content() {
        // Two memories that share <80% of first 100 chars should both appear
        let mem1 = format!("AAA{}", "X".repeat(97));
        let mem2 = format!("BBB{}", "Y".repeat(97));
        let memories = vec![
            make_memory(&mem1, "general", None, 10.0),
            make_memory(&mem2, "general", None, 8.0),
        ];
        let result = build_budgeted_export(&memories);
        let entry_count = result.lines().filter(|l| l.starts_with("- ")).count();
        assert_eq!(entry_count, 2, "Different content should not be deduped");
    }
}
