// src/memory/features/message_pipeline/analyzers/unified.rs

//! Unified analyzer that coordinates message analysis
//! This is the main entry point for all message analysis
//!
//! Code detection is handled here for routing purposes.
//! Detailed code analysis with AST parsing is handled by CodeIntelligenceService.

use std::sync::Arc;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

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
    
    /// Main analysis entry point - coordinates all analysis types
    pub async fn analyze_message(
        &self, 
        content: &str, 
        role: &str, 
        context: Option<&str>
    ) -> Result<UnifiedAnalysisResult> {
        
        debug!("Starting unified analysis for {} message", role);
        
        // Quick code detection for routing purposes
        let is_code = self.detect_code_content(content);
        if is_code {
            info!("Detected code content");
        }
        
        // Run chat analysis
        let chat_result = self.chat_analyzer.analyze(content, role, context).await?;
        
        // Build unified result
        self.build_unified_result(chat_result, is_code).await
    }
    
    /// Quick heuristic code detection for routing decisions
    /// Note: This is sufficient for routing. Detailed code analysis with AST parsing
    /// is handled separately by CodeIntelligenceService when needed.
    fn detect_code_content(&self, content: &str) -> bool {
        let code_indicators = [
            "```", "fn ", "pub fn", "impl ", "struct ", "enum ",
            "const ", "let ", "mut ", "async fn", "await",
            "use ", "mod ", "#[", "//", "/*", "*/",
            "function", "const ", "let ", "var ", "=>",
            "import ", "export ", "interface ", "type ",
            "class ", "extends", "implements"
        ];
        
        let content_lower = content.to_lowercase();
        let code_count = code_indicators.iter()
            .filter(|&&indicator| content_lower.contains(indicator))
            .count();
            
        // If we find multiple code indicators, likely code content
        code_count >= 2
    }
    
    /// Build unified result from chat analysis and code detection
    async fn build_unified_result(
        &self,
        chat_result: super::chat_analyzer::ChatAnalysisResult,
        is_code: bool,
    ) -> Result<UnifiedAnalysisResult> {
        
        // Determine programming language if code detected
        let programming_lang = if is_code {
            self.detect_programming_language(&chat_result.content)
        } else {
            None
        };
        
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
    
    /// Simple programming language detection for routing purposes
    /// Note: This basic detection is sufficient for routing decisions.
    /// Detailed language analysis happens via CodeIntelligenceService with proper AST parsing.
    fn detect_programming_language(&self, content: &str) -> Option<String> {
        if content.contains("fn ") || content.contains("impl ") || content.contains("struct ") {
            Some("rust".to_string())
        } else if content.contains("interface ") || content.contains("type ") {
            Some("typescript".to_string())
        } else if content.contains("function ") || content.contains("const ") || content.contains("=>") {
            Some("javascript".to_string())
        } else if content.contains("def ") || content.contains("class ") {
            Some("python".to_string())
        } else {
            None
        }
    }
}
