// background/diff_analysis/types.rs
// Type definitions for semantic diff analysis

use serde::{Deserialize, Serialize};

/// A semantic change identified in the diff
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticChange {
    pub change_type: String,
    pub file_path: String,
    pub symbol_name: Option<String>,
    pub description: String,
    pub breaking: bool,
    pub security_relevant: bool,
}

/// Impact analysis results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImpactAnalysis {
    /// Functions affected (name, file, depth from changed code)
    pub affected_functions: Vec<(String, String, u32)>,
    /// Files that might be affected
    pub affected_files: Vec<String>,
}

/// Risk assessment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskAssessment {
    pub overall: String, // Low, Medium, High, Critical
    pub flags: Vec<String>,
}

/// Complete diff analysis result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffAnalysisResult {
    pub from_ref: String,
    pub to_ref: String,
    pub changes: Vec<SemanticChange>,
    pub impact: Option<ImpactAnalysis>,
    pub risk: RiskAssessment,
    pub summary: String,
    pub files_changed: i64,
    pub lines_added: i64,
    pub lines_removed: i64,
    /// Full list of changed file paths (from git numstat)
    #[serde(default)]
    pub files: Vec<String>,
}

/// Diff statistics from git
#[derive(Debug, Default)]
pub struct DiffStats {
    pub files_changed: i64,
    pub lines_added: i64,
    pub lines_removed: i64,
    pub files: Vec<String>,
}

/// Historical risk assessment computed from mined change patterns
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoricalRisk {
    /// Overall risk adjustment: "elevated" or "normal"
    pub risk_delta: String,
    /// Patterns that matched the current diff
    pub matching_patterns: Vec<MatchedPattern>,
    /// Weighted average confidence across matched patterns
    pub overall_confidence: f64,
}

/// A single pattern that matched the current diff
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchedPattern {
    /// Pattern subtype: "module_hotspot", "co_change_gap", "size_risk"
    pub pattern_subtype: String,
    /// Human-readable description of the match
    pub description: String,
    /// Pattern confidence (0.0-1.0)
    pub confidence: f64,
    /// Bad outcome rate from historical data
    pub bad_rate: f64,
}
