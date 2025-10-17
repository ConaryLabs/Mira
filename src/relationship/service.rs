// src/relationship/service.rs
use sqlx::SqlitePool;
use std::sync::Arc;
use anyhow::Result;
use tracing::{info, warn, debug};
use crate::relationship::{
    storage::RelationshipStorage,
    pattern_engine::PatternEngine,
    context_loader::ContextLoader,
};

/// Main relationship service - coordinates all relationship functionality
pub struct RelationshipService {
    pub storage: Arc<RelationshipStorage>,
    pub pattern_engine: Arc<PatternEngine>,
    pub context_loader: Arc<ContextLoader>,
}

impl RelationshipService {
    /// Create a new relationship service
    pub fn new(pool: Arc<SqlitePool>) -> Self {
        info!("Initializing RelationshipService");
        
        let storage = Arc::new(RelationshipStorage::new(pool));
        let pattern_engine = Arc::new(PatternEngine::new(storage.clone()));
        let context_loader = Arc::new(ContextLoader::new(
            storage.clone(),
            pattern_engine.clone(),
        ));
        
        Self {
            storage,
            pattern_engine,
            context_loader,
        }
    }
    
    /// Get storage layer
    pub fn storage(&self) -> Arc<RelationshipStorage> {
        self.storage.clone()
    }
    
    /// Get pattern engine
    pub fn pattern_engine(&self) -> Arc<PatternEngine> {
        self.pattern_engine.clone()
    }
    
    /// Get context loader
    pub fn context_loader(&self) -> Arc<ContextLoader> {
        self.context_loader.clone()
    }
    
    /// Process relationship updates from LLM response
    /// Parses relationship_impact JSON and updates profile/patterns/facts
    pub async fn process_llm_updates(
        &self,
        user_id: &str,
        relationship_impact: Option<&str>,
    ) -> Result<()> {
        // If no relationship_impact, nothing to process
        let Some(impact_str) = relationship_impact else {
            return Ok(());
        };
        
        if impact_str.trim().is_empty() {
            return Ok(());
        }
        
        // Try to parse as structured JSON
        let impact_json = match serde_json::from_str::<serde_json::Value>(impact_str) {
            Ok(json) => json,
            Err(_) => {
                // Not JSON - just log as plain text observation
                debug!("Relationship impact is plain text: {}", impact_str);
                return Ok(());
            }
        };
        
        debug!("Processing relationship updates for user {}", user_id);
        
        // Process profile changes
        if let Some(profile_changes) = impact_json.get("profile_changes").and_then(|v| v.as_object()) {
            self.process_profile_changes(user_id, profile_changes).await?;
        }
        
        // Process new patterns
        if let Some(patterns_arr) = impact_json.get("new_patterns").and_then(|v| v.as_array()) {
            self.process_new_patterns(user_id, patterns_arr).await?;
        }
        
        // Process new facts
        if let Some(facts_arr) = impact_json.get("new_facts").and_then(|v| v.as_array()) {
            self.process_new_facts(user_id, facts_arr).await?;
        }
        
        Ok(())
    }
    
    /// Process profile changes from LLM
    async fn process_profile_changes(
        &self,
        user_id: &str,
        changes: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<()> {
        // Get current profile
        let mut profile = self.storage.get_or_create_profile(user_id).await?;
        
        // Update profile fields based on changes
        if let Some(langs) = changes.get("preferred_languages").and_then(|v| v.as_array()) {
            let langs_json = serde_json::to_string(langs)?;
            profile.preferred_languages = Some(langs_json);
        }
        
        if let Some(style) = changes.get("coding_style").and_then(|v| v.as_str()) {
            profile.coding_style = Some(style.to_string());
        }
        
        if let Some(verbosity) = changes.get("code_verbosity").and_then(|v| v.as_str()) {
            profile.code_verbosity = Some(verbosity.to_string());
        }
        
        if let Some(testing) = changes.get("testing_philosophy").and_then(|v| v.as_str()) {
            profile.testing_philosophy = Some(testing.to_string());
        }
        
        if let Some(arch) = changes.get("architecture_preferences").and_then(|v| v.as_str()) {
            profile.architecture_preferences = Some(arch.to_string());
        }
        
        if let Some(depth) = changes.get("explanation_depth").and_then(|v| v.as_str()) {
            profile.explanation_depth = Some(depth.to_string());
        }
        
        if let Some(conv) = changes.get("conversation_style").and_then(|v| v.as_str()) {
            profile.conversation_style = Some(conv.to_string());
        }
        
        // Update profile in database
        profile.updated_at = chrono::Utc::now().timestamp();
        self.storage.update_profile(&profile).await?;
        
        info!("Updated {} profile fields for user {}", changes.len(), user_id);
        Ok(())
    }
    
    /// Process new patterns from LLM
    async fn process_new_patterns(
        &self,
        user_id: &str,
        patterns: &[serde_json::Value],
    ) -> Result<()> {
        for pattern_val in patterns {
            let pattern_type = pattern_val.get("pattern_type")
                .and_then(|v| v.as_str())
                .unwrap_or("general");
            let pattern_name = pattern_val.get("pattern_name")
                .and_then(|v| v.as_str())
                .unwrap_or("unnamed");
            let description = pattern_val.get("pattern_description")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let confidence = pattern_val.get("confidence")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.7);
            let applies_when = pattern_val.get("applies_when")
                .and_then(|v| v.as_str());
            
            // Create LearnedPattern struct
            let now = chrono::Utc::now().timestamp();
            let pattern = crate::relationship::LearnedPattern {
                id: uuid::Uuid::new_v4().to_string(),
                user_id: user_id.to_string(),
                pattern_type: pattern_type.to_string(),
                pattern_name: pattern_name.to_string(),
                pattern_description: description.to_string(),
                examples: None,
                confidence,
                times_observed: 1,
                times_applied: 0,
                applies_when: applies_when.map(|s| s.to_string()),
                deprecated: 0,
                first_observed: now,
                last_observed: now,
                last_applied: None,
            };
            
            match self.storage.upsert_pattern(&pattern).await {
                Ok(_) => info!("Recorded pattern '{}' for user {}", pattern_name, user_id),
                Err(e) => warn!("Failed to record pattern '{}': {}", pattern_name, e),
            }
        }
        
        Ok(())
    }
    
    /// Process new facts from LLM
    async fn process_new_facts(
        &self,
        user_id: &str,
        facts: &[serde_json::Value],
    ) -> Result<()> {
        for fact_val in facts {
            let fact_key = fact_val.get("fact_key")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let fact_value = fact_val.get("fact_value")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let fact_category = fact_val.get("fact_category")
                .and_then(|v| v.as_str())
                .unwrap_or("general");
            let confidence = fact_val.get("confidence")
                .and_then(|v| v.as_f64())
                .unwrap_or(1.0);
            let context = fact_val.get("context")
                .and_then(|v| v.as_str());
            
            // Create MemoryFact struct
            let now = chrono::Utc::now().timestamp();
            let fact = crate::relationship::MemoryFact {
                id: uuid::Uuid::new_v4().to_string(),
                user_id: user_id.to_string(),
                fact_key: fact_key.to_string(),
                fact_value: fact_value.to_string(),
                fact_category: fact_category.to_string(),
                confidence,
                source: context.map(|s| s.to_string()),
                learned_at: now,
                last_confirmed: Some(now),
                times_referenced: 0,
            };
            
            match self.storage.upsert_fact(&fact).await {
                Ok(_) => info!("Stored fact '{}' for user {}", fact_key, user_id),
                Err(e) => warn!("Failed to store fact '{}': {}", fact_key, e),
            }
        }
        
        Ok(())
    }
}
