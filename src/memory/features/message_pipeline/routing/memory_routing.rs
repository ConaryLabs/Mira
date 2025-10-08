// src/memory/features/message_pipeline/routing/memory_routing.rs

//! Memory routing logic for embedding decisions
//! Determines which embedding heads to use based on analysis results
//!
//! Phase 4.2: Lowered min_salience_threshold from 0.2 to 0.1 to trust the model's judgment more

use std::collections::HashSet;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

use crate::llm::embeddings::EmbeddingHead;
use super::super::analyzers::unified::UnifiedAnalysisResult;

// ===== ROUTING CONFIGURATION =====

#[derive(Debug, Clone)]
pub struct RoutingConfig {
    pub min_salience_threshold: f32,
    pub enable_topic_routing: bool,
    pub enable_language_routing: bool,
    pub max_embedding_heads: usize,
}

impl Default for RoutingConfig {
    fn default() -> Self {
        Self {
            // PHASE 4.2: Lowered from 0.2 to 0.1 - trust the model more, filter less
            min_salience_threshold: 0.1,
            enable_topic_routing: true,
            enable_language_routing: true,
            max_embedding_heads: 5,
        }
    }
}

// ===== ROUTING STRATEGY =====

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingStrategy {
    pub primary_head: EmbeddingHead,
    pub secondary_heads: Vec<EmbeddingHead>,
    pub reasoning: String,
}

// ===== MEMORY ROUTER =====

pub struct MemoryRouter {
    config: RoutingConfig,
}

impl MemoryRouter {
    pub fn new(config: RoutingConfig) -> Self {
        Self { config }
    }
    
    /// Determine embedding strategy based on analysis results
    pub async fn determine_routing(&self, analysis: &UnifiedAnalysisResult) -> Result<RoutingStrategy> {
        debug!("Determining routing for message with salience: {}", analysis.salience);
        
        // Check salience threshold first
        if analysis.salience < self.config.min_salience_threshold {
            return Ok(RoutingStrategy {
                primary_head: EmbeddingHead::Semantic,
                secondary_heads: vec![],
                reasoning: format!(
                    "Salience {} below threshold {}, minimal routing", 
                    analysis.salience, 
                    self.config.min_salience_threshold
                ),
            });
        }
        
        let mut heads = HashSet::new();
        let mut reasoning_parts = Vec::new();
        
        // Always start with semantic head
        heads.insert(EmbeddingHead::Semantic);
        reasoning_parts.push("Semantic head for baseline embedding".to_string());
        
        // Code-specific routing
        if analysis.is_code {
            heads.insert(EmbeddingHead::Code);
            reasoning_parts.push("Code head for programming content".to_string());
            
            // Language-specific routing
            if self.config.enable_language_routing {
                if let Some(lang) = &analysis.programming_lang {
                    match lang.as_str() {
                        "rust" => {
                            heads.insert(EmbeddingHead::Code);
                            reasoning_parts.push("Code head for Rust-specific content".to_string());
                        },
                        "javascript" | "typescript" => {
                            heads.insert(EmbeddingHead::Code);
                            reasoning_parts.push("Code head for JS/TS content".to_string());
                        },
                        "python" => {
                            heads.insert(EmbeddingHead::Code);
                            reasoning_parts.push("Code head for Python content".to_string());
                        },
                        _ => {
                            debug!("No specific head for language: {}", lang);
                        }
                    }
                }
            }
        }
        
        // Topic-based routing
        if self.config.enable_topic_routing {
            for topic in &analysis.topics {
                match topic.to_lowercase().as_str() {
                    "architecture" | "design" | "planning" | "refactoring" => {
                        heads.insert(EmbeddingHead::Semantic);
                        reasoning_parts.push("Semantic head for design/planning content".to_string());
                    },
                    "bug" | "error" | "debug" | "fix" | "issue" | "problem" => {
                        heads.insert(EmbeddingHead::Code);
                        reasoning_parts.push("Code head for troubleshooting content".to_string());
                    },
                    "memory" | "storage" | "database" | "persistence" => {
                        heads.insert(EmbeddingHead::Semantic);
                        reasoning_parts.push("Semantic head for storage-related content".to_string());
                    },
                    "ai" | "llm" | "gpt" | "assistant" | "intelligence" => {
                        heads.insert(EmbeddingHead::Semantic);
                        reasoning_parts.push("Semantic head for AI/ML related content".to_string());
                    },
                    _ => {
                        debug!("No specific head for topic: {}", topic);
                    }
                }
            }
        }
        
        // Intent-based routing
        if let Some(intent) = &analysis.intent {
            match intent.to_lowercase().as_str() {
                "question" | "help" | "documentation" => {
                    heads.insert(EmbeddingHead::Documents);
                    reasoning_parts.push("Documents head for Q&A content".to_string());
                },
                "task" | "todo" | "implement" | "build" => {
                    heads.insert(EmbeddingHead::Semantic);
                    reasoning_parts.push("Semantic head for action items".to_string());
                },
                _ => {}
            }
        }
        
        // Respect max heads limit
        let mut heads_vec: Vec<EmbeddingHead> = heads.into_iter().collect();
        
        // Sort for deterministic ordering
        heads_vec.sort_by_key(|head| match head {
            EmbeddingHead::Code => 1,
            EmbeddingHead::Documents => 2,
            EmbeddingHead::Summary => 3,
            EmbeddingHead::Semantic => 10,
        });
        
        if heads_vec.len() > self.config.max_embedding_heads {
            let original_count = heads_vec.len();
            heads_vec.truncate(self.config.max_embedding_heads);
            reasoning_parts.push(format!(
                "Truncated from {} to {} heads (max limit)", 
                original_count, 
                self.config.max_embedding_heads
            ));
        }
        
        // Determine primary and secondary heads
        let primary_head = heads_vec.first().cloned().unwrap_or(EmbeddingHead::Semantic);
        let secondary_heads = heads_vec.into_iter().skip(1).collect();
        
        let reasoning = reasoning_parts.join("; ");
        
        info!("Routing strategy: primary={:?}, secondary={:?}", primary_head, secondary_heads);
        
        Ok(RoutingStrategy {
            primary_head,
            secondary_heads,
            reasoning,
        })
    }
    
    /// Validate routing strategy makes sense
    pub fn validate_routing(&self, strategy: &RoutingStrategy) -> Result<()> {
        // Ensure we have at least one head
        if strategy.secondary_heads.is_empty() && strategy.primary_head == EmbeddingHead::Semantic {
            // This is fine - minimal routing
        }
        
        // Ensure no duplicate heads
        let mut all_heads = vec![strategy.primary_head.clone()];
        all_heads.extend(strategy.secondary_heads.iter().cloned());
        
        let unique_heads: HashSet<_> = all_heads.iter().collect();
        if unique_heads.len() != all_heads.len() {
            return Err(anyhow::anyhow!("Duplicate heads in routing strategy"));
        }
        
        // Ensure reasonable number of heads
        if all_heads.len() > self.config.max_embedding_heads {
            return Err(anyhow::anyhow!(
                "Too many heads: {} > {}", 
                all_heads.len(), 
                self.config.max_embedding_heads
            ));
        }
        
        Ok(())
    }
    
    /// Get all heads for this routing strategy
    pub fn get_all_heads(&self, strategy: &RoutingStrategy) -> Vec<EmbeddingHead> {
        let mut heads = vec![strategy.primary_head.clone()];
        heads.extend(strategy.secondary_heads.iter().cloned());
        heads
    }
}

// ===== ROUTING UTILITIES =====

/// Helper to determine if content should be embedded at all
pub fn should_embed_content(analysis: &UnifiedAnalysisResult, min_threshold: f32) -> bool {
    analysis.salience >= min_threshold
}

/// Helper to prioritize routing heads by importance
pub fn prioritize_heads(heads: Vec<EmbeddingHead>) -> Vec<EmbeddingHead> {
    let mut heads = heads;
    
    // Sort by priority (most specific first)
    heads.sort_by_key(|head| match head {
        EmbeddingHead::Code => 1,          // Most specific
        EmbeddingHead::Documents => 2,     // Domain specific  
        EmbeddingHead::Summary => 3,       // Context specific
        EmbeddingHead::Semantic => 10,     // Least specific
    });
    
    heads
}
