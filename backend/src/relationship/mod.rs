// src/relationship/mod.rs

pub mod types;
pub mod storage;
pub mod pattern_engine;
pub mod context_loader;
pub mod service;
pub mod facts_service;  // NEW: Facts service

use serde::{Deserialize, Serialize};
use sqlx::FromRow;

// Re-export all types from types module
pub use types::*;

// Re-export service types
pub use storage::RelationshipStorage;
pub use pattern_engine::PatternEngine;
pub use context_loader::ContextLoader;
pub use service::RelationshipService;
pub use facts_service::FactsService;  // NEW: Export facts service

/// User's profile and preferences
/// Maps directly to `user_profile` table
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct UserProfile {
    #[sqlx(default)]
    pub id: i64,
    pub user_id: String,
    
    // Coding Preferences
    pub preferred_languages: Option<String>, // JSON array
    pub coding_style: Option<String>, // JSON object
    pub code_verbosity: Option<String>, // "minimal", "moderate", "verbose"
    pub testing_philosophy: Option<String>, // JSON object
    pub architecture_preferences: Option<String>, // JSON object
    
    // Communication Style
    pub explanation_depth: Option<String>, // "concise", "moderate", "detailed"
    pub conversation_style: Option<String>, // "casual", "professional", "technical"
    pub profanity_comfort: Option<String>, // "none", "mild", "comfortable"
    
    // Tech Context
    pub tech_stack: Option<String>, // JSON array
    pub learning_goals: Option<String>, // JSON array
    
    // Metadata
    #[sqlx(default)]
    pub relationship_started: i64,
    pub last_active: Option<i64>,
    #[sqlx(default)]
    pub total_sessions: i64,
    
    #[sqlx(default)]
    pub created_at: i64,
    #[sqlx(default)]
    pub updated_at: i64,
}

/// Learned patterns about the user's behavior
/// Maps directly to `learned_patterns` table
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct LearnedPattern {
    pub id: String,
    pub user_id: String,
    
    // Pattern Identity
    pub pattern_type: String, // "coding_style", "work_pattern", "communication", etc.
    pub pattern_name: String,
    pub pattern_description: String,
    
    // Evidence
    pub examples: Option<String>, // JSON array of example instances
    
    // Confidence & Validation
    pub confidence: f64,
    #[sqlx(default)]
    pub times_observed: i64,
    #[sqlx(default)]
    pub times_applied: i64,
    
    // Context
    pub applies_when: Option<String>, // When this pattern should be applied
    #[sqlx(default)]
    pub deprecated: i64, // SQLite boolean (0 or 1)
    
    // Timing
    #[sqlx(default)]
    pub first_observed: i64,
    #[sqlx(default)]
    pub last_observed: i64,
    pub last_applied: Option<i64>,
}

/// Simple key-value facts about the user
/// Maps directly to `memory_facts` table
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct MemoryFact {
    pub id: String,
    pub user_id: String,
    
    // Fact Identity
    pub fact_key: String, // e.g., "wife_name", "daughter_birthday"
    pub fact_value: String,
    pub fact_category: String, // "personal", "professional", "technical", etc.
    
    // Confidence
    #[sqlx(default)]
    pub confidence: f64,
    pub source: Option<String>, // Where this came from (maps to context in DB)
    
    // Timing
    #[sqlx(default)]
    pub learned_at: i64,  // Maps to created_at in DB
    pub last_confirmed: Option<i64>,  // Maps to last_referenced in DB
    #[sqlx(default)]
    pub times_referenced: i64,  // Maps to reference_count in DB
}
