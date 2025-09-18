// src/memory/features/message_pipeline/analyzers/unified.rs

//! Unified analyzer that coordinates chat and code analysis
//! This is the main entry point for all message analysis

use std::sync::Arc;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

use crate::llm::client::OpenAIClient;
use crate::llm::embeddings::EmbeddingHead;

use super::{chat_analyzer::ChatAnalyzer, code_analyzer::CodeAnalyzer};

// ===== UNIFIED ANALYSIS RESULT =====

/// Complete analysis result combining chat and code analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedAnalysisResult {
    // Core classification
    pub salience: f32,
    pub topics: Vec<String>,
    pub is_code: bool,
    pub programming_lang: Option<String>,
    
    // Chat analysis (from ChatAnalyzer)
    pub mood: Option<String>,
    pub intensity: Option<f32>,
    pub intent: Option<String>,
    pub summary: Option<String>,
    pub relationship_impact: Option<String>,
    
    // Code analysis (from CodeAnalyzer - future)
    pub code_quality: Option<String>,
    pub code_complexity: Option<f32>,
    pub code_purpose: Option<String>,
    
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
    code_analyzer: CodeAnalyzer,
    config: AnalyzerConfig,
}

#[derive(Debug, Clone)]
pub struct AnalyzerConfig {
    pub min_salience_threshold: f32,
    pub enable_code_analysis: bool,
    pub analysis_version: String,
}

impl Default for AnalyzerConfig {
    fn default() -> Self {
        Self {
            min_salience_threshold: 0.2,
            enable_code_analysis: true,
            analysis_version: "2.0".to_string(),
        }
    }
}

impl UnifiedAnalyzer {
    pub fn new(llm_client: Arc<OpenAIClient>) -> Self {
        let chat_analyzer = ChatAnalyzer::new(llm_client.clone());
        let code_analyzer = CodeAnalyzer::new(llm_client.clone());
        
        Self {
            chat_analyzer,
            code_analyzer,
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
        
        // First pass: quick code detection
        let is_code = self.detect_code_content(content);
        
        // Run appropriate analysis based on content type
        let (chat_result, code_result) = if is_code && self.config.enable_code_analysis {
            info!("Detected code content, running both analyzers");
            let chat_future = self.chat_analyzer.analyze(content, role, context);
            let code_future = self.code_analyzer.analyze(content, role, context);
            
            // Run both analyzers concurrently
            let (chat_res, code_res) = tokio::try_join!(chat_future, code_future)?;
            (Some(chat_res), Some(code_res))
            
        } else {
            info!("Regular chat content, running chat analyzer only");
            let chat_result = self.chat_analyzer.analyze(content, role, context).await?;
            (Some(chat_result), None)
        };
        
        // Combine results into unified analysis
        self.build_unified_result(chat_result, code_result, is_code).await
    }
    
    /// Quick heuristic code detection
    /// TODO: Replace with proper AST parsing when code intelligence is implemented
    fn detect_code_content(&self, content: &str) -> bool {
        // TEMPORARY: Simple heuristics - will be replaced with proper parsing
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
    
    /// Combine chat and code analysis results
    async fn build_unified_result(
        &self,
        chat_result: Option<super::chat_analyzer::ChatAnalysisResult>,
        code_result: Option<super::code_analyzer::CodeAnalysisResult>,
        is_code: bool,
    ) -> Result<UnifiedAnalysisResult> {
        
        // Start with chat analysis as base (always present)
        let chat = chat_result.as_ref().unwrap();
        
        // Determine programming language if code detected
        let programming_lang = if is_code {
            code_result.as_ref().and_then(|c| c.programming_lang.clone())
                .or_else(|| self.guess_programming_language(&chat.content))
        } else {
            None
        };
        
        // Build routing decision
        let routing = self.build_routing_decision(&chat, code_result.as_ref(), is_code).await?;
        
        Ok(UnifiedAnalysisResult {
            // Core classification
            salience: chat.salience,
            topics: chat.topics.clone(),
            is_code,
            programming_lang,
            
            // Chat analysis
            mood: chat.mood.clone(),
            intensity: chat.intensity,
            intent: chat.intent.clone(),
            summary: chat.summary.clone(),
            relationship_impact: chat.relationship_impact.clone(),
            
            // Code analysis (if available)
            code_quality: code_result.as_ref().and_then(|c| c.quality.clone()),
            code_complexity: code_result.as_ref().and_then(|c| c.complexity),
            code_purpose: code_result.as_ref().and_then(|c| c.purpose.clone()),
            
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
        code_result: Option<&super::code_analyzer::CodeAnalysisResult>,
        is_code: bool,
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
            if let Some(code_analysis) = code_result {
                if let Some(lang) = &code_analysis.programming_lang {
                    if lang == "rust" {
                        heads.push(EmbeddingHead::Code);
                    }
                    // Add other language heads as needed
                }
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
        
        // Remove duplicates (no sorting since EmbeddingHead doesn't implement Ord)
        heads.dedup();
        
        Ok(RoutingDecision {
            should_embed: true,
            embedding_heads: heads,
            skip_reason: None,
        })
    }
    
    /// Simple programming language detection fallback
    /// TODO: Replace with proper AST-based language detection
    fn guess_programming_language(&self, content: &str) -> Option<String> {
        // TEMPORARY: Basic pattern matching - will be replaced with proper parsing
        if content.contains("fn ") || content.contains("impl ") || content.contains("struct ") {
            Some("rust".to_string())
        } else if content.contains("function ") || content.contains("const ") || content.contains("=>") {
            Some("javascript".to_string())
        } else if content.contains("interface ") || content.contains("type ") {
            Some("typescript".to_string())
        } else {
            None
        }
    }
}
