// src/relationship/context_loader.rs

use anyhow::Result;
use std::sync::Arc;
use tracing::{debug, info};

use crate::relationship::pattern_engine::PatternEngine;
use crate::relationship::storage::RelationshipStorage;
use crate::relationship::types::*;

/// Context loader for pulling relationship data at session start
pub struct ContextLoader {
    storage: Arc<RelationshipStorage>,
    pattern_engine: Arc<PatternEngine>,
}

impl ContextLoader {
    pub fn new(storage: Arc<RelationshipStorage>, pattern_engine: Arc<PatternEngine>) -> Self {
        Self {
            storage,
            pattern_engine,
        }
    }

    /// Load full relationship context for a user
    pub async fn load_context(&self, user_id: &str) -> Result<RelationshipContext> {
        info!("Loading relationship context for user: {}", user_id);

        // Load profile (or create if doesn't exist)
        let profile = self.storage.get_or_create_profile(user_id).await?;

        // Load patterns (coding style, work patterns, communication style)
        let coding_patterns = self
            .pattern_engine
            .get_pattern_context(
                user_id,
                &[pattern_types::CODING_STYLE, pattern_types::PROBLEM_SOLVING],
            )
            .await?;

        let communication_patterns = self
            .pattern_engine
            .get_pattern_context(
                user_id,
                &[pattern_types::COMMUNICATION, pattern_types::PREFERENCE],
            )
            .await?;

        let work_patterns = self
            .pattern_engine
            .get_pattern_context(
                user_id,
                &[pattern_types::WORK_PATTERN, pattern_types::TOPIC_INTEREST],
            )
            .await?;

        // Load memory facts (personal, professional, technical)
        let personal_facts = self
            .storage
            .get_facts_by_category(user_id, fact_categories::PERSONAL)
            .await?;

        let professional_facts = self
            .storage
            .get_facts_by_category(user_id, fact_categories::PROFESSIONAL)
            .await?;

        let technical_facts = self
            .storage
            .get_facts_by_category(user_id, fact_categories::TECHNICAL)
            .await?;

        // Convert facts to context format
        let fact_contexts: Vec<FactContext> = [personal_facts, professional_facts, technical_facts]
            .concat()
            .iter()
            .map(|f| FactContext {
                fact_category: f.fact_category.clone(),
                fact_key: f.fact_key.clone(),
                fact_value: f.fact_value.clone(),
                confidence: f.confidence,
            })
            .collect();

        let context = RelationshipContext {
            profile: UserProfileContext {
                user_id: profile.user_id.clone(),
                preferred_languages: profile
                    .preferred_languages
                    .as_ref()
                    .and_then(|s| serde_json::from_str(s).ok()),
                conversation_style: profile.conversation_style.clone(),
                code_verbosity: profile.code_verbosity.clone(),
                explanation_depth: profile.explanation_depth.clone(),
            },
            coding_patterns,
            communication_patterns,
            work_patterns,
            facts: fact_contexts,
        };

        debug!(
            "Loaded context: {} coding patterns, {} communication patterns, {} work patterns, {} facts",
            context.coding_patterns.len(),
            context.communication_patterns.len(),
            context.work_patterns.len(),
            context.facts.len()
        );

        Ok(context)
    }

    /// Load minimal context (just for quick reference)
    pub async fn load_minimal_context(&self, user_id: &str) -> Result<MinimalContext> {
        let profile = self.storage.get_or_create_profile(user_id).await?;

        // Get only high-confidence patterns
        let top_patterns = self
            .pattern_engine
            .get_applicable_patterns(
                user_id,
                &[
                    pattern_types::CODING_STYLE,
                    pattern_types::COMMUNICATION,
                    pattern_types::PREFERENCE,
                ],
                0.7, // High confidence only
            )
            .await?;

        // Get key facts
        let key_facts = self.storage.get_facts(user_id).await?;
        let key_facts: Vec<_> = key_facts
            .iter()
            .filter(|f| f.confidence >= 0.8)
            .take(10)
            .cloned()
            .collect();

        Ok(MinimalContext {
            code_verbosity: profile.code_verbosity.clone(),
            conversation_style: profile.conversation_style.clone(),
            top_patterns: top_patterns.into_iter().take(5).collect(),
            key_facts,
        })
    }

    /// Get context formatted for LLM prompt
    pub async fn get_llm_context_string(&self, user_id: &str) -> Result<String> {
        let context = self.load_context(user_id).await?;

        let mut parts = Vec::new();

        // Profile preferences
        if let Some(ref verbosity) = context.profile.code_verbosity {
            parts.push(format!("Code verbosity preference: {}", verbosity));
        }
        if let Some(ref style) = context.profile.conversation_style {
            parts.push(format!("Conversation style: {}", style));
        }

        // Top coding patterns
        if !context.coding_patterns.is_empty() {
            parts.push("\nCoding patterns:".to_string());
            for pattern in context.coding_patterns.iter().take(3) {
                parts.push(format!(
                    "- {} (confidence: {:.0}%): {}",
                    pattern.pattern_name,
                    pattern.confidence * 100.0,
                    pattern.pattern_description
                ));
            }
        }

        // Communication patterns
        if !context.communication_patterns.is_empty() {
            parts.push("\nCommunication patterns:".to_string());
            for pattern in context.communication_patterns.iter().take(2) {
                parts.push(format!(
                    "- {}: {}",
                    pattern.pattern_name, pattern.pattern_description
                ));
            }
        }

        // Key facts
        if !context.facts.is_empty() {
            parts.push("\nKey facts:".to_string());
            for fact in context.facts.iter().take(5) {
                parts.push(format!("- {}: {}", fact.fact_key, fact.fact_value));
            }
        }

        Ok(parts.join("\n"))
    }

    /// Update session metadata (last active, session count)
    pub async fn update_session_metadata(&self, user_id: &str) -> Result<()> {
        let mut profile = self.storage.get_or_create_profile(user_id).await?;

        let now = chrono::Utc::now().timestamp();
        profile.last_active = Some(now);
        profile.total_sessions += 1;

        self.storage.update_profile(&profile).await?;

        debug!("Updated session metadata for user: {}", user_id);
        Ok(())
    }
}

/// Full relationship context for a user
#[derive(Debug, Clone)]
pub struct RelationshipContext {
    pub profile: UserProfileContext,
    pub coding_patterns: Vec<PatternContext>,
    pub communication_patterns: Vec<PatternContext>,
    pub work_patterns: Vec<PatternContext>,
    pub facts: Vec<FactContext>,
}

/// Minimal context for quick lookups
#[derive(Debug, Clone)]
pub struct MinimalContext {
    pub code_verbosity: Option<String>,
    pub conversation_style: Option<String>,
    pub top_patterns: Vec<crate::relationship::LearnedPattern>,
    pub key_facts: Vec<crate::relationship::MemoryFact>,
}
