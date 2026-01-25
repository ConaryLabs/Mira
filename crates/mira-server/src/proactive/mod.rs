// crates/mira-server/src/proactive/mod.rs
// Proactive Intelligence Engine - anticipates developer needs through pattern recognition

pub mod behavior;
pub mod feedback;
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
    FileSequence,   // Files accessed together or in sequence
    ToolChain,      // Tools used in sequence
    SessionFlow,    // Common session patterns
    QueryPattern,   // Common query patterns
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
    Accepted,   // User explicitly accepted/used the suggestion
    Dismissed,  // User explicitly dismissed
    ActedUpon,  // User took related action without explicit acceptance
    Ignored,    // No response within timeout
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
pub fn get_proactive_config(conn: &Connection, user_id: Option<&str>, project_id: i64) -> Result<ProactiveConfig> {
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
