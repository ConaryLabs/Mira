// crates/mira-server/src/tools/core/experts/findings.rs
// ParsedFinding struct and parsing/storage logic

use super::ToolContext;

/// A parsed finding from expert response
#[derive(Debug, Clone)]
pub struct ParsedFinding {
    pub finding_type: String,
    pub severity: String,
    pub content: String,
    pub suggestion: Option<String>,
    pub file_path: Option<String>,
    pub code_snippet: Option<String>,
}

/// Parse structured findings from expert response
/// Looks for common patterns like severity markers, bullet points, etc.
pub fn parse_expert_findings(response: &str, expert_role: &str) -> Vec<ParsedFinding> {
    let mut findings = Vec::new();

    // Determine finding type based on expert role
    let default_type = match expert_role {
        "code_reviewer" => "code_quality",
        "security" => "security",
        "architect" => "architecture",
        "scope_analyst" => "scope",
        "plan_reviewer" => "plan",
        _ => "general",
    };

    // Parse severity markers: [!!] critical, [!] major, [-] minor, [nit] nit
    let severity_patterns = [
        ("[!!]", "critical"),
        ("[!]", "major"),
        ("**Critical**", "critical"),
        ("**Major**", "major"),
        ("**Minor**", "minor"),
        ("CRITICAL:", "critical"),
        ("MAJOR:", "major"),
        ("MINOR:", "minor"),
        ("NIT:", "nit"),
        ("[-]", "minor"),
        ("[nit]", "nit"),
    ];

    // Look for numbered or bulleted findings
    let mut current_severity = "medium";
    let mut current_content = String::new();
    let mut current_suggestion: Option<String> = None;
    let mut in_finding = false;

    for line in response.lines() {
        let trimmed = line.trim();

        // Check for severity markers
        for (pattern, severity) in &severity_patterns {
            if trimmed.contains(pattern) {
                // Save previous finding if any
                if in_finding && !current_content.is_empty() {
                    findings.push(ParsedFinding {
                        finding_type: default_type.to_string(),
                        severity: current_severity.to_string(),
                        content: current_content.trim().to_string(),
                        suggestion: current_suggestion.take(),
                        file_path: None,
                        code_snippet: None,
                    });
                }

                // Start new finding
                current_severity = severity;
                current_content = trimmed.replace(pattern, "").trim().to_string();
                in_finding = true;
                break;
            }
        }

        // Check for numbered findings: "1.", "2.", etc.
        if trimmed
            .chars()
            .next()
            .map(|c| c.is_ascii_digit())
            .unwrap_or(false)
            && trimmed.contains('.')
        {
            if let Some(pos) = trimmed.find('.') {
                if pos < 3 {
                    // Likely a numbered item
                    if in_finding && !current_content.is_empty() {
                        findings.push(ParsedFinding {
                            finding_type: default_type.to_string(),
                            severity: current_severity.to_string(),
                            content: current_content.trim().to_string(),
                            suggestion: current_suggestion.take(),
                            file_path: None,
                            code_snippet: None,
                        });
                    }
                    current_content = trimmed[pos + 1..].trim().to_string();
                    current_severity = "medium";
                    in_finding = true;
                }
            }
        }

        // Look for "Suggestion:" or "Fix:" lines
        if in_finding
            && (trimmed.starts_with("Suggestion:")
                || trimmed.starts_with("Fix:")
                || trimmed.starts_with("Recommendation:"))
        {
            current_suggestion = Some(
                trimmed
                    .trim_start_matches("Suggestion:")
                    .trim_start_matches("Fix:")
                    .trim_start_matches("Recommendation:")
                    .trim()
                    .to_string(),
            );
        }
    }

    // Don't forget the last finding
    if in_finding && !current_content.is_empty() {
        findings.push(ParsedFinding {
            finding_type: default_type.to_string(),
            severity: current_severity.to_string(),
            content: current_content.trim().to_string(),
            suggestion: current_suggestion,
            file_path: None,
            code_snippet: None,
        });
    }

    findings
}

/// Store parsed findings in the database
pub async fn store_findings<C: ToolContext>(
    ctx: &C,
    findings: &[ParsedFinding],
    expert_role: &str,
) -> usize {
    use crate::db::store_review_finding_sync;

    let project_id = ctx.project_id().await;
    let session_id = ctx.get_or_create_session().await;
    let user_id = ctx.get_user_identity();

    let mut stored = 0;
    for finding in findings {
        if finding.content.len() < 10 {
            continue; // Skip very short findings
        }

        let expert_role = expert_role.to_string();
        let file_path = finding.file_path.clone();
        let finding_type = finding.finding_type.clone();
        let severity = finding.severity.clone();
        let content = finding.content.clone();
        let code_snippet = finding.code_snippet.clone();
        let suggestion = finding.suggestion.clone();
        let user_id_clone = user_id.clone();
        let session_id_clone = session_id.clone();

        let result = ctx
            .pool()
            .run(move |conn| {
                store_review_finding_sync(
                    conn,
                    project_id,
                    &expert_role,
                    file_path.as_deref(),
                    &finding_type,
                    &severity,
                    &content,
                    code_snippet.as_deref(),
                    suggestion.as_deref(),
                    0.7, // Default confidence for parsed findings
                    user_id_clone.as_deref(),
                    Some(&session_id_clone),
                )
            })
            .await;

        if result.is_ok() {
            stored += 1;
        }
    }

    if stored > 0 {
        tracing::info!(stored, expert_role, "Stored review findings");
    }

    stored
}
