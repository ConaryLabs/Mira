// crates/mira-server/src/proactive/predictor.rs
// Context prediction - uses patterns to predict what the user will need next

use anyhow::Result;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use super::patterns::{PatternData, get_high_confidence_patterns};
use super::{InterventionType, PatternType, ProactiveConfig};

/// A prediction with confidence score
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Prediction {
    pub prediction_type: PredictionType,
    pub content: String,
    pub confidence: f64,
    pub source_pattern_id: Option<i64>,
    pub context: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PredictionType {
    NextFile,           // Predict what file the user will access next
    RelatedFiles,       // Files related to current context
    NextTool,           // Predict what tool will be used next
    RelatedMemories,    // Memories relevant to current context
    ResourceSuggestion, // External resources that might help
    CoChangeSuggestion, // Suggest files that should change together
}

/// Predict next likely files based on current file
pub fn predict_next_files(
    conn: &Connection,
    project_id: i64,
    current_file: &str,
    config: &ProactiveConfig,
) -> Result<Vec<Prediction>> {
    let patterns = get_high_confidence_patterns(conn, project_id, config.min_confidence)?;

    let mut predictions = Vec::new();

    for pattern in patterns {
        if pattern.pattern_type != PatternType::FileSequence {
            continue;
        }

        if let PatternData::FileSequence { files, transitions } = &pattern.pattern_data {
            // Check if current file is in this pattern
            for (from, to) in transitions {
                if from == current_file {
                    predictions.push(Prediction {
                        prediction_type: PredictionType::NextFile,
                        content: to.clone(),
                        confidence: pattern.confidence,
                        source_pattern_id: pattern.id,
                        context: Some(format!("Based on pattern: {} -> {}", from, to)),
                    });
                }
            }

            // Also check if file is part of a related group
            if files.iter().any(|f| f == current_file) {
                for file in files {
                    if file != current_file {
                        predictions.push(Prediction {
                            prediction_type: PredictionType::RelatedFiles,
                            content: file.clone(),
                            confidence: pattern.confidence * 0.8, // Slightly lower confidence for related
                            source_pattern_id: pattern.id,
                            context: Some("Files frequently accessed together".to_string()),
                        });
                    }
                }
            }
        }
    }

    // Sort by confidence and deduplicate
    predictions.sort_by(|a, b| {
        b.confidence
            .partial_cmp(&a.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    predictions.dedup_by(|a, b| a.content == b.content);

    // Limit results
    predictions.truncate(5);

    Ok(predictions)
}

/// Predict co-change suggestions based on change patterns
pub fn predict_co_changes(
    conn: &Connection,
    project_id: i64,
    current_file: &str,
    config: &ProactiveConfig,
) -> Result<Vec<Prediction>> {
    let patterns = get_high_confidence_patterns(conn, project_id, config.min_confidence)?;

    let mut predictions = Vec::new();

    for pattern in patterns {
        if pattern.pattern_type != PatternType::ChangePattern {
            continue;
        }

        if let PatternData::ChangePattern {
            files,
            pattern_subtype,
            outcome_stats,
            ..
        } = &pattern.pattern_data
        {
            if pattern_subtype != "co_change_gap" || files.len() < 2 {
                continue;
            }

            // files[0] = the file that was changed, files[1] = the companion that was missing
            if files[0] == current_file {
                let bad_rate = (outcome_stats.reverted + outcome_stats.follow_up_fix) as f64
                    / outcome_stats.total.max(1) as f64;
                predictions.push(Prediction {
                    prediction_type: PredictionType::CoChangeSuggestion,
                    content: files[1].clone(),
                    confidence: pattern.confidence,
                    source_pattern_id: pattern.id,
                    context: Some(format!(
                        "When {} changes without {}, {:.0}% of diffs had issues ({} observed)",
                        files[0],
                        files[1],
                        bad_rate * 100.0,
                        outcome_stats.total
                    )),
                });
            }
        }
    }

    predictions.sort_by(|a, b| {
        b.confidence
            .partial_cmp(&a.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    predictions.truncate(3);

    Ok(predictions)
}

/// Predict next likely tool based on current tool
pub fn predict_next_tool(
    conn: &Connection,
    project_id: i64,
    current_tool: &str,
    config: &ProactiveConfig,
) -> Result<Vec<Prediction>> {
    let patterns = get_high_confidence_patterns(conn, project_id, config.min_confidence)?;

    let mut predictions = Vec::new();

    for pattern in patterns {
        if pattern.pattern_type != PatternType::ToolChain {
            continue;
        }

        if let PatternData::ToolChain { tools, .. } = &pattern.pattern_data
            && tools.len() >= 2
            && tools[0] == current_tool
        {
            predictions.push(Prediction {
                prediction_type: PredictionType::NextTool,
                content: tools[1].clone(),
                confidence: pattern.confidence,
                source_pattern_id: pattern.id,
                context: Some(format!("Common sequence: {} -> {}", tools[0], tools[1])),
            });
        }
    }

    predictions.sort_by(|a, b| {
        b.confidence
            .partial_cmp(&a.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    predictions.truncate(3);

    Ok(predictions)
}

/// Generate context predictions based on current state
pub fn generate_context_predictions(
    conn: &Connection,
    project_id: i64,
    current_context: &CurrentContext,
    config: &ProactiveConfig,
) -> Result<Vec<Prediction>> {
    let mut all_predictions = Vec::new();

    // Predict based on current file
    if let Some(file) = &current_context.current_file {
        let file_predictions = predict_next_files(conn, project_id, file, config)?;
        all_predictions.extend(file_predictions);

        let co_change_predictions = predict_co_changes(conn, project_id, file, config)?;
        all_predictions.extend(co_change_predictions);
    }

    // Predict based on last tool
    if let Some(tool) = &current_context.last_tool {
        let tool_predictions = predict_next_tool(conn, project_id, tool, config)?;
        all_predictions.extend(tool_predictions);
    }

    // Sort all predictions by confidence
    all_predictions.sort_by(|a, b| {
        b.confidence
            .partial_cmp(&a.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    Ok(all_predictions)
}

/// Current context for making predictions
#[derive(Debug, Clone, Default)]
pub struct CurrentContext {
    pub current_file: Option<String>,
    pub last_tool: Option<String>,
    pub recent_queries: Vec<String>,
    pub session_stage: Option<String>,
}

/// Convert predictions to intervention suggestions
pub fn predictions_to_interventions(
    predictions: &[Prediction],
    config: &ProactiveConfig,
) -> Vec<InterventionSuggestion> {
    predictions
        .iter()
        .filter(|p| p.confidence >= config.min_confidence)
        .map(|p| InterventionSuggestion {
            intervention_type: match p.prediction_type {
                PredictionType::NextFile | PredictionType::RelatedFiles => {
                    InterventionType::ContextPrediction
                }
                PredictionType::NextTool => InterventionType::ContextPrediction,
                PredictionType::RelatedMemories => InterventionType::ContextPrediction,
                PredictionType::ResourceSuggestion => InterventionType::ResourceSuggestion,
                PredictionType::CoChangeSuggestion => InterventionType::ContextPrediction,
            },
            content: p.content.clone(),
            confidence: p.confidence,
            source_pattern_id: p.source_pattern_id,
            context: p.context.clone(),
        })
        .collect()
}

/// A suggestion for a proactive intervention
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterventionSuggestion {
    pub intervention_type: InterventionType,
    pub content: String,
    pub confidence: f64,
    pub source_pattern_id: Option<i64>,
    pub context: Option<String>,
}

impl InterventionSuggestion {
    /// Format as context injection string
    pub fn to_context_string(&self) -> String {
        let confidence_label = if self.confidence >= 0.9 {
            "high confidence"
        } else if self.confidence >= 0.7 {
            "medium confidence"
        } else {
            "suggested"
        };

        match self.intervention_type {
            InterventionType::ContextPrediction => {
                format!(
                    "[Proactive] You may want to look at: {} ({})",
                    self.content, confidence_label
                )
            }
            InterventionType::ResourceSuggestion => {
                format!(
                    "[Suggestion] Related resource: {} ({})",
                    self.content, confidence_label
                )
            }
            InterventionType::SecurityAlert => {
                format!("[Security] Potential issue: {}", self.content)
            }
            InterventionType::BugWarning => {
                format!("[Warning] Potential bug pattern: {}", self.content)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proactive::PatternType;
    use crate::proactive::patterns::{BehaviorPattern, OutcomeStats, PatternData};
    use rusqlite::Connection;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute(
            "CREATE TABLE behavior_patterns (
                id INTEGER PRIMARY KEY,
                project_id INTEGER NOT NULL,
                pattern_type TEXT NOT NULL,
                pattern_key TEXT UNIQUE NOT NULL,
                pattern_data TEXT NOT NULL,
                confidence REAL NOT NULL,
                occurrence_count INTEGER NOT NULL,
                last_triggered_at TEXT,
                first_seen_at TEXT DEFAULT CURRENT_TIMESTAMP,
                updated_at TEXT DEFAULT CURRENT_TIMESTAMP,
                UNIQUE(project_id, pattern_type, pattern_key)
            )",
            [],
        )
        .unwrap();
        conn
    }

    #[test]
    fn test_predict_co_changes_with_matching_pattern() {
        let conn = setup_test_db();
        let project_id = 1;

        // Insert a co-change gap pattern: file_a.rs changed without file_b.rs had 60% bad rate
        let pattern = BehaviorPattern {
            id: None,
            project_id,
            pattern_type: PatternType::ChangePattern,
            pattern_key: "co_change_gap:file_a.rs|file_b.rs".to_string(),
            pattern_data: PatternData::ChangePattern {
                files: vec!["file_a.rs".to_string(), "file_b.rs".to_string()],
                module: None,
                pattern_subtype: "co_change_gap".to_string(),
                outcome_stats: OutcomeStats {
                    total: 10,
                    clean: 4,
                    reverted: 5,
                    follow_up_fix: 1,
                },
                sample_commits: vec![],
            },
            confidence: 0.75,
            occurrence_count: 10,
        };

        crate::proactive::patterns::upsert_pattern(&conn, &pattern).unwrap();

        let config = ProactiveConfig {
            min_confidence: 0.5,
            ..Default::default()
        };

        let predictions = predict_co_changes(&conn, project_id, "file_a.rs", &config).unwrap();

        assert_eq!(predictions.len(), 1);
        assert_eq!(
            predictions[0].prediction_type,
            PredictionType::CoChangeSuggestion
        );
        assert_eq!(predictions[0].content, "file_b.rs");
        assert_eq!(predictions[0].confidence, 0.75);
        assert!(predictions[0].context.is_some());
        assert!(predictions[0].context.as_ref().unwrap().contains("60%"));
    }

    #[test]
    fn test_predict_co_changes_non_matching_file() {
        let conn = setup_test_db();
        let project_id = 1;

        let pattern = BehaviorPattern {
            id: None,
            project_id,
            pattern_type: PatternType::ChangePattern,
            pattern_key: "co_change_gap:file_a.rs|file_b.rs".to_string(),
            pattern_data: PatternData::ChangePattern {
                files: vec!["file_a.rs".to_string(), "file_b.rs".to_string()],
                module: None,
                pattern_subtype: "co_change_gap".to_string(),
                outcome_stats: OutcomeStats {
                    total: 10,
                    clean: 4,
                    reverted: 5,
                    follow_up_fix: 1,
                },
                sample_commits: vec![],
            },
            confidence: 0.75,
            occurrence_count: 10,
        };

        crate::proactive::patterns::upsert_pattern(&conn, &pattern).unwrap();

        let config = ProactiveConfig {
            min_confidence: 0.5,
            ..Default::default()
        };

        // Query with a different file that doesn't match
        let predictions = predict_co_changes(&conn, project_id, "other_file.rs", &config).unwrap();

        assert_eq!(predictions.len(), 0);
    }

    #[test]
    fn test_predict_co_changes_sorted_by_confidence() {
        let conn = setup_test_db();
        let project_id = 1;

        // Insert two patterns with different confidence levels
        let pattern1 = BehaviorPattern {
            id: None,
            project_id,
            pattern_type: PatternType::ChangePattern,
            pattern_key: "co_change_gap:file_a.rs|file_b.rs".to_string(),
            pattern_data: PatternData::ChangePattern {
                files: vec!["file_a.rs".to_string(), "file_b.rs".to_string()],
                module: None,
                pattern_subtype: "co_change_gap".to_string(),
                outcome_stats: OutcomeStats {
                    total: 5,
                    clean: 2,
                    reverted: 3,
                    follow_up_fix: 0,
                },
                sample_commits: vec![],
            },
            confidence: 0.60,
            occurrence_count: 5,
        };

        let pattern2 = BehaviorPattern {
            id: None,
            project_id,
            pattern_type: PatternType::ChangePattern,
            pattern_key: "co_change_gap:file_a.rs|file_c.rs".to_string(),
            pattern_data: PatternData::ChangePattern {
                files: vec!["file_a.rs".to_string(), "file_c.rs".to_string()],
                module: None,
                pattern_subtype: "co_change_gap".to_string(),
                outcome_stats: OutcomeStats {
                    total: 8,
                    clean: 2,
                    reverted: 5,
                    follow_up_fix: 1,
                },
                sample_commits: vec![],
            },
            confidence: 0.85,
            occurrence_count: 8,
        };

        crate::proactive::patterns::upsert_pattern(&conn, &pattern1).unwrap();
        crate::proactive::patterns::upsert_pattern(&conn, &pattern2).unwrap();

        let config = ProactiveConfig {
            min_confidence: 0.5,
            ..Default::default()
        };

        let predictions = predict_co_changes(&conn, project_id, "file_a.rs", &config).unwrap();

        assert_eq!(predictions.len(), 2);
        // Higher confidence should come first
        assert_eq!(predictions[0].content, "file_c.rs");
        assert_eq!(predictions[0].confidence, 0.85);
        assert_eq!(predictions[1].content, "file_b.rs");
        assert_eq!(predictions[1].confidence, 0.60);
    }

    #[test]
    fn test_predictions_to_interventions_includes_co_change() {
        let predictions = vec![Prediction {
            prediction_type: PredictionType::CoChangeSuggestion,
            content: "file_b.rs".to_string(),
            confidence: 0.75,
            source_pattern_id: Some(1),
            context: Some(
                "When file_a.rs changes without file_b.rs, 60% of diffs had issues (10 observed)"
                    .to_string(),
            ),
        }];

        let config = ProactiveConfig {
            min_confidence: 0.5,
            ..Default::default()
        };

        let interventions = predictions_to_interventions(&predictions, &config);

        assert_eq!(interventions.len(), 1);
        assert_eq!(
            interventions[0].intervention_type,
            InterventionType::ContextPrediction
        );
        assert_eq!(interventions[0].content, "file_b.rs");
    }
}
