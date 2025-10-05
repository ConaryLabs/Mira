// src/memory/features/message_pipeline/analyzers/unified.rs

//! Unified analyzer that coordinates message analysis
//! Code detection is now handled by LLM via tool schema - no regex heuristics

use std::sync::Arc;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use crate::llm::client::OpenAIClient;
use crate::llm::embeddings::EmbeddingHead;

use super::chat_analyzer::ChatAnalyzer;

// ===== UNIFIED ANALYSIS RESULT =====

/// Complete analysis result combining chat analysis and code detection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedAnalysisResult {
    // Core classification
    pub salience: f32,
    pub topics: Vec<String>,
    pub is_code: bool,
    pub programming_lang: Option<String>,
    
    // Chat analysis
    pub mood: Option<String>,
    pub intensity: Option<f32>,
    pub intent: Option<String>,
    pub summary: Option<String>,
    pub relationship_impact: Option<String>,
    
    // Routing decision
    pub routing: RoutingDecision,
    
    // Metadata
    pub processed_at: chrono::DateTime<chrono::Utc>,
    pub analysis_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingDecision {
    pub should_embed: bool,
    pub embedding_heads: Vec<EmbeddingHead>,
    pub skip_reason: Option<String>,
}

// ===== UNIFIED ANALYZER =====

pub struct UnifiedAnalyzer {
    chat_analyzer: ChatAnalyzer,
    config: AnalyzerConfig,
}

#[derive(Debug, Clone)]
pub struct AnalyzerConfig {
    pub min_salience_threshold: f32,
    pub analysis_version: String,
}

impl Default for AnalyzerConfig {
    fn default() -> Self {
        Self {
            min_salience_threshold: 0.2,
            analysis_version: "2.0".to_string(),
        }
    }
}

impl UnifiedAnalyzer {
    pub fn new(llm_client: Arc<OpenAIClient>) -> Self {
        let chat_analyzer = ChatAnalyzer::new(llm_client);
        
        Self {
            chat_analyzer,
            config: AnalyzerConfig::default(),
        }
    }
    
    pub fn with_config(llm_client: Arc<OpenAIClient>, config: AnalyzerConfig) -> Self {
        let mut analyzer = Self::new(llm_client);
        analyzer.config = config;
        analyzer
    }
    
    /// Main analysis entry point - LLM handles all code detection
    pub async fn analyze_message(
        &self, 
        content: &str, 
        role: &str, 
        context: Option<&str>
    ) -> Result<UnifiedAnalysisResult> {
        
        debug!("Starting unified analysis for {} message", role);
        
        // Run chat analysis - LLM detects code via tool schema
        let chat_result = self.chat_analyzer.analyze(content, role, context).await?;
        
        // Build unified result from LLM response
        self.build_unified_result(chat_result).await
    }
    
    /// Build unified result from chat analysis
    /// No heuristics - trust the LLM's code detection
    async fn build_unified_result(
        &self,
        chat_result: super::chat_analyzer::ChatAnalysisResult,
    ) -> Result<UnifiedAnalysisResult> {
        
        // Extract code detection from LLM response
        let mut is_code = chat_result.contains_code.unwrap_or(false);
        let mut programming_lang = chat_result.programming_lang.clone();
        
        // Safety check: if code detected but no language, treat as non-code
        if is_code && programming_lang.is_none() {
            warn!("LLM detected code but didn't specify language - treating as non-code to avoid DB constraint");
            is_code = false;
        }
        
        // Build routing decision
        let routing = self.build_routing_decision(&chat_result, is_code, &programming_lang).await?;
        
        Ok(UnifiedAnalysisResult {
            // Core classification
            salience: chat_result.salience,
            topics: chat_result.topics,
            is_code,
            programming_lang,
            
            // Chat analysis
            mood: chat_result.mood,
            intensity: chat_result.intensity,
            intent: chat_result.intent,
            summary: chat_result.summary,
            relationship_impact: chat_result.relationship_impact,
            
            // Routing
            routing,
            
            // Metadata
            processed_at: chrono::Utc::now(),
            analysis_version: self.config.analysis_version.clone(),
        })
    }
    
    /// Build routing decision based on analysis results
    async fn build_routing_decision(
        &self,
        chat_result: &super::chat_analyzer::ChatAnalysisResult,
        is_code: bool,
        programming_lang: &Option<String>,
    ) -> Result<RoutingDecision> {
        
        // Skip if salience too low
        if chat_result.salience < self.config.min_salience_threshold {
            return Ok(RoutingDecision {
                should_embed: false,
                embedding_heads: vec![],
                skip_reason: Some(format!(
                    "Salience {} below threshold {}", 
                    chat_result.salience, 
                    self.config.min_salience_threshold
                )),
            });
        }
        
        let mut heads = vec![EmbeddingHead::Semantic];
        
        // Add code-specific routing
        if is_code {
            heads.push(EmbeddingHead::Code);
            
            // Add language-specific routing if detected
            if let Some(lang) = programming_lang {
                if lang == "rust" {
                    heads.push(EmbeddingHead::Code);
                }
                // Add other language heads as needed
            }
        }
        
        // Add topic-specific routing
        for topic in &chat_result.topics {
            match topic.to_lowercase().as_str() {
                "architecture" | "design" | "planning" => {
                    heads.push(EmbeddingHead::Semantic);
                }
                "bug" | "error" | "debug" | "fix" => {
                    heads.push(EmbeddingHead::Code);
                }
                _ => {} // Semantic head covers everything else
            }
        }
        
        // Remove duplicates
        heads.dedup();
        
        Ok(RoutingDecision {
            should_embed: true,
            embedding_heads: heads,
            skip_reason: None,
        })
    }
}
