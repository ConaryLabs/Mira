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
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
    strum::IntoStaticStr,
    strum::EnumString,
)]
#[strum(serialize_all = "snake_case")]
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
        self.into()
    }
}

/// Direction of pattern sharing
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, strum::IntoStaticStr)]
pub enum SharingDirection {
    #[strum(serialize = "exported")]
    Export,
    #[strum(serialize = "imported")]
    Import,
}

impl SharingDirection {
    pub fn as_str(&self) -> &'static str {
        self.into()
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
