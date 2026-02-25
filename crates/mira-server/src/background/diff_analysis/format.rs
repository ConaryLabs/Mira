// background/diff_analysis/format.rs
// Formatting functions for diff analysis output

use super::types::{DiffAnalysisResult, ImpactAnalysis};

/// Format diff analysis result for display
pub fn format_diff_analysis(result: &DiffAnalysisResult) -> String {
    let mut output = String::new();

    output.push_str(&format!(
        "## Diff Analysis: {}..{}\n\n",
        result.from_ref, result.to_ref
    ));

    // Summary
    output.push_str("### Summary\n");
    output.push_str(&result.summary);
    output.push_str("\n\n");

    // Stats
    output.push_str(&format!(
        "**Stats:** {} files changed, +{} -{}\n\n",
        result.files_changed, result.lines_added, result.lines_removed
    ));

    // Impact
    if let Some(ref impact) = result.impact {
        output.push_str(&format_impact_section(impact));
    }

    output
}

/// Format impact analysis section
fn format_impact_section(impact: &ImpactAnalysis) -> String {
    if impact.affected_functions.is_empty() {
        return String::new();
    }

    let direct = impact
        .affected_functions
        .iter()
        .filter(|(_, _, d)| *d == 1)
        .count();
    let transitive = impact
        .affected_functions
        .iter()
        .filter(|(_, _, d)| *d > 1)
        .count();

    format!(
        "### Impact\n- Directly affected: {} functions\n- Transitively affected: {} functions\n- Affected files: {}\n\n",
        direct,
        transitive,
        impact.affected_files.len()
    )
}
