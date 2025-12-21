//! Mira Intelligence Integration
//!
//! Integrates Mira's learning capabilities into the conductor:
//! - Error fix lookup (find_similar_fixes)
//! - Rejected approach filtering
//! - Cochange pattern context
//!
//! Uses core::ops for all business logic - this is a thin interface layer.

use std::sync::Arc;

use sqlx::sqlite::SqlitePool;

use crate::core::ops::build as core_build;
use crate::core::ops::git as core_git;
use crate::core::ops::mira as core_mira;
use crate::core::primitives::semantic::SemanticSearch;
use crate::core::OpContext;

/// Mira intelligence provider for the conductor
pub struct MiraIntel {
    db: SqlitePool,
    semantic: Option<Arc<SemanticSearch>>,
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
        Self {
            db,
            semantic: None,
            project_id,
        }
    }

    /// Create with semantic search support
    pub fn with_semantic(db: SqlitePool, semantic: Arc<SemanticSearch>, project_id: Option<i64>) -> Self {
        Self {
            db,
            semantic: Some(semantic),
            project_id,
        }
    }

    /// Build OpContext for core operations
    fn context(&self) -> OpContext {
        let mut ctx = OpContext::new(std::env::current_dir().unwrap_or_default())
            .with_db(self.db.clone());
        if let Some(ref semantic) = self.semantic {
            ctx = ctx.with_semantic(semantic.clone());
        }
        ctx
    }

    /// Find similar error fixes from past sessions
    ///
    /// Called before retry when a step fails
    pub async fn find_similar_fixes(&self, error_message: &str) -> Vec<FixSuggestion> {
        let ctx = self.context();

        let input = core_build::FindSimilarFixesInput {
            error: error_message.to_string(),
            category: None,
            language: None,
            limit: 3,
        };

        match core_build::find_similar_fixes(&ctx, input).await {
            Ok(fixes) => fixes
                .into_iter()
                .map(|f| FixSuggestion {
                    error_pattern: f.error_pattern,
                    fix_description: f.fix_description.unwrap_or_default(),
                    times_applied: f.times_fixed as i32,
                })
                .collect(),
            Err(e) => {
                tracing::warn!("Failed to find similar fixes: {}", e);
                Vec::new()
            }
        }
    }

    /// Get rejected approaches relevant to a task
    ///
    /// Called during planning to avoid re-suggesting failed solutions
    pub async fn get_rejected_approaches(&self, task_context: &str) -> Vec<RejectedApproach> {
        let ctx = self.context();

        let input = core_mira::GetRejectedApproachesInput {
            task_context: Some(task_context.to_string()),
            project_id: self.project_id,
            limit: 5,
        };

        match core_mira::get_rejected_approaches(&ctx, input).await {
            Ok(approaches) => approaches
                .into_iter()
                .map(|r| RejectedApproach {
                    problem_context: r.problem_context,
                    approach: r.approach,
                    rejection_reason: r.rejection_reason,
                })
                .collect(),
            Err(e) => {
                tracing::warn!("Failed to get rejected approaches: {}", e);
                Vec::new()
            }
        }
    }

    /// Get cochange patterns for a file
    ///
    /// Called when planning edits to include related files
    pub async fn get_cochange_patterns(&self, file_path: &str) -> Vec<CochangePattern> {
        let ctx = self.context();

        let input = core_git::FindCochangeInput {
            file_path: file_path.to_string(),
            limit: 5,
        };

        match core_git::find_cochange_patterns(&ctx, input).await {
            Ok(patterns) => patterns
                .into_iter()
                .map(|p| CochangePattern {
                    related_file: p.file,
                    cochange_count: p.cochange_count as i32,
                    confidence: p.confidence,
                })
                .collect(),
            Err(e) => {
                tracing::warn!("Failed to get cochange patterns: {}", e);
                Vec::new()
            }
        }
    }

    /// Record an error fix for future learning
    pub async fn record_fix(&self, error_pattern: &str, fix_description: &str) {
        let ctx = self.context();

        let input = core_build::RecordErrorFixInput {
            error_pattern: error_pattern.to_string(),
            fix_description: fix_description.to_string(),
            category: None,
            language: None,
            file_pattern: None,
            fix_diff: None,
            fix_commit: None,
        };

        if let Err(e) = core_build::record_error_fix(&ctx, input).await {
            tracing::warn!("Failed to record error fix: {}", e);
        }
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
