// crates/mira-server/src/experts/mod.rs
// Evolutionary Expert System - experts that learn and adapt over time

pub mod consultation;
pub mod patterns;
pub mod adaptation;
pub mod collaboration;

use serde::{Deserialize, Serialize};

/// Expert roles available in the system
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExpertRole {
    Architect,
    CodeReviewer,
    Security,
    PlanReviewer,
    ScopeAnalyst,
}

impl ExpertRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            ExpertRole::Architect => "architect",
            ExpertRole::CodeReviewer => "code_reviewer",
            ExpertRole::Security => "security",
            ExpertRole::PlanReviewer => "plan_reviewer",
            ExpertRole::ScopeAnalyst => "scope_analyst",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "architect" => Some(ExpertRole::Architect),
            "code_reviewer" => Some(ExpertRole::CodeReviewer),
            "security" => Some(ExpertRole::Security),
            "plan_reviewer" => Some(ExpertRole::PlanReviewer),
            "scope_analyst" => Some(ExpertRole::ScopeAnalyst),
            _ => None,
        }
    }

    pub fn all() -> &'static [ExpertRole] {
        &[
            ExpertRole::Architect,
            ExpertRole::CodeReviewer,
            ExpertRole::Security,
            ExpertRole::PlanReviewer,
            ExpertRole::ScopeAnalyst,
        ]
    }

    /// Get the base expertise domain for this expert
    pub fn domain(&self) -> &'static str {
        match self {
            ExpertRole::Architect => "system_design",
            ExpertRole::CodeReviewer => "code_quality",
            ExpertRole::Security => "security",
            ExpertRole::PlanReviewer => "planning",
            ExpertRole::ScopeAnalyst => "requirements",
        }
    }
}

/// Problem complexity assessment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplexityAssessment {
    pub score: f64,              // 0.0-1.0
    pub file_count: usize,
    pub domains_involved: Vec<String>,
    pub interdependency_level: f64,
    pub risk_level: f64,
}

impl ComplexityAssessment {
    pub fn simple() -> Self {
        Self {
            score: 0.2,
            file_count: 1,
            domains_involved: vec![],
            interdependency_level: 0.1,
            risk_level: 0.1,
        }
    }

    pub fn from_context(context: &str) -> Self {
        // Analyze context to estimate complexity
        let file_count = context.matches("file").count().max(1);
        let has_security = context.to_lowercase().contains("security")
            || context.to_lowercase().contains("auth");
        let has_architecture = context.to_lowercase().contains("architect")
            || context.to_lowercase().contains("design");
        let has_performance = context.to_lowercase().contains("performance")
            || context.to_lowercase().contains("optim");

        let mut domains = vec![];
        if has_security { domains.push("security".to_string()); }
        if has_architecture { domains.push("architecture".to_string()); }
        if has_performance { domains.push("performance".to_string()); }

        let domain_count = domains.len();
        let interdependency = (domain_count as f64 * 0.2).min(1.0);
        let risk = if has_security { 0.7 } else { 0.3 };

        let score = (file_count as f64 * 0.1 + interdependency * 0.4 + risk * 0.3).min(1.0);

        Self {
            score,
            file_count,
            domains_involved: domains,
            interdependency_level: interdependency,
            risk_level: risk,
        }
    }
}

/// Collaboration mode for multi-expert consultations
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CollaborationMode {
    /// Experts work simultaneously, results synthesized
    Parallel,
    /// Experts work in sequence, each building on previous
    Sequential,
    /// One lead expert coordinates others
    Hierarchical,
    /// Single expert handles the problem
    Single,
}

impl CollaborationMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            CollaborationMode::Parallel => "parallel",
            CollaborationMode::Sequential => "sequential",
            CollaborationMode::Hierarchical => "hierarchical",
            CollaborationMode::Single => "single",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ═══════════════════════════════════════════════════════════════════════════════
    // ExpertRole Tests
    // ═══════════════════════════════════════════════════════════════════════════════

    #[test]
    fn test_expert_role_as_str() {
        assert_eq!(ExpertRole::Architect.as_str(), "architect");
        assert_eq!(ExpertRole::CodeReviewer.as_str(), "code_reviewer");
        assert_eq!(ExpertRole::Security.as_str(), "security");
        assert_eq!(ExpertRole::PlanReviewer.as_str(), "plan_reviewer");
        assert_eq!(ExpertRole::ScopeAnalyst.as_str(), "scope_analyst");
    }

    #[test]
    fn test_expert_role_from_str() {
        assert_eq!(ExpertRole::from_str("architect"), Some(ExpertRole::Architect));
        assert_eq!(ExpertRole::from_str("code_reviewer"), Some(ExpertRole::CodeReviewer));
        assert_eq!(ExpertRole::from_str("security"), Some(ExpertRole::Security));
        assert_eq!(ExpertRole::from_str("plan_reviewer"), Some(ExpertRole::PlanReviewer));
        assert_eq!(ExpertRole::from_str("scope_analyst"), Some(ExpertRole::ScopeAnalyst));
        assert_eq!(ExpertRole::from_str("invalid"), None);
        assert_eq!(ExpertRole::from_str(""), None);
    }

    #[test]
    fn test_expert_role_all() {
        let all = ExpertRole::all();
        assert_eq!(all.len(), 5);
        assert!(all.contains(&ExpertRole::Architect));
        assert!(all.contains(&ExpertRole::CodeReviewer));
        assert!(all.contains(&ExpertRole::Security));
        assert!(all.contains(&ExpertRole::PlanReviewer));
        assert!(all.contains(&ExpertRole::ScopeAnalyst));
    }

    #[test]
    fn test_expert_role_domain() {
        assert_eq!(ExpertRole::Architect.domain(), "system_design");
        assert_eq!(ExpertRole::CodeReviewer.domain(), "code_quality");
        assert_eq!(ExpertRole::Security.domain(), "security");
        assert_eq!(ExpertRole::PlanReviewer.domain(), "planning");
        assert_eq!(ExpertRole::ScopeAnalyst.domain(), "requirements");
    }

    #[test]
    fn test_expert_role_roundtrip() {
        for role in ExpertRole::all() {
            let s = role.as_str();
            let parsed = ExpertRole::from_str(s);
            assert_eq!(parsed, Some(*role), "Roundtrip failed for {:?}", role);
        }
    }

    #[test]
    fn test_expert_role_serialization() {
        let role = ExpertRole::Security;
        let json = serde_json::to_string(&role).unwrap();
        assert_eq!(json, "\"security\"");

        let parsed: ExpertRole = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, role);
    }

    // ═══════════════════════════════════════════════════════════════════════════════
    // ComplexityAssessment Tests
    // ═══════════════════════════════════════════════════════════════════════════════

    #[test]
    fn test_complexity_assessment_simple() {
        let assessment = ComplexityAssessment::simple();
        assert_eq!(assessment.score, 0.2);
        assert_eq!(assessment.file_count, 1);
        assert!(assessment.domains_involved.is_empty());
        assert_eq!(assessment.interdependency_level, 0.1);
        assert_eq!(assessment.risk_level, 0.1);
    }

    #[test]
    fn test_complexity_assessment_from_context_security() {
        let assessment = ComplexityAssessment::from_context("Review the security of the authentication module");
        assert!(assessment.domains_involved.contains(&"security".to_string()));
        assert_eq!(assessment.risk_level, 0.7); // Security raises risk
    }

    #[test]
    fn test_complexity_assessment_from_context_architecture() {
        let assessment = ComplexityAssessment::from_context("Design a new architecture for the system");
        assert!(assessment.domains_involved.contains(&"architecture".to_string()));
    }

    #[test]
    fn test_complexity_assessment_from_context_performance() {
        let assessment = ComplexityAssessment::from_context("Optimize the query performance");
        assert!(assessment.domains_involved.contains(&"performance".to_string()));
    }

    #[test]
    fn test_complexity_assessment_from_context_multiple_domains() {
        let assessment = ComplexityAssessment::from_context(
            "Design a secure architecture with performance optimization"
        );
        assert!(assessment.domains_involved.len() >= 2);
        assert!(assessment.interdependency_level > 0.1);
    }

    #[test]
    fn test_complexity_assessment_from_context_file_count() {
        let assessment = ComplexityAssessment::from_context("Check file A and file B and file C");
        assert_eq!(assessment.file_count, 3);
    }

    #[test]
    fn test_complexity_assessment_from_context_no_keywords() {
        let assessment = ComplexityAssessment::from_context("hello world");
        assert!(assessment.domains_involved.is_empty());
        assert_eq!(assessment.risk_level, 0.3); // Default low risk
    }

    #[test]
    fn test_complexity_assessment_score_bounded() {
        // Test that score is capped at 1.0 even with many domains
        let assessment = ComplexityAssessment::from_context(
            "security auth architecture design performance optimization file file file file file"
        );
        assert!(assessment.score <= 1.0);
    }

    // ═══════════════════════════════════════════════════════════════════════════════
    // CollaborationMode Tests
    // ═══════════════════════════════════════════════════════════════════════════════

    #[test]
    fn test_collaboration_mode_as_str() {
        assert_eq!(CollaborationMode::Parallel.as_str(), "parallel");
        assert_eq!(CollaborationMode::Sequential.as_str(), "sequential");
        assert_eq!(CollaborationMode::Hierarchical.as_str(), "hierarchical");
        assert_eq!(CollaborationMode::Single.as_str(), "single");
    }

    #[test]
    fn test_collaboration_mode_serialization() {
        let mode = CollaborationMode::Parallel;
        let json = serde_json::to_string(&mode).unwrap();
        assert_eq!(json, "\"parallel\"");

        let parsed: CollaborationMode = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, mode);
    }

    #[test]
    fn test_collaboration_mode_equality() {
        assert_eq!(CollaborationMode::Single, CollaborationMode::Single);
        assert_ne!(CollaborationMode::Single, CollaborationMode::Parallel);
    }
}
