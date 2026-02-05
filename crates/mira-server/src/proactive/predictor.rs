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
                if from == current_file && pattern.confidence >= config.min_confidence {
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
            if files.contains(&current_file.to_string()) {
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
