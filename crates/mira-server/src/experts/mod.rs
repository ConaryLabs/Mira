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
