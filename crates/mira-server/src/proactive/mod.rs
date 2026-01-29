// crates/mira-server/src/proactive/mod.rs
// Proactive Intelligence Engine - anticipates developer needs through pattern recognition

pub mod behavior;
pub mod feedback;
pub mod interventions;
pub mod patterns;
pub mod predictor;

use anyhow::Result;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

/// Event types tracked in the behavior log
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    FileAccess,
    ToolUse,
    Query,
    ContextSwitch,
    GoalUpdate,
    MemoryRecall,
}

impl EventType {
    pub fn as_str(&self) -> &'static str {
        match self {
            EventType::FileAccess => "file_access",
            EventType::ToolUse => "tool_use",
            EventType::Query => "query",
            EventType::ContextSwitch => "context_switch",
            EventType::GoalUpdate => "goal_update",
            EventType::MemoryRecall => "memory_recall",
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "file_access" => Some(EventType::FileAccess),
            "tool_use" => Some(EventType::ToolUse),
            "query" => Some(EventType::Query),
            "context_switch" => Some(EventType::ContextSwitch),
            "goal_update" => Some(EventType::GoalUpdate),
            "memory_recall" => Some(EventType::MemoryRecall),
            _ => None,
        }
    }
}

/// Pattern types for behavior analysis
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PatternType {
    FileSequence, // Files accessed together or in sequence
    ToolChain,    // Tools used in sequence
    SessionFlow,  // Common session patterns
    QueryPattern, // Common query patterns
}

impl PatternType {
    pub fn as_str(&self) -> &'static str {
        match self {
            PatternType::FileSequence => "file_sequence",
            PatternType::ToolChain => "tool_chain",
            PatternType::SessionFlow => "session_flow",
            PatternType::QueryPattern => "query_pattern",
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "file_sequence" => Some(PatternType::FileSequence),
            "tool_chain" => Some(PatternType::ToolChain),
            "session_flow" => Some(PatternType::SessionFlow),
            "query_pattern" => Some(PatternType::QueryPattern),
            _ => None,
        }
    }
}

/// Intervention types
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum InterventionType {
    ContextPrediction,  // Predict what context the user will need
    SecurityAlert,      // Warn about security issues in code
    BugWarning,         // Warn about potential bugs
    ResourceSuggestion, // Suggest related resources/docs
}

impl InterventionType {
    pub fn as_str(&self) -> &'static str {
        match self {
            InterventionType::ContextPrediction => "context_prediction",
            InterventionType::SecurityAlert => "security_alert",
            InterventionType::BugWarning => "bug_warning",
            InterventionType::ResourceSuggestion => "resource_suggestion",
        }
    }
}

/// User response to an intervention
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum UserResponse {
    Accepted,  // User explicitly accepted/used the suggestion
    Dismissed, // User explicitly dismissed
    ActedUpon, // User took related action without explicit acceptance
    Ignored,   // No response within timeout
}

impl UserResponse {
    pub fn as_str(&self) -> &'static str {
        match self {
            UserResponse::Accepted => "accepted",
            UserResponse::Dismissed => "dismissed",
            UserResponse::ActedUpon => "acted_upon",
            UserResponse::Ignored => "ignored",
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "accepted" => Some(UserResponse::Accepted),
            "dismissed" => Some(UserResponse::Dismissed),
            "acted_upon" => Some(UserResponse::ActedUpon),
            "ignored" => Some(UserResponse::Ignored),
            _ => None,
        }
    }

    /// Effectiveness multiplier for learning
    pub fn effectiveness_multiplier(&self) -> f64 {
        match self {
            UserResponse::Accepted => 1.0,
            UserResponse::ActedUpon => 0.8,
            UserResponse::Ignored => 0.0,
            UserResponse::Dismissed => -0.5,
        }
    }
}

/// Configuration for proactive intelligence behavior
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProactiveConfig {
    /// Minimum confidence threshold for interventions (0.0-1.0)
    pub min_confidence: f64,
    /// Maximum interventions per hour
    pub max_interventions_per_hour: u32,
    /// Whether to enable proactive features
    pub enabled: bool,
    /// Minimum time between interventions (seconds)
    pub cooldown_seconds: u32,
}

impl Default for ProactiveConfig {
    fn default() -> Self {
        Self {
            min_confidence: 0.7,
            max_interventions_per_hour: 10,
            enabled: true,
            cooldown_seconds: 300, // 5 minutes
        }
    }
}

/// Get proactive config for a user/project
pub fn get_proactive_config(
    conn: &Connection,
    user_id: Option<&str>,
    project_id: i64,
) -> Result<ProactiveConfig> {
    let mut config = ProactiveConfig::default();

    // Load user preferences if set
    let sql = r#"
        SELECT preference_key, preference_value
        FROM proactive_preferences
        WHERE (user_id = ? OR user_id IS NULL)
          AND (project_id = ? OR project_id IS NULL)
        ORDER BY user_id DESC, project_id DESC
    "#;

    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map([user_id.unwrap_or(""), &project_id.to_string()], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;

    for row in rows.flatten() {
        let (key, value) = row;
        match key.as_str() {
            "min_confidence" => {
                if let Ok(v) = value.parse::<f64>() {
                    config.min_confidence = v;
                }
            }
            "max_interventions_per_hour" => {
                if let Ok(v) = value.parse::<u32>() {
                    config.max_interventions_per_hour = v;
                }
            }
            "enabled" => {
                config.enabled = value == "true" || value == "1";
            }
            "cooldown_seconds" => {
                if let Ok(v) = value.parse::<u32>() {
                    config.cooldown_seconds = v;
                }
            }
            _ => {}
        }
    }

    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ═══════════════════════════════════════════════════════════════════════════════
    // EventType Tests
    // ═══════════════════════════════════════════════════════════════════════════════

    #[test]
    fn test_event_type_as_str() {
        assert_eq!(EventType::FileAccess.as_str(), "file_access");
        assert_eq!(EventType::ToolUse.as_str(), "tool_use");
        assert_eq!(EventType::Query.as_str(), "query");
        assert_eq!(EventType::ContextSwitch.as_str(), "context_switch");
        assert_eq!(EventType::GoalUpdate.as_str(), "goal_update");
        assert_eq!(EventType::MemoryRecall.as_str(), "memory_recall");
    }

    #[test]
    fn test_event_type_from_str() {
        assert_eq!(
            EventType::from_str("file_access"),
            Some(EventType::FileAccess)
        );
        assert_eq!(EventType::from_str("tool_use"), Some(EventType::ToolUse));
        assert_eq!(EventType::from_str("query"), Some(EventType::Query));
        assert_eq!(
            EventType::from_str("context_switch"),
            Some(EventType::ContextSwitch)
        );
        assert_eq!(
            EventType::from_str("goal_update"),
            Some(EventType::GoalUpdate)
        );
        assert_eq!(
            EventType::from_str("memory_recall"),
            Some(EventType::MemoryRecall)
        );
        assert_eq!(EventType::from_str("invalid"), None);
        assert_eq!(EventType::from_str(""), None);
    }

    #[test]
    fn test_event_type_roundtrip() {
        let events = [
            EventType::FileAccess,
            EventType::ToolUse,
            EventType::Query,
            EventType::ContextSwitch,
            EventType::GoalUpdate,
            EventType::MemoryRecall,
        ];
        for event in &events {
            let s = event.as_str();
            let parsed = EventType::from_str(s);
            assert_eq!(
                parsed,
                Some(event.clone()),
                "Roundtrip failed for {:?}",
                event
            );
        }
    }

    #[test]
    fn test_event_type_serialization() {
        let event = EventType::ToolUse;
        let json = serde_json::to_string(&event).unwrap();
        assert_eq!(json, "\"tool_use\"");

        let parsed: EventType = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, event);
    }

    // ═══════════════════════════════════════════════════════════════════════════════
    // PatternType Tests
    // ═══════════════════════════════════════════════════════════════════════════════

    #[test]
    fn test_pattern_type_as_str() {
        assert_eq!(PatternType::FileSequence.as_str(), "file_sequence");
        assert_eq!(PatternType::ToolChain.as_str(), "tool_chain");
        assert_eq!(PatternType::SessionFlow.as_str(), "session_flow");
        assert_eq!(PatternType::QueryPattern.as_str(), "query_pattern");
    }

    #[test]
    fn test_pattern_type_from_str() {
        assert_eq!(
            PatternType::from_str("file_sequence"),
            Some(PatternType::FileSequence)
        );
        assert_eq!(
            PatternType::from_str("tool_chain"),
            Some(PatternType::ToolChain)
        );
        assert_eq!(
            PatternType::from_str("session_flow"),
            Some(PatternType::SessionFlow)
        );
        assert_eq!(
            PatternType::from_str("query_pattern"),
            Some(PatternType::QueryPattern)
        );
        assert_eq!(PatternType::from_str("invalid"), None);
    }

    #[test]
    fn test_pattern_type_roundtrip() {
        let patterns = [
            PatternType::FileSequence,
            PatternType::ToolChain,
            PatternType::SessionFlow,
            PatternType::QueryPattern,
        ];
        for pattern in &patterns {
            let s = pattern.as_str();
            let parsed = PatternType::from_str(s);
            assert_eq!(
                parsed,
                Some(pattern.clone()),
                "Roundtrip failed for {:?}",
                pattern
            );
        }
    }

    // ═══════════════════════════════════════════════════════════════════════════════
    // InterventionType Tests
    // ═══════════════════════════════════════════════════════════════════════════════

    #[test]
    fn test_intervention_type_as_str() {
        assert_eq!(
            InterventionType::ContextPrediction.as_str(),
            "context_prediction"
        );
        assert_eq!(InterventionType::SecurityAlert.as_str(), "security_alert");
        assert_eq!(InterventionType::BugWarning.as_str(), "bug_warning");
        assert_eq!(
            InterventionType::ResourceSuggestion.as_str(),
            "resource_suggestion"
        );
    }

    #[test]
    fn test_intervention_type_serialization() {
        let intervention = InterventionType::SecurityAlert;
        let json = serde_json::to_string(&intervention).unwrap();
        assert_eq!(json, "\"security_alert\"");

        let parsed: InterventionType = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, intervention);
    }

    // ═══════════════════════════════════════════════════════════════════════════════
    // UserResponse Tests
    // ═══════════════════════════════════════════════════════════════════════════════

    #[test]
    fn test_user_response_as_str() {
        assert_eq!(UserResponse::Accepted.as_str(), "accepted");
        assert_eq!(UserResponse::Dismissed.as_str(), "dismissed");
        assert_eq!(UserResponse::ActedUpon.as_str(), "acted_upon");
        assert_eq!(UserResponse::Ignored.as_str(), "ignored");
    }

    #[test]
    fn test_user_response_from_str() {
        assert_eq!(
            UserResponse::from_str("accepted"),
            Some(UserResponse::Accepted)
        );
        assert_eq!(
            UserResponse::from_str("dismissed"),
            Some(UserResponse::Dismissed)
        );
        assert_eq!(
            UserResponse::from_str("acted_upon"),
            Some(UserResponse::ActedUpon)
        );
        assert_eq!(
            UserResponse::from_str("ignored"),
            Some(UserResponse::Ignored)
        );
        assert_eq!(UserResponse::from_str("invalid"), None);
    }

    #[test]
    fn test_user_response_effectiveness_multiplier() {
        assert_eq!(UserResponse::Accepted.effectiveness_multiplier(), 1.0);
        assert_eq!(UserResponse::ActedUpon.effectiveness_multiplier(), 0.8);
        assert_eq!(UserResponse::Ignored.effectiveness_multiplier(), 0.0);
        assert_eq!(UserResponse::Dismissed.effectiveness_multiplier(), -0.5);
    }

    #[test]
    fn test_user_response_multiplier_ordering() {
        // Accepted should be most positive
        assert!(
            UserResponse::Accepted.effectiveness_multiplier()
                > UserResponse::ActedUpon.effectiveness_multiplier()
        );
        // ActedUpon should be positive
        assert!(
            UserResponse::ActedUpon.effectiveness_multiplier()
                > UserResponse::Ignored.effectiveness_multiplier()
        );
        // Ignored should be neutral
        assert!(
            UserResponse::Ignored.effectiveness_multiplier()
                > UserResponse::Dismissed.effectiveness_multiplier()
        );
        // Dismissed should be negative
        assert!(UserResponse::Dismissed.effectiveness_multiplier() < 0.0);
    }

    // ═══════════════════════════════════════════════════════════════════════════════
    // ProactiveConfig Tests
    // ═══════════════════════════════════════════════════════════════════════════════

    #[test]
    fn test_proactive_config_default() {
        let config = ProactiveConfig::default();
        assert_eq!(config.min_confidence, 0.7);
        assert_eq!(config.max_interventions_per_hour, 10);
        assert!(config.enabled);
        assert_eq!(config.cooldown_seconds, 300);
    }

    #[test]
    fn test_proactive_config_serialization() {
        let config = ProactiveConfig {
            min_confidence: 0.8,
            max_interventions_per_hour: 5,
            enabled: false,
            cooldown_seconds: 600,
        };

        let json = serde_json::to_string(&config).unwrap();
        let parsed: ProactiveConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.min_confidence, 0.8);
        assert_eq!(parsed.max_interventions_per_hour, 5);
        assert!(!parsed.enabled);
        assert_eq!(parsed.cooldown_seconds, 600);
    }
}
