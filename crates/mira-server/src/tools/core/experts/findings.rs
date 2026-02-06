// crates/mira-server/src/tools/core/experts/findings.rs
// ParsedFinding struct, parsing/storage logic, and council findings store

use super::ToolContext;
use crate::db::ReviewFindingParams;
use serde::{Deserialize, Serialize};
use std::sync::Mutex;

/// Minimum character length for a finding to be considered valid.
pub const MIN_FINDING_LENGTH: usize = 20;

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
            && let Some(pos) = trimmed.find('.')
            && pos < 4
        {
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
    findings: Vec<ParsedFinding>,
    expert_role: &str,
) -> usize {
    use crate::db::store_review_finding_sync;

    let project_id = ctx.project_id().await;
    let session_id = ctx.get_or_create_session().await;
    let user_id = ctx.get_user_identity();

    let mut stored = 0;
    for finding in findings {
        if finding.content.len() < MIN_FINDING_LENGTH {
            continue; // Skip very short findings
        }

        let params = ReviewFindingParams {
            project_id,
            expert_role: expert_role.to_string(),
            file_path: finding.file_path,
            finding_type: finding.finding_type,
            severity: finding.severity,
            content: finding.content,
            code_snippet: finding.code_snippet,
            suggestion: finding.suggestion,
            confidence: 0.7, // Default confidence for parsed findings
            user_id: user_id.clone(),
            session_id: Some(session_id.clone()),
        };

        let result = ctx
            .pool()
            .run(move |conn| store_review_finding_sync(conn, &params))
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

// ═══════════════════════════════════════════════════════════════════════════════
// Council Findings Store (per-consultation, in-memory only)
// ═══════════════════════════════════════════════════════════════════════════════

/// Maximum findings per council consultation (prevents runaway experts).
const MAX_COUNCIL_FINDINGS: usize = 50;

/// Maximum findings per expert role (prevents one expert from monopolizing the store).
const MAX_FINDINGS_PER_ROLE: usize = 20;

/// Result of attempting to add a finding to the store.
#[derive(Debug)]
pub enum AddFindingResult {
    /// Finding was added successfully.
    Added { total: usize },
    /// This expert has hit their per-role limit.
    RoleLimitReached { role_count: usize },
    /// The global findings limit has been reached.
    GlobalLimitReached { total: usize },
}

/// A structured finding emitted by an expert during a council consultation
/// via the `store_finding` tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CouncilFinding {
    pub role: String,
    pub topic: String,
    pub content: String,
    #[serde(default)]
    pub evidence: Vec<String>,
    #[serde(default = "default_severity")]
    pub severity: String,
    pub recommendation: Option<String>,
}

fn default_severity() -> String {
    "info".to_string()
}

/// Thread-safe, per-consultation store for council findings.
/// Created once per `run_council()` call and shared across expert tasks.
pub struct FindingsStore {
    findings: Mutex<Vec<CouncilFinding>>,
}

impl FindingsStore {
    pub fn new() -> Self {
        Self {
            findings: Mutex::new(Vec::new()),
        }
    }

    /// Add a finding. Returns the result indicating success or which limit was hit.
    pub fn add(&self, finding: CouncilFinding) -> AddFindingResult {
        let mut findings = self.findings.lock().unwrap_or_else(|e| e.into_inner());
        if findings.len() >= MAX_COUNCIL_FINDINGS {
            return AddFindingResult::GlobalLimitReached {
                total: findings.len(),
            };
        }
        if !finding.role.is_empty() {
            let role_count = findings.iter().filter(|f| f.role == finding.role).count();
            if role_count >= MAX_FINDINGS_PER_ROLE {
                return AddFindingResult::RoleLimitReached { role_count };
            }
        }
        findings.push(finding);
        AddFindingResult::Added {
            total: findings.len(),
        }
    }

    /// Get findings from a specific role.
    pub fn by_role(&self, role: &str) -> Vec<CouncilFinding> {
        self.findings
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter(|f| f.role == role)
            .cloned()
            .collect()
    }

    /// Number of findings stored.
    pub fn count(&self) -> usize {
        self.findings
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .len()
    }

    /// Format all findings for the synthesis prompt.
    pub fn format_for_synthesis(&self) -> String {
        let findings = self.findings.lock().unwrap_or_else(|e| e.into_inner());
        if findings.is_empty() {
            return "No structured findings were recorded.".to_string();
        }

        let mut output = String::new();
        let mut current_role = "";

        for finding in findings.iter() {
            if finding.role != current_role {
                current_role = &finding.role;
                output.push_str(&format!("\n### {} Findings\n\n", current_role));
            }

            output.push_str(&format!(
                "**[{}] {}**: {}\n",
                finding.severity.to_uppercase(),
                finding.topic,
                finding.content
            ));

            if !finding.evidence.is_empty() {
                for ev in &finding.evidence {
                    output.push_str(&format!("  - Evidence: {}\n", ev));
                }
            }

            if let Some(ref rec) = finding.recommendation {
                output.push_str(&format!("  - Recommendation: {}\n", rec));
            }

            output.push('\n');
        }

        output
    }
}

#[cfg(test)]
impl FindingsStore {
    /// Get all findings (test-only).
    pub fn all(&self) -> Vec<CouncilFinding> {
        self.findings
            .lock()
            .expect("findings mutex not poisoned")
            .clone()
    }

    /// Rough token estimate for the findings (1 token ≈ 4 chars, test-only).
    pub fn estimated_tokens(&self) -> usize {
        let findings = self.findings.lock().expect("findings mutex not poisoned");
        let total_chars: usize = findings
            .iter()
            .map(|f| {
                f.topic.len()
                    + f.content.len()
                    + f.evidence.iter().map(|e| e.len()).sum::<usize>()
                    + f.recommendation.as_ref().map(|r| r.len()).unwrap_or(0)
                    + 50 // overhead per finding
            })
            .sum();
        total_chars / 4
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_finding(role: &str, topic: &str) -> CouncilFinding {
        CouncilFinding {
            role: role.to_string(),
            topic: topic.to_string(),
            content: "Test finding content".to_string(),
            evidence: vec!["file.rs:42".to_string()],
            severity: "medium".to_string(),
            recommendation: Some("Fix this".to_string()),
        }
    }

    #[test]
    fn test_findings_store_add_and_count() {
        let store = FindingsStore::new();
        assert_eq!(store.count(), 0);
        assert!(matches!(
            store.add(sample_finding("architect", "design")),
            AddFindingResult::Added { total: 1 }
        ));
        assert_eq!(store.count(), 1);
    }

    #[test]
    fn test_findings_store_all() {
        let store = FindingsStore::new();
        store.add(sample_finding("architect", "design"));
        store.add(sample_finding("security", "auth"));
        let all = store.all();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn test_findings_store_by_role() {
        let store = FindingsStore::new();
        store.add(sample_finding("architect", "design"));
        store.add(sample_finding("security", "auth"));
        store.add(sample_finding("architect", "patterns"));

        assert_eq!(store.by_role("architect").len(), 2);
        assert_eq!(store.by_role("security").len(), 1);
        assert_eq!(store.by_role("code_reviewer").len(), 0);
    }

    #[test]
    fn test_findings_store_max_cap() {
        let store = FindingsStore::new();
        // Use different roles to avoid per-role limit
        for i in 0..MAX_COUNCIL_FINDINGS {
            let role = format!("role_{}", i % 10);
            assert!(matches!(
                store.add(sample_finding(&role, &format!("topic_{}", i))),
                AddFindingResult::Added { .. }
            ));
        }
        // 51st should fail with global limit
        assert!(matches!(
            store.add(sample_finding("role_0", "overflow")),
            AddFindingResult::GlobalLimitReached { .. }
        ));
        assert_eq!(store.count(), MAX_COUNCIL_FINDINGS);
    }

    #[test]
    fn test_findings_store_per_role_limit() {
        let store = FindingsStore::new();
        for i in 0..MAX_FINDINGS_PER_ROLE {
            assert!(matches!(
                store.add(sample_finding("architect", &format!("topic_{}", i))),
                AddFindingResult::Added { .. }
            ));
        }
        // Next finding from same role should be rejected
        assert!(matches!(
            store.add(sample_finding("architect", "one_too_many")),
            AddFindingResult::RoleLimitReached { .. }
        ));
        // But a different role can still add
        assert!(matches!(
            store.add(sample_finding("security", "still_ok")),
            AddFindingResult::Added { .. }
        ));
    }

    #[test]
    fn test_findings_store_format_for_synthesis() {
        let store = FindingsStore::new();
        store.add(sample_finding("architect", "design"));
        store.add(sample_finding("security", "auth"));

        let formatted = store.format_for_synthesis();
        assert!(formatted.contains("architect Findings"));
        assert!(formatted.contains("security Findings"));
        assert!(formatted.contains("[MEDIUM]"));
        assert!(formatted.contains("Evidence:"));
        assert!(formatted.contains("Recommendation:"));
    }

    #[test]
    fn test_findings_store_format_empty() {
        let store = FindingsStore::new();
        let formatted = store.format_for_synthesis();
        assert!(formatted.contains("No structured findings"));
    }

    #[test]
    fn test_findings_store_estimated_tokens() {
        let store = FindingsStore::new();
        assert_eq!(store.estimated_tokens(), 0);
        store.add(sample_finding("architect", "design"));
        assert!(store.estimated_tokens() > 0);
    }

    #[test]
    fn test_findings_store_thread_safety() {
        use std::sync::Arc;
        use std::thread;

        let store = Arc::new(FindingsStore::new());
        let mut handles = vec![];

        // Use different roles to avoid per-role limit
        for i in 0..10 {
            let store = Arc::clone(&store);
            handles.push(thread::spawn(move || {
                let role = format!("role_{}", i % 5);
                store.add(sample_finding(&role, &format!("topic_{}", i)));
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        assert_eq!(store.count(), 10);
    }
}
