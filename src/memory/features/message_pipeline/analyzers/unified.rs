// src/memory/features/message_pipeline/analyzers/unified.rs

//! Unified analyzer that coordinates message analysis
//! Code and error detection handled by LLM - no regex heuristics
//!
//! Phase 4.2: Lowered min_salience_threshold from 0.2 to 0.1 to trust Claude's judgment more

use std::sync::Arc;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use crate::llm::provider::LlmProvider;
use crate::llm::embeddings::EmbeddingHead;

use super::chat_analyzer::ChatAnalyzer;

// ===== UNIFIED ANALYSIS RESULT =====

/// Complete analysis result combining chat analysis, code detection, and error detection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedAnalysisResult {
    // Core classification
    pub salience: f32,
    pub topics: Vec<String>,
    pub is_code: bool,
    pub programming_lang: Option<String>,
    
    // Error detection (LLM-determined)
    pub contains_error: bool,
    pub error_type: Option<String>,
    pub error_file: Option<String>,
    pub error_severity: Option<String>,
    
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
            // PHASE 4.2: Lowered from 0.2 to 0.1 - trust Claude more, filter less
            min_salience_threshold: 0.1,
            analysis_version: "2.0".to_string(),
        }
    }
}

impl UnifiedAnalyzer {
    pub fn new(llm_provider: Arc<dyn LlmProvider>) -> Self {
        let chat_analyzer = ChatAnalyzer::new(llm_provider);
        
        Self {
            chat_analyzer,
            config: AnalyzerConfig::default(),
        }
    }
    
    pub fn with_config(llm_provider: Arc<dyn LlmProvider>, config: AnalyzerConfig) -> Self {
        let mut analyzer = Self::new(llm_provider);
        analyzer.config = config;
        analyzer
    }
    
    /// Main analysis entry point - LLM handles all detection
    pub async fn analyze_message(
        &self, 
        content: &str, 
        role: &str, 
        context: Option<&str>
    ) -> Result<UnifiedAnalysisResult> {
        
        debug!("Starting unified analysis for {} message", role);
        
        // Run chat analysis - LLM detects code, errors, sentiment, everything
        let chat_result = self.chat_analyzer.analyze(content, role, context).await?;
        
        // Build unified result from LLM response
        self.build_unified_result(chat_result).await
    }
    
    /// Build unified result from chat analysis
    /// No heuristics - trust the LLM's detection
    async fn build_unified_result(
        &self,
        chat_result: super::chat_analyzer::ChatAnalysisResult,
    ) -> Result<UnifiedAnalysisResult> {
        
        // Extract code detection from LLM response
        let mut is_code = chat_result.contains_code.unwrap_or(false);
        let mut programming_lang = chat_result.programming_lang.clone();
        
        // CRITICAL FIX: Validate programming_lang against database constraint
        // Database only allows: rust, typescript, javascript, python, go, java
        // Config languages like json, yaml, bash, etc. must be filtered out
        if let Some(lang) = programming_lang.as_ref() {
            let valid_langs = ["rust", "typescript", "javascript", "python", "go", "java"];
            if !valid_langs.contains(&lang.to_lowercase().as_str()) {
                warn!("Language '{}' not in DB constraint - setting to NULL to avoid constraint violation", lang);
                programming_lang = None;
                // If we filtered out the language, treat as non-code for storage purposes
                if is_code {
                    warn!("Detected code with unsupported language - treating as non-code for storage");
                    is_code = false;
                }
            }
        }
        
        // Safety check: if code detected but no language, treat as non-code
        if is_code && programming_lang.is_none() {
            warn!("LLM detected code but didn't specify valid language - treating as non-code to avoid DB constraint");
            is_code = false;
        }
        
        // Extract error detection from LLM response
        let contains_error = chat_result.contains_error.unwrap_or(false);
        
        if contains_error {
            info!("LLM detected error - type: {:?}, file: {:?}, severity: {:?}",
                chat_result.error_type, chat_result.error_file, chat_result.error_severity);
        }
        
        // Build routing decision
        let routing = self.build_routing_decision(&chat_result, is_code, &programming_lang).await?;
        
        Ok(UnifiedAnalysisResult {
            // Core classification
            salience: chat_result.salience,
            topics: chat_result.topics,
            is_code,
            programming_lang,
            
            // Error detection
            contains_error,
            error_type: chat_result.error_type,
            error_file: chat_result.error_file,
            error_severity: chat_result.error_severity,
            
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
