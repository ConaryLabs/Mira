// src/relationship/types.rs

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Pattern types (stored as strings in DB)
pub mod pattern_types {
    pub const CODING_STYLE: &str = "coding_style";
    pub const WORK_PATTERN: &str = "work_pattern";
    pub const COMMUNICATION: &str = "communication";
    pub const PREFERENCE: &str = "preference";
    pub const TOPIC_INTEREST: &str = "topic_interest";
    pub const PROBLEM_SOLVING: &str = "problem_solving";
}

/// Fact categories (stored as strings in DB)
pub mod fact_categories {
    pub const PERSONAL: &str = "personal";
    pub const PROFESSIONAL: &str = "professional";
    pub const TECHNICAL: &str = "technical";
    pub const PREFERENCE: &str = "preference";
    pub const BIOGRAPHICAL: &str = "biographical";
}

/// Code verbosity levels
pub mod code_verbosity {
    pub const MINIMAL: &str = "minimal";
    pub const MODERATE: &str = "moderate";
    pub const VERBOSE: &str = "verbose";
}

/// Explanation depth levels
pub mod explanation_depth {
    pub const CONCISE: &str = "concise";
    pub const MODERATE: &str = "moderate";
    pub const DETAILED: &str = "detailed";
}

/// Conversation styles
pub mod conversation_style {
    pub const CASUAL: &str = "casual";
    pub const PROFESSIONAL: &str = "professional";
    pub const TECHNICAL: &str = "technical";
}

/// Profanity comfort levels
pub mod profanity_comfort {
    pub const NONE: &str = "none";
    pub const MILD: &str = "mild";
    pub const COMFORTABLE: &str = "comfortable";
}

/// Coding style preferences (stored as JSON in DB)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodingStylePreferences {
    pub indentation: Option<String>, // "spaces_2", "spaces_4", "tabs"
    pub line_length: Option<usize>,
    pub naming_convention: Option<HashMap<String, String>>, // By language
    pub comment_style: Option<String>,                      // "verbose", "minimal", "docstrings"
    pub error_handling: Option<String>,                     // "result_type", "exceptions", "panic"
    pub test_coverage: Option<String>,                      // "high", "medium", "low"
}

/// Testing philosophy (stored as JSON in DB)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestingPhilosophy {
    pub test_first: bool,
    pub coverage_target: Option<f32>,
    pub integration_vs_unit: Option<String>, // "integration_heavy", "balanced", "unit_heavy"
    pub mocking_preference: Option<String>,  // "extensive", "minimal", "situational"
}

/// Architecture preferences (stored as JSON in DB)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchitecturePreferences {
    pub patterns: Vec<String>, // e.g., ["mvc", "clean_architecture", "microservices"]
    pub separation_of_concerns: Option<String>, // "strict", "pragmatic", "flexible"
    pub dependency_management: Option<String>,
    pub file_organization: Option<String>,
}

/// Coding style pattern details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodingStylePattern {
    pub language: String,
    pub specific_preferences: HashMap<String, String>,
    pub common_patterns: Vec<String>,
    pub avoid_patterns: Vec<String>,
}

/// Work pattern details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkPattern {
    pub typical_session_duration_minutes: Option<u32>,
    pub peak_productivity_hours: Vec<u8>,
    pub preferred_break_frequency: Option<String>,
    pub multitasking_style: Option<String>,
}

/// Communication pattern details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommunicationPattern {
    pub verbosity: String,
    pub emoji_usage: String,
    pub formality_level: String,
    pub question_frequency: String,
}

/// Topic interest details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopicInterest {
    pub topic: String,
    pub interest_level: String,  // "high", "medium", "low"
    pub expertise_level: String, // "expert", "intermediate", "beginner", "unknown"
    pub discussion_count: u32,
}

/// Problem solving approach details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProblemSolvingPattern {
    pub approach: String, // "top_down", "bottom_up", "exploratory", "methodical"
    pub debugging_style: String, // "print_statements", "debugger", "test_driven", "reasoning"
    pub research_preference: String, // "documentation", "examples", "experimentation", "ask_for_help"
}

/// Context loaded for a session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionContext {
    pub user_profile: Option<UserProfileContext>,
    pub relevant_patterns: Vec<PatternContext>,
    pub relevant_facts: Vec<FactContext>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserProfileContext {
    pub user_id: String,
    pub preferred_languages: Option<Vec<String>>,
    pub conversation_style: Option<String>,
    pub code_verbosity: Option<String>,
    pub explanation_depth: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatternContext {
    pub pattern_type: String,
    pub pattern_name: String,
    pub pattern_description: String,
    pub confidence: f64,
    pub applies_when: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FactContext {
    pub fact_category: String,
    pub fact_key: String,
    pub fact_value: String,
    pub confidence: f64,
}

/// Pattern update from conversation analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatternUpdate {
    pub pattern_type: String,
    pub pattern_name: String,
    pub pattern_description: String,
    pub confidence_delta: f64,
    pub reason: String,
    pub example: Option<String>,
}

/// Fact update from conversation analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FactUpdate {
    pub fact_category: String,
    pub fact_key: String,
    pub fact_value: String,
    pub context: Option<String>,
    pub confidence: f64,
}

/// Analysis of a conversation for pattern/fact extraction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationAnalysis {
    pub conversation_id: String,
    pub pattern_updates: Vec<PatternUpdate>,
    pub fact_updates: Vec<FactUpdate>,
    pub profile_updates: Option<ProfileUpdates>,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileUpdates {
    pub preferred_languages: Option<Vec<String>>,
    pub conversation_style: Option<String>,
    pub code_verbosity: Option<String>,
    pub explanation_depth: Option<String>,
}
