// background/diff_analysis/heuristic.rs
// Heuristic (non-LLM) diff analysis: factual stats-based summary only

use super::types::DiffStats;

/// Analyze diff heuristically without LLM.
/// Returns a factual summary string based on git stats.
pub fn analyze_diff_heuristic(diff_content: &str, stats: &DiffStats) -> String {
    if diff_content.is_empty() {
        return "[heuristic] No changes".to_string();
    }

    format!(
        "[heuristic] {} files changed (+{} -{})",
        stats.files_changed, stats.lines_added, stats.lines_removed,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_analyze_diff_heuristic_empty_diff() {
        let stats = DiffStats::default();
        let summary = analyze_diff_heuristic("", &stats);
        assert_eq!(summary, "[heuristic] No changes");
    }

    #[test]
    fn test_analyze_diff_heuristic_summary_format() {
        let diff = "\
diff --git a/src/lib.rs b/src/lib.rs
+pub fn added_fn() {}
-pub fn removed_fn() {}";
        let stats = DiffStats {
            files_changed: 2,
            lines_added: 10,
            lines_removed: 5,
            files: vec!["src/lib.rs".to_string()],
        };

        let summary = analyze_diff_heuristic(diff, &stats);

        assert!(
            summary.starts_with("[heuristic]"),
            "Summary should start with [heuristic] prefix"
        );
        assert!(
            summary.contains("2 files changed"),
            "Summary should contain files_changed from stats"
        );
        assert!(
            summary.contains("+10"),
            "Summary should contain lines_added from stats"
        );
        assert!(
            summary.contains("-5"),
            "Summary should contain lines_removed from stats"
        );
    }
}
