// background/diff_analysis/format.rs
// Formatting functions for diff analysis output

use super::types::{DiffAnalysisResult, HistoricalRisk, ImpactAnalysis, SemanticChange};
use std::collections::HashSet;

/// Format diff analysis result for display
pub fn format_diff_analysis(
    result: &DiffAnalysisResult,
    historical_risk: Option<&HistoricalRisk>,
) -> String {
    let mut output = String::new();

    output.push_str(&format!(
        "## Semantic Diff Analysis: {}..{}\n\n",
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

    // Changes
    if !result.changes.is_empty() {
        output.push_str(&format_changes_section(&result.changes));
    }

    // Impact
    if let Some(ref impact) = result.impact {
        output.push_str(&format_impact_section(impact));
    }

    // Risk
    output.push_str(&format!("### Risk: {}\n", result.risk.overall));
    for flag in &result.risk.flags {
        output.push_str(&format!("- {}\n", flag));
    }

    // Historical Risk (from mined change patterns)
    if let Some(hr) = historical_risk {
        output.push_str(&format!(
            "\n### Historical Risk: {}\n",
            hr.risk_delta.to_uppercase()
        ));
        output.push_str(&format!(
            "Based on {} matching pattern(s) (confidence: {:.0}%)\n",
            hr.matching_patterns.len(),
            hr.overall_confidence * 100.0
        ));
        for mp in &hr.matching_patterns {
            output.push_str(&format!(
                "- **{}**: {}\n",
                mp.pattern_subtype, mp.description
            ));
        }
    }

    output
}

/// Format grouped changes section
fn format_changes_section(changes: &[SemanticChange]) -> String {
    let mut output = format!("### Changes ({})\n", changes.len());

    let groups: &[(&str, &[&str])] = &[
        ("New Features", &["NewFunction", "NewFeature"]),
        (
            "Modifications",
            &["ModifiedFunction", "SignatureChange", "Refactoring"],
        ),
        ("Deletions", &["DeletedFunction"]),
    ];

    let mut classified: HashSet<(&str, &str, &str)> = HashSet::new();

    for (title, types) in groups {
        let matching: Vec<_> = changes
            .iter()
            .filter(|c| types.contains(&c.change_type.as_str()))
            .collect();

        if !matching.is_empty() {
            output.push_str(&format!("**{}**\n", title));
            for c in &matching {
                let markers = format_change_markers(c);
                output.push_str(&format!(
                    "- {}: {}{}\n",
                    c.file_path, c.description, markers
                ));
                classified.insert((&c.file_path, &c.description, &c.change_type));
            }
            output.push('\n');
        }
    }

    // Other (unclassified)
    let other: Vec<_> = changes
        .iter()
        .filter(|c| {
            !classified.contains(&(
                c.file_path.as_str(),
                c.description.as_str(),
                c.change_type.as_str(),
            ))
        })
        .collect();

    if !other.is_empty() {
        output.push_str("**Other Changes**\n");
        for c in other {
            let markers = format_change_markers(c);
            output.push_str(&format!(
                "- {}: {}{}\n",
                c.file_path, c.description, markers
            ));
        }
        output.push('\n');
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

pub(super) fn format_change_markers(change: &SemanticChange) -> String {
    let mut markers = String::new();
    if change.breaking {
        markers.push_str(" [BREAKING]");
    }
    if change.security_relevant {
        markers.push_str(" [SECURITY]");
    }
    markers
}
