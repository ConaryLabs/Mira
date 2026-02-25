// crates/mira-server/src/proactive/mod.rs
// Proactive Intelligence Engine - anticipates developer needs through pattern recognition

pub mod background;
pub mod behavior;
pub mod feedback;
pub mod interventions;
pub mod patterns;
pub mod predictor;

use serde::{Deserialize, Serialize};

/// Event types tracked in the behavior log
#[derive(
    Debug, Clone, Serialize, Deserialize, PartialEq, strum::IntoStaticStr, strum::EnumString,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum EventType {
    FileAccess,
    ToolUse,
    ToolFailure,
    Query,
    ContextSwitch,
    GoalUpdate,
    MemoryRecall,
}

impl EventType {
    pub fn as_str(&self) -> &'static str {
        self.into()
    }
}

/// Pattern types for behavior analysis
#[derive(
    Debug, Clone, Serialize, Deserialize, PartialEq, strum::IntoStaticStr, strum::EnumString,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum PatternType {
    FileSequence,  // Files accessed together or in sequence
    ToolChain,     // Tools used in sequence
    SessionFlow,   // Common session patterns
    QueryPattern,  // Common query patterns
    ChangePattern, // Recurring code change patterns correlated with outcomes
}

impl PatternType {
    pub fn as_str(&self) -> &'static str {
        self.into()
    }
}

/// Intervention types
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, strum::IntoStaticStr)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum InterventionType {
    ContextPrediction,  // Predict what context the user will need
    SecurityAlert,      // Warn about security issues in code
    BugWarning,         // Warn about potential bugs
    ResourceSuggestion, // Suggest related resources/docs
}

impl InterventionType {
    pub fn as_str(&self) -> &'static str {
        self.into()
    }
}

/// User response to an intervention
#[derive(
    Debug, Clone, Serialize, Deserialize, PartialEq, strum::IntoStaticStr, strum::EnumString,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum UserResponse {
    Accepted,  // User explicitly accepted/used the suggestion
    Dismissed, // User explicitly dismissed
    ActedUpon, // User took related action without explicit acceptance
    Ignored,   // No response within timeout
}

impl UserResponse {
    pub fn as_str(&self) -> &'static str {
        self.into()
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
            enabled: false,
            cooldown_seconds: 300, // 5 minutes
        }
    }
}

/// Get proactive config for a user/project.
///
/// Memory-based opt-in removed (Phase 4). Returns default config (disabled).
pub fn get_proactive_config(
    _conn: &rusqlite::Connection,
    _user_id: Option<&str>,
    _project_id: i64,
) -> ProactiveConfig {
    ProactiveConfig::default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    // ═══════════════════════════════════════════════════════════════════════════════
    // EventType Tests
    // ═══════════════════════════════════════════════════════════════════════════════

    #[test]
    fn test_event_type_as_str() {
        assert_eq!(EventType::FileAccess.as_str(), "file_access");
        assert_eq!(EventType::ToolUse.as_str(), "tool_use");
        assert_eq!(EventType::ToolFailure.as_str(), "tool_failure");
        assert_eq!(EventType::Query.as_str(), "query");
        assert_eq!(EventType::ContextSwitch.as_str(), "context_switch");
        assert_eq!(EventType::GoalUpdate.as_str(), "goal_update");
        assert_eq!(EventType::MemoryRecall.as_str(), "memory_recall");
    }

    #[test]
    fn test_event_type_from_str() {
        assert_eq!("file_access".parse(), Ok(EventType::FileAccess));
        assert_eq!("tool_use".parse(), Ok(EventType::ToolUse));
        assert_eq!("tool_failure".parse(), Ok(EventType::ToolFailure));
        assert_eq!("query".parse(), Ok(EventType::Query));
        assert_eq!("context_switch".parse(), Ok(EventType::ContextSwitch));
        assert_eq!("goal_update".parse(), Ok(EventType::GoalUpdate));
        assert_eq!("memory_recall".parse(), Ok(EventType::MemoryRecall));
        assert!("invalid".parse::<EventType>().is_err());
        assert!("".parse::<EventType>().is_err());
    }

    #[test]
    fn test_event_type_roundtrip() {
        let events = [
            EventType::FileAccess,
            EventType::ToolUse,
            EventType::ToolFailure,
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
                Ok(event.clone()),
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
        assert_eq!(PatternType::ChangePattern.as_str(), "change_pattern");
    }

    #[test]
    fn test_pattern_type_from_str() {
        assert_eq!("file_sequence".parse(), Ok(PatternType::FileSequence));
        assert_eq!("tool_chain".parse(), Ok(PatternType::ToolChain));
        assert_eq!("session_flow".parse(), Ok(PatternType::SessionFlow));
        assert_eq!("query_pattern".parse(), Ok(PatternType::QueryPattern));
        assert_eq!("change_pattern".parse(), Ok(PatternType::ChangePattern));
        assert!("invalid".parse::<PatternType>().is_err());
    }

    #[test]
    fn test_pattern_type_roundtrip() {
        let patterns = [
            PatternType::FileSequence,
            PatternType::ToolChain,
            PatternType::SessionFlow,
            PatternType::QueryPattern,
            PatternType::ChangePattern,
        ];
        for pattern in &patterns {
            let s = pattern.as_str();
            let parsed = PatternType::from_str(s);
            assert_eq!(
                parsed,
                Ok(pattern.clone()),
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
        assert_eq!("accepted".parse(), Ok(UserResponse::Accepted));
        assert_eq!("dismissed".parse(), Ok(UserResponse::Dismissed));
        assert_eq!("acted_upon".parse(), Ok(UserResponse::ActedUpon));
        assert_eq!("ignored".parse(), Ok(UserResponse::Ignored));
        assert!("invalid".parse::<UserResponse>().is_err());
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
        assert!(!config.enabled);
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
