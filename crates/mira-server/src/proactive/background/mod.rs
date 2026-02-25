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
    if cycle_count.is_multiple_of(3) {
        processed += mining::mine_patterns(pool).await?;
    }

    // Template suggestions every 10th cycle
    if cycle_count.is_multiple_of(10) {
        processed += templates::generate_template_suggestions(pool).await?;
    }

    Ok(processed)
}
