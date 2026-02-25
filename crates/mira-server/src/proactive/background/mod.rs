// background/proactive/
// Background proactive suggestion processing
//
// Two-tier frequency:
// - Pattern mining: Every 3rd cycle (~15 minutes) - SQL only, no LLM
// - Template suggestions: Every 10th cycle (~50 minutes) - generates template-based suggestions

mod lookup;
mod mining;
mod storage;
mod templates;

use crate::db::pool::DatabasePool;
use std::sync::Arc;

pub use lookup::{
    get_pre_generated_suggestions, get_top_pre_generated_suggestions, mark_suggestion_accepted,
    mark_suggestion_shown,
};
pub use storage::cleanup_expired_suggestions;

/// Confidence multiplier for template suggestions vs LLM quality
pub(super) const TEMPLATE_CONFIDENCE_MULTIPLIER: f64 = 0.85;

/// Minimum pattern confidence to generate a template suggestion
/// pattern.confidence * TEMPLATE_CONFIDENCE_MULTIPLIER must be >= 0.7
pub(super) const MIN_PATTERN_CONFIDENCE: f64 = 0.7;

/// A pre-generated suggestion ready for storage
#[derive(Debug)]
pub(super) struct PreGeneratedSuggestion {
    pub pattern_id: Option<i64>,
    pub trigger_key: String,
    pub suggestion_text: String,
    pub confidence: f64,
}

/// Whether pattern mining should run on this cycle (every 3rd cycle)
pub(crate) fn should_run_mining(cycle_count: u64) -> bool {
    cycle_count.is_multiple_of(3)
}

/// Whether template suggestions should run on this cycle (every 10th cycle)
pub(crate) fn should_run_suggestions(cycle_count: u64) -> bool {
    cycle_count.is_multiple_of(10)
}

/// Process proactive suggestions in background
///
/// - Every 3rd cycle: Mine patterns from behavior logs (SQL only, fast)
/// - Every 10th cycle: Generate template-based suggestions from high-confidence patterns
pub async fn process_proactive(
    pool: &Arc<DatabasePool>,
    cycle_count: u64,
) -> Result<usize, String> {
    let mut processed = 0;

    // Pattern mining every 3rd cycle (fast, SQL only -- always runs)
    if should_run_mining(cycle_count) {
        processed += mining::mine_patterns(pool).await?;
    }

    // Template suggestions every 10th cycle
    if should_run_suggestions(cycle_count) {
        processed += templates::generate_template_suggestions(pool).await?;
    }

    Ok(processed)
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // should_run_mining: every 3rd cycle
    // =========================================================================

    #[test]
    fn mining_runs_on_multiples_of_3() {
        // Explicit boundary cases
        assert!(should_run_mining(3), "cycle 3");
        assert!(should_run_mining(6), "cycle 6");
        assert!(should_run_mining(9), "cycle 9");
        assert!(should_run_mining(30), "cycle 30");
        // 0 is a multiple of every positive integer
        assert!(should_run_mining(0), "cycle 0");
    }

    #[test]
    fn mining_skips_non_multiples_of_3() {
        assert!(!should_run_mining(1), "cycle 1");
        assert!(!should_run_mining(2), "cycle 2");
        assert!(!should_run_mining(4), "cycle 4");
        assert!(!should_run_mining(5), "cycle 5");
        assert!(!should_run_mining(7), "cycle 7");
        assert!(!should_run_mining(8), "cycle 8");
        assert!(!should_run_mining(11), "cycle 11");
    }

    #[test]
    fn mining_exhaustive_first_30_cycles() {
        for cycle in 0u64..=30 {
            let expected = cycle % 3 == 0;
            assert_eq!(
                should_run_mining(cycle),
                expected,
                "cycle {cycle}: expected mining={expected}"
            );
        }
    }

    // =========================================================================
    // should_run_suggestions: every 10th cycle
    // =========================================================================

    #[test]
    fn suggestions_run_on_multiples_of_10() {
        assert!(should_run_suggestions(10), "cycle 10");
        assert!(should_run_suggestions(20), "cycle 20");
        assert!(should_run_suggestions(30), "cycle 30");
        assert!(should_run_suggestions(0), "cycle 0");
    }

    #[test]
    fn suggestions_skip_non_multiples_of_10() {
        assert!(!should_run_suggestions(1), "cycle 1");
        assert!(!should_run_suggestions(9), "cycle 9");
        assert!(!should_run_suggestions(11), "cycle 11");
        assert!(!should_run_suggestions(15), "cycle 15");
        assert!(!should_run_suggestions(19), "cycle 19");
        assert!(!should_run_suggestions(21), "cycle 21");
    }

    #[test]
    fn suggestions_exhaustive_first_30_cycles() {
        for cycle in 0u64..=30 {
            let expected = cycle % 10 == 0;
            assert_eq!(
                should_run_suggestions(cycle),
                expected,
                "cycle {cycle}: expected suggestions={expected}"
            );
        }
    }

    // =========================================================================
    // Combined: verify mining and suggestion intervals are independent
    // =========================================================================

    #[test]
    fn mining_and_suggestions_independent_gating() {
        // Cycles that trigger both (multiples of lcm(3,10)=30)
        for cycle in [0u64, 30, 60, 90] {
            assert!(should_run_mining(cycle), "cycle {cycle}: mining");
            assert!(should_run_suggestions(cycle), "cycle {cycle}: suggestions");
        }

        // Cycles that trigger only mining (multiples of 3 but not 10)
        for cycle in [3u64, 6, 9, 12, 21, 27] {
            assert!(should_run_mining(cycle), "cycle {cycle}: mining");
            assert!(
                !should_run_suggestions(cycle),
                "cycle {cycle}: no suggestions"
            );
        }

        // Cycles that trigger only suggestions (multiples of 10 but not 3)
        for cycle in [10u64, 20, 40, 50, 70, 80] {
            assert!(!should_run_mining(cycle), "cycle {cycle}: no mining");
            assert!(should_run_suggestions(cycle), "cycle {cycle}: suggestions");
        }

        // Cycles that trigger neither
        for cycle in [1u64, 2, 4, 7, 11, 13] {
            assert!(!should_run_mining(cycle), "cycle {cycle}: no mining");
            assert!(
                !should_run_suggestions(cycle),
                "cycle {cycle}: no suggestions"
            );
        }
    }
}
