// background/proactive/
// Background proactive suggestion processing
//
// Two-tier frequency:
// - Pattern mining: Every 3rd cycle (~15 minutes) - SQL only, no LLM
// - LLM enhancement: Every 10th cycle (~50 minutes) - generates contextual suggestions

mod llm;
mod lookup;
mod mining;
mod storage;
mod templates;

use crate::db::pool::DatabasePool;
use crate::llm::LlmClient;
use std::sync::Arc;

pub use lookup::{get_pre_generated_suggestions, mark_suggestion_accepted, mark_suggestion_shown};
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
/// - Every 10th cycle: Enhance high-confidence patterns with LLM-generated suggestions
pub async fn process_proactive(
    pool: &Arc<DatabasePool>,
    client: Option<&Arc<dyn LlmClient>>,
    cycle_count: u64,
) -> Result<usize, String> {
    let mut processed = 0;

    // Pattern mining every 3rd cycle (fast, SQL only — always runs)
    if cycle_count.is_multiple_of(3) {
        processed += mining::mine_patterns(pool).await?;
    }

    // LLM enhancement every 10th cycle (or template fallback when no LLM)
    if cycle_count.is_multiple_of(10) {
        match client {
            Some(client) => {
                processed += llm::enhance_suggestions(pool, client).await?;
            }
            None => {
                processed += templates::generate_template_suggestions(pool).await?;
            }
        }
    }

    Ok(processed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proactive::patterns::{BehaviorPattern, PatternData};
    use crate::proactive::PatternType;

    #[test]
    fn test_parse_suggestions_json() {
        let patterns = vec![BehaviorPattern {
            id: Some(1),
            project_id: 1,
            pattern_type: PatternType::FileSequence,
            pattern_key: "test".to_string(),
            pattern_data: PatternData::FileSequence {
                files: vec!["src/main.rs".to_string()],
                transitions: vec![("src/main.rs".to_string(), "src/lib.rs".to_string())],
            },
            confidence: 0.8,
            occurrence_count: 5,
        }];

        let json = r#"[{"trigger": "src/main.rs", "hint": "Check lib.rs for related code"}]"#;
        let suggestions = llm::parse_suggestions(json, &patterns).unwrap();

        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].trigger_key, "src/main.rs");
        assert!(suggestions[0].suggestion_text.contains("lib.rs"));
    }

    #[test]
    fn test_parse_suggestions_markdown() {
        let patterns = vec![];
        let markdown = r#"Here are suggestions:
```json
[{"trigger": "grep", "hint": "Consider semantic search"}]
```"#;
        let suggestions = llm::parse_suggestions(markdown, &patterns).unwrap();
        assert_eq!(suggestions.len(), 1);
    }
}
