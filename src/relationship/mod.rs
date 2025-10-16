// src/relationship/mod.rs

pub mod types;

use serde::{Deserialize, Serialize};
use sqlx::FromRow;

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
    
    // Context
    pub context: Option<String>,
    #[sqlx(default)]
    pub confidence: f64,
    
    // Relevance
    pub last_referenced: Option<i64>,
    #[sqlx(default)]
    pub reference_count: i64,
    #[sqlx(default)]
    pub still_relevant: i64, // SQLite boolean (0 or 1)
    
    // Timing
    #[sqlx(default)]
    pub created_at: i64,
    #[sqlx(default)]
    pub updated_at: i64,
}

impl UserProfile {
    pub fn new(user_id: String) -> Self {
        let now = chrono::Utc::now().timestamp();
        Self {
            id: 0, // Will be set by database
            user_id,
            preferred_languages: None,
            coding_style: None,
            code_verbosity: None,
            testing_philosophy: None,
            architecture_preferences: None,
            explanation_depth: None,
            conversation_style: None,
            profanity_comfort: None,
            tech_stack: None,
            learning_goals: None,
            relationship_started: now,
            last_active: Some(now),
            total_sessions: 0,
            created_at: now,
            updated_at: now,
        }
    }
    
    pub fn update_activity(&mut self) {
        self.last_active = Some(chrono::Utc::now().timestamp());
        self.total_sessions += 1;
        self.updated_at = chrono::Utc::now().timestamp();
    }
}

impl LearnedPattern {
    pub fn new(
        user_id: String,
        pattern_type: String,
        pattern_name: String,
        pattern_description: String,
    ) -> Self {
        use uuid::Uuid;
        let now = chrono::Utc::now().timestamp();
        
        Self {
            id: Uuid::new_v4().to_string(),
            user_id,
            pattern_type,
            pattern_name,
            pattern_description,
            examples: None,
            confidence: 0.5, // Start at medium confidence
            times_observed: 1,
            times_applied: 0,
            applies_when: None,
            deprecated: 0,
            first_observed: now,
            last_observed: now,
            last_applied: None,
        }
    }
    
    pub fn observe(&mut self) {
        self.times_observed += 1;
        self.last_observed = chrono::Utc::now().timestamp();
        
        // Increase confidence with more observations (cap at 0.95)
        self.confidence = (self.confidence + 0.05).min(0.95);
    }
    
    pub fn apply(&mut self) {
        self.times_applied += 1;
        self.last_applied = Some(chrono::Utc::now().timestamp());
    }
    
    pub fn decrease_confidence(&mut self) {
        self.confidence = (self.confidence - 0.1).max(0.1);
    }
    
    pub fn deprecate(&mut self) {
        self.deprecated = 1;
    }
}

impl MemoryFact {
    pub fn new(
        user_id: String,
        fact_category: String,
        fact_key: String,
        fact_value: String,
    ) -> Self {
        use uuid::Uuid;
        let now = chrono::Utc::now().timestamp();
        
        Self {
            id: Uuid::new_v4().to_string(),
            user_id,
            fact_key,
            fact_value,
            fact_category,
            context: None,
            confidence: 0.8, // Facts start with high confidence
            last_referenced: None,
            reference_count: 0,
            still_relevant: 1,
            created_at: now,
            updated_at: now,
        }
    }
    
    pub fn update_value(&mut self, new_value: String) {
        self.fact_value = new_value;
        self.updated_at = chrono::Utc::now().timestamp();
        // Reset confidence when updating
        self.confidence = 0.8;
    }
    
    pub fn reference(&mut self) {
        self.reference_count += 1;
        self.last_referenced = Some(chrono::Utc::now().timestamp());
    }
    
    pub fn mark_irrelevant(&mut self) {
        self.still_relevant = 0;
        self.updated_at = chrono::Utc::now().timestamp();
    }
}
