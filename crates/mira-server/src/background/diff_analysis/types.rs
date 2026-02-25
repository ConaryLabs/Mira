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

