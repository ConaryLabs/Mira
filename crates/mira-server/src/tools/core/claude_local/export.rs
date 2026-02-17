use crate::db::{RankedMemory, fetch_ranked_memories_for_export_sync};
use crate::error::MiraError;
use crate::tools::core::ToolContext;
use std::collections::HashSet;
use std::path::Path;

/// Total byte budget for CLAUDE.local.md content (~2K tokens)
const CLAUDE_LOCAL_BYTE_BUDGET: usize = 8192;

/// Max bytes per individual memory entry (truncate verbose ones)
const MAX_MEMORY_BYTES: usize = 500;

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
    let mut seen_content: HashSet<String> = HashSet::new();

    for mem in memories {
        if consecutive_skips >= 10 {
            break;
        }

        let content = super::truncate_content(&mem.content, MAX_MEMORY_BYTES);

        // Determine section first so dedup can be scoped per section.
        // This allows the same content prefix to appear in both Preferences
        // and Decisions if it genuinely belongs to both.
        let section_name =
            super::classify_by_type_and_category(&mem.fact_type, mem.category.as_deref());

        // Deduplicate per-section: key = "section:prefix100".
        // Only mark seen after a successful budget fit to avoid a too-large
        // hotter entry poisoning the key and blocking a smaller entry that
        // would have fit.
        let dedup_key: String = content.chars().take(100).collect();
        let composite_key = format!("{}:{}", section_name, dedup_key);
        if seen_content.contains(&composite_key) {
            continue;
        }

        let entry_line = format!("- {}\n", content);
        let entry_bytes = entry_line.len();

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
            // Mark seen ONLY after successful budget add. Inserting before the
            // budget check would cause a too-large entry to poison the key,
            // blocking a smaller same-section entry that would have fit.
            seen_content.insert(composite_key);
            consecutive_skips = 0;
        } else if entry_bytes > budget_remaining {
            // This entry won't fit even without header cost — skip it
            consecutive_skips += 1;
        } else {
            // Header + entry doesn't fit, but maybe just entry doesn't fit either
            consecutive_skips += 1;
        }
    }

    super::render_sections(header, &sections)
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
        // Two memories with same prefix but different suffixes beyond 500 bytes
        // After truncation to MAX_MEMORY_BYTES, they share the same first 100 chars
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
    fn test_budgeted_export_dedup_is_per_section() {
        // Two memories with identical first 100 chars but different sections
        // should both appear (Codex finding L1 / QA-hardening M1).
        let content = "A".repeat(100);
        let memories = vec![
            make_memory(&content, "preference", Some("preference"), 10.0),
            make_memory(&content, "decision", Some("decision"), 8.0),
        ];
        let result = build_budgeted_export(&memories);
        assert!(
            result.contains("## Preferences"),
            "Preference entry should be present"
        );
        assert!(
            result.contains("## Decisions"),
            "Decision entry should be present"
        );
        let entry_count = result.lines().filter(|l| l.starts_with("- ")).count();
        assert_eq!(
            entry_count, 2,
            "Same content in different sections should not be deduped"
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
}
