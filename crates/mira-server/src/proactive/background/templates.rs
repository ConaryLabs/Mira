// background/proactive/templates.rs
// Template-based suggestion generation (no LLM fallback)

use super::storage::store_suggestions;
use super::{MIN_PATTERN_CONFIDENCE, PreGeneratedSuggestion, TEMPLATE_CONFIDENCE_MULTIPLIER};
use crate::background::TEMPLATE_PREFIX;
use crate::db::get_projects_needing_suggestions_sync;
use crate::db::pool::DatabasePool;
use crate::proactive::patterns::{BehaviorPattern, PatternData, get_high_confidence_patterns};
use crate::utils::ResultExt;
use std::sync::Arc;

/// Generate template-based suggestions from high-confidence patterns (no LLM)
pub(super) async fn generate_template_suggestions(
    pool: &Arc<DatabasePool>,
) -> Result<usize, String> {
    // Get projects with high-confidence patterns (same query as LLM path)
    let projects_with_patterns = pool
        .interact(|conn| {
            get_projects_needing_suggestions_sync(conn)
                .map_err(|e| anyhow::anyhow!("Failed to get projects: {}", e))
        })
        .await
        .str_err()?;

    let mut total_suggestions = 0;

    for project_id in projects_with_patterns {
        let patterns: Vec<BehaviorPattern> = {
            let pool_clone = pool.clone();
            pool_clone
                .interact(move |conn| {
                    get_high_confidence_patterns(conn, project_id, MIN_PATTERN_CONFIDENCE)
                        .map_err(|e| anyhow::anyhow!("Failed to get patterns: {}", e))
                })
                .await
                .str_err()?
        };

        if patterns.is_empty() {
            continue;
        }

        let suggestions: Vec<PreGeneratedSuggestion> = patterns
            .iter()
            .take(10)
            .filter_map(|p| {
                let adjusted_confidence = p.confidence * TEMPLATE_CONFIDENCE_MULTIPLIER;
                if adjusted_confidence < MIN_PATTERN_CONFIDENCE {
                    return None;
                }

                match &p.pattern_data {
                    PatternData::FileSequence { transitions, .. } => {
                        if transitions.is_empty() {
                            return None;
                        }
                        let (from, to) = &transitions[0];
                        Some(PreGeneratedSuggestion {
                            pattern_id: p.id,
                            trigger_key: from.clone(),
                            suggestion_text: format!("{}Often edited with {}", TEMPLATE_PREFIX, to),
                            confidence: adjusted_confidence,
                        })
                    }
                    PatternData::ToolChain { tools, .. } => {
                        if tools.len() < 2 {
                            return None;
                        }
                        Some(PreGeneratedSuggestion {
                            pattern_id: p.id,
                            trigger_key: tools[0].clone(),
                            suggestion_text: format!(
                                "{}Usually followed by {}",
                                TEMPLATE_PREFIX, tools[1]
                            ),
                            confidence: adjusted_confidence,
                        })
                    }
                    _ => None,
                }
            })
            .collect();

        if !suggestions.is_empty() {
            let stored = store_suggestions(pool, project_id, &suggestions).await?;
            total_suggestions += stored;
        }
    }

    if total_suggestions > 0 {
        tracing::info!(
            "Proactive: generated {} template suggestions",
            total_suggestions
        );
    }

    Ok(total_suggestions)
}
