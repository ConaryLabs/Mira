// crates/mira-server/src/tools/core/claude_local/mod.rs
// CLAUDE.local.md integration - bidirectional sync with Mira memories

mod auto_memory;
mod export;
mod import;

pub use auto_memory::{auto_memory_dir_exists, get_auto_memory_dir, write_auto_memory_sync};
pub use export::{build_budgeted_export, export_claude_local, write_claude_local_md_sync};
pub use import::{import_claude_local_md_async, parse_claude_local_md};

/// Max ranked memories to fetch from DB (more than budget allows, gives room for packing)
const RANKED_FETCH_LIMIT: usize = 200;

/// Classify a memory into a section bucket based on fact_type and category.
/// Works for both RankedMemory and AutoMemoryCandidate via the two field accessors.
fn classify_by_type_and_category(fact_type: &str, category: Option<&str>) -> &'static str {
    match fact_type {
        "preference" => "Preferences",
        "decision" => "Decisions",
        "pattern" | "convention" => "Patterns",
        _ => match category {
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

/// Render named sections as markdown. Returns empty string if all sections are empty.
fn render_sections(header: &str, sections: &[(&str, Vec<String>)]) -> String {
    if sections.iter().all(|(_, entries)| entries.is_empty()) {
        return String::new();
    }

    let mut output = String::from(header);
    for (name, entries) in sections {
        if entries.is_empty() {
            continue;
        }
        output.push_str(&format!("## {}\n\n", name));
        for entry in entries {
            output.push_str(entry);
        }
        output.push('\n');
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_by_type_and_category() {
        // Test fact_type classification
        assert_eq!(
            classify_by_type_and_category("preference", None),
            "Preferences"
        );
        assert_eq!(classify_by_type_and_category("decision", None), "Decisions");
        assert_eq!(classify_by_type_and_category("pattern", None), "Patterns");
        assert_eq!(
            classify_by_type_and_category("convention", None),
            "Patterns"
        );

        // Test category fallback
        assert_eq!(
            classify_by_type_and_category("general", Some("preference")),
            "Preferences"
        );
        assert_eq!(
            classify_by_type_and_category("general", Some("decision")),
            "Decisions"
        );

        // Default to General
        assert_eq!(
            classify_by_type_and_category("general", Some("other")),
            "General"
        );
        assert_eq!(classify_by_type_and_category("general", None), "General");
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
