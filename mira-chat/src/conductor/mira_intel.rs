//! Mira Intelligence Integration
//!
//! Integrates Mira's learning capabilities into the conductor:
//! - Error fix lookup (find_similar_fixes)
//! - Rejected approach filtering
//! - Cochange pattern context
//! - Smart excerpts for large output

use sqlx::sqlite::SqlitePool;

/// Mira intelligence provider for the conductor
pub struct MiraIntel {
    db: SqlitePool,
    project_id: Option<i64>,
}

/// A fix suggestion from past errors
#[derive(Debug, Clone)]
pub struct FixSuggestion {
    pub error_pattern: String,
    pub fix_description: String,
    pub times_applied: i32,
}

/// A rejected approach to avoid
#[derive(Debug, Clone)]
pub struct RejectedApproach {
    pub problem_context: String,
    pub approach: String,
    pub rejection_reason: String,
}

/// Files that typically change together
#[derive(Debug, Clone)]
pub struct CochangePattern {
    pub related_file: String,
    pub cochange_count: i32,
    pub confidence: f64,
}

impl MiraIntel {
    /// Create a new Mira intelligence provider
    pub fn new(db: SqlitePool, project_id: Option<i64>) -> Self {
        Self { db, project_id }
    }

    /// Find similar error fixes from past sessions
    ///
    /// Called before retry when a step fails
    pub async fn find_similar_fixes(&self, error_message: &str) -> Vec<FixSuggestion> {
        // Extract key terms from error (simple approach - first 100 chars)
        let error_snippet = if error_message.len() > 100 {
            &error_message[..100]
        } else {
            error_message
        };

        let fixes: Vec<(String, String, i32)> = sqlx::query_as(
            r#"
            SELECT error_pattern, fix_description, times_fixed
            FROM error_fixes
            WHERE error_pattern LIKE '%' || $1 || '%'
               OR $1 LIKE '%' || error_pattern || '%'
            ORDER BY times_fixed DESC
            LIMIT 3
            "#
        )
        .bind(error_snippet)
        .fetch_all(&self.db)
        .await
        .unwrap_or_default();

        fixes.into_iter()
            .map(|(pattern, fix, times)| FixSuggestion {
                error_pattern: pattern,
                fix_description: fix,
                times_applied: times,
            })
            .collect()
    }

    /// Get rejected approaches relevant to a task
    ///
    /// Called during planning to avoid re-suggesting failed solutions
    pub async fn get_rejected_approaches(&self, task_context: &str) -> Vec<RejectedApproach> {
        let project_filter = self.project_id.unwrap_or(0);

        let approaches: Vec<(String, String, String)> = sqlx::query_as(
            r#"
            SELECT problem_context, approach, rejection_reason
            FROM rejected_approaches
            WHERE project_id = $1 OR project_id IS NULL
            ORDER BY created_at DESC
            LIMIT 5
            "#
        )
        .bind(project_filter)
        .fetch_all(&self.db)
        .await
        .unwrap_or_default();

        // Filter to relevant ones (simple keyword match)
        let task_lower = task_context.to_lowercase();
        approaches.into_iter()
            .filter(|(ctx, _, _)| {
                let ctx_lower = ctx.to_lowercase();
                // Check for any word overlap
                task_lower.split_whitespace()
                    .any(|word| word.len() > 3 && ctx_lower.contains(word))
            })
            .map(|(ctx, approach, reason)| RejectedApproach {
                problem_context: ctx,
                approach,
                rejection_reason: reason,
            })
            .collect()
    }

    /// Get cochange patterns for a file
    ///
    /// Called when planning edits to include related files
    pub async fn get_cochange_patterns(&self, file_path: &str) -> Vec<CochangePattern> {
        let patterns: Vec<(String, i32, f64)> = sqlx::query_as(
            r#"
            SELECT
                CASE WHEN file_a = $1 THEN file_b ELSE file_a END as related,
                cochange_count,
                confidence
            FROM cochange_patterns
            WHERE file_a = $1 OR file_b = $1
            ORDER BY confidence DESC, cochange_count DESC
            LIMIT 5
            "#
        )
        .bind(file_path)
        .fetch_all(&self.db)
        .await
        .unwrap_or_default();

        patterns.into_iter()
            .map(|(file, count, conf)| CochangePattern {
                related_file: file,
                cochange_count: count,
                confidence: conf,
            })
            .collect()
    }

    /// Record an error fix for future learning
    pub async fn record_fix(&self, error_pattern: &str, fix_description: &str) {
        let _ = sqlx::query(
            r#"
            INSERT INTO error_fixes (error_pattern, fix_description, times_seen, times_fixed, last_seen, created_at, project_id)
            VALUES ($1, $2, 1, 1, unixepoch(), unixepoch(), $3)
            ON CONFLICT(error_pattern) DO UPDATE SET
                times_fixed = times_fixed + 1,
                last_seen = unixepoch()
            "#
        )
        .bind(error_pattern)
        .bind(fix_description)
        .bind(self.project_id.unwrap_or(0))
        .execute(&self.db)
        .await;
    }

    /// Format fix suggestions as a prompt hint
    pub fn format_fix_hints(fixes: &[FixSuggestion]) -> String {
        if fixes.is_empty() {
            return String::new();
        }

        let mut hint = String::from("\n## Similar errors fixed before:\n");
        for fix in fixes {
            hint.push_str(&format!(
                "- Pattern: `{}`\n  Fix: {}\n",
                truncate(&fix.error_pattern, 80),
                fix.fix_description
            ));
        }
        hint
    }

    /// Format rejected approaches as a prompt warning
    pub fn format_rejected_approaches(approaches: &[RejectedApproach]) -> String {
        if approaches.is_empty() {
            return String::new();
        }

        let mut warning = String::from("\n## Approaches to AVOID (previously rejected):\n");
        for r in approaches {
            warning.push_str(&format!(
                "- ❌ {} (Reason: {})\n",
                truncate(&r.approach, 100),
                truncate(&r.rejection_reason, 80)
            ));
        }
        warning
    }

    /// Format cochange patterns as context
    pub fn format_cochange_context(patterns: &[CochangePattern]) -> String {
        if patterns.is_empty() {
            return String::new();
        }

        let mut ctx = String::from("\n## Files that typically change together:\n");
        for p in patterns {
            ctx.push_str(&format!(
                "- {} (changed together {} times, {:.0}% confidence)\n",
                p.related_file, p.cochange_count, p.confidence * 100.0
            ));
        }
        ctx
    }
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        s
    } else {
        &s[..max]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_fix_hints_empty() {
        assert_eq!(MiraIntel::format_fix_hints(&[]), "");
    }

    #[test]
    fn test_format_fix_hints() {
        let fixes = vec![FixSuggestion {
            error_pattern: "cannot find type".into(),
            fix_description: "Add import statement".into(),
            times_applied: 3,
        }];
        let hint = MiraIntel::format_fix_hints(&fixes);
        assert!(hint.contains("cannot find type"));
        assert!(hint.contains("Add import"));
    }

    #[test]
    fn test_format_rejected() {
        let rejected = vec![RejectedApproach {
            problem_context: "rate limiting".into(),
            approach: "Use global mutex".into(),
            rejection_reason: "Causes deadlocks".into(),
        }];
        let warning = MiraIntel::format_rejected_approaches(&rejected);
        assert!(warning.contains("AVOID"));
        assert!(warning.contains("global mutex"));
    }

    #[test]
    fn test_truncate() {
        assert_eq!(truncate("short", 10), "short");
        assert_eq!(truncate("longer string here", 6), "longer");
    }
}
