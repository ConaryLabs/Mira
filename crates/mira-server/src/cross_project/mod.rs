// crates/mira-server/src/cross_project/mod.rs
// Cross-project intelligence network for privacy-preserving pattern sharing

mod anonymizer;
mod preferences;
mod storage;

pub use anonymizer::{AnonymizationLevel, AnonymizedPattern, PatternAnonymizer};
pub use preferences::{
    SharingPreferences, disable_sharing, enable_sharing, get_preferences, reset_privacy_budget,
    update_preferences,
};
pub use storage::{
    CrossProjectPattern, SharingStats, extract_and_store_patterns, get_patterns_for_project,
    get_shareable_patterns, get_sharing_stats, import_pattern, log_sharing_event, store_pattern,
};

use serde::{Deserialize, Serialize};

/// Types of patterns that can be shared across projects
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CrossPatternType {
    /// File access sequences (e.g., "config then main then tests")
    FileSequence,
    /// Tool usage chains (e.g., "grep then read then edit")
    ToolChain,
    /// Problem patterns from expert consultations
    ProblemPattern,
    /// Expert collaboration patterns
    Collaboration,
    /// Behavior patterns (general)
    Behavior,
}

impl CrossPatternType {
    pub fn as_str(&self) -> &'static str {
        match self {
            CrossPatternType::FileSequence => "file_sequence",
            CrossPatternType::ToolChain => "tool_chain",
            CrossPatternType::ProblemPattern => "problem_pattern",
            CrossPatternType::Collaboration => "collaboration",
            CrossPatternType::Behavior => "behavior",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "file_sequence" => Some(CrossPatternType::FileSequence),
            "tool_chain" => Some(CrossPatternType::ToolChain),
            "problem_pattern" => Some(CrossPatternType::ProblemPattern),
            "collaboration" => Some(CrossPatternType::Collaboration),
            "behavior" => Some(CrossPatternType::Behavior),
            _ => None,
        }
    }
}

/// Direction of pattern sharing
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SharingDirection {
    Export,
    Import,
}

impl SharingDirection {
    pub fn as_str(&self) -> &'static str {
        match self {
            SharingDirection::Export => "exported",
            SharingDirection::Import => "imported",
        }
    }
}

/// Configuration for cross-project intelligence
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossProjectConfig {
    /// Minimum confidence threshold for sharing patterns
    pub min_confidence: f64,
    /// K-anonymity threshold (minimum projects before pattern is shareable)
    pub k_anonymity_threshold: u32,
    /// Differential privacy epsilon (privacy budget)
    pub epsilon: f64,
    /// Maximum patterns to import per sync
    pub max_import_count: usize,
    /// Categories to include/exclude
    pub category_filter: Option<Vec<String>>,
}

impl Default for CrossProjectConfig {
    fn default() -> Self {
        Self {
            min_confidence: 0.6,
            k_anonymity_threshold: 3,
            epsilon: 1.0,
            max_import_count: 50,
            category_filter: None,
        }
    }
}
