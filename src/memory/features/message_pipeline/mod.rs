// src/memory/features/message_pipeline/mod.rs

//! Message Pipeline - Unified analysis for all message types

pub mod analyzers;

use std::sync::Arc;
use anyhow::Result;
use tracing::{info, debug, error};

use crate::llm::provider::LlmProvider;
use crate::memory::storage::sqlite::core::MessageAnalysis;

use self::analyzers::unified::UnifiedAnalyzer;

/// Main MessagePipeline - coordinates analysis
pub struct MessagePipeline {
    analyzer: UnifiedAnalyzer,
}

impl MessagePipeline {
    /// Create new message pipeline
    pub fn new(llm_provider: Arc<dyn LlmProvider>) -> Self {
        let analyzer = UnifiedAnalyzer::new(llm_provider);
        Self { analyzer }
    }
    
    /// Create message pipeline with custom configuration
    pub fn with_config(
        llm_provider: Arc<dyn LlmProvider>,
        analyzer_config: AnalyzerConfig,
    ) -> Self {
        let analyzer = UnifiedAnalyzer::with_config(llm_provider, analyzer_config);
        Self { analyzer }
    }
    
    /// Main analysis entry point
    pub async fn analyze_message(
        &self,
        content: &str,
        role: &str,
        context: Option<&str>,
    ) -> Result<MessagePipelineResult> {
        info!("Processing message through unified pipeline: role={}", role);
        
        let analysis_result = self.analyzer
            .analyze_message(content, role, context)
            .await
            .map_err(|e| {
                error!("Analysis failed: {}", e);
                e
            })?;
        
        debug!("Analysis complete: salience={}, is_code={}", 
               analysis_result.salience, analysis_result.is_code);
        
        let pipeline_result = MessagePipelineResult {
            analysis: analysis_result.clone(),
            should_embed: analysis_result.routing.should_embed,
        };
        
        info!("Message processing complete: should_embed={}", 
              pipeline_result.should_embed);
        
        Ok(pipeline_result)
    }
    
    /// Analyze message and return UnifiedAnalysisResult for coordinator compatibility
    pub async fn analyze_message_for_coordinator(
        &self,
        content: &str,
        role: &str,
        context: Option<&str>,
    ) -> Result<UnifiedAnalysisResult> {
        let result = self.analyze_message(content, role, context).await?;
        Ok(result.analysis)
    }
    
    /// Process pending messages in batch
    pub async fn process_pending_messages(&self, _session_id: &str) -> Result<usize> {
        // TODO: Implement batch processing once storage layer is integrated
        Ok(0)
    }
    
    /// Quick content classification without full analysis
    pub fn classify_content(&self, content: &str) -> ContentClassification {
        let estimated_complexity = if content.len() > 1000 { 
            ContentComplexity::High 
        } else if content.len() > 200 { 
            ContentComplexity::Medium 
        } else { 
            ContentComplexity::Low 
        };
        
        ContentClassification {
            estimated_complexity,
            content_length: content.len(),
        }
    }
}

/// Complete result from message pipeline processing
#[derive(Debug, Clone)]
pub struct MessagePipelineResult {
    pub analysis: UnifiedAnalysisResult,
    pub should_embed: bool,
}

impl MessagePipelineResult {
    /// Convert to MessageAnalysis format expected by storage layer
    pub fn to_storage_analysis(&self) -> MessageAnalysis {
        MessageAnalysis {
            salience: Some(self.analysis.salience),
            original_salience: None,
            topics: Some(self.analysis.topics.clone()),
            mood: self.analysis.mood.clone(),
            intensity: self.analysis.intensity,
            intent: self.analysis.intent.clone(),
            summary: self.analysis.summary.clone(),
            relationship_impact: self.analysis.relationship_impact.clone(),
            analysis_version: Some(self.analysis.analysis_version.clone()),
            contains_code: Some(self.analysis.is_code),
            language: self.analysis.programming_lang.clone(),
            programming_lang: self.analysis.programming_lang.clone(),
            routed_to_heads: None, // GPT-5's routed_to_heads used directly from structured response
            // Error tracking fields from analysis
            contains_error: Some(self.analysis.contains_error),
            error_type: self.analysis.error_type.clone(),
            error_severity: self.analysis.error_severity.clone(),
            error_file: self.analysis.error_file.clone(),
        }
    }
    
    /// Check if this message has high value for memory storage
    pub fn is_high_value(&self) -> bool {
        self.analysis.salience > 0.7 || 
        self.analysis.is_code ||
        self.analysis.contains_error || // Errors are high value
        self.analysis.topics.iter().any(|t| {
            matches!(t.to_lowercase().as_str(), 
                    "architecture" | "design" | "bug" | "error" | "important")
        })
    }
}

/// Quick content classification without full analysis
#[derive(Debug, Clone)]
pub struct ContentClassification {
    pub estimated_complexity: ContentComplexity,
    pub content_length: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ContentComplexity {
    Low,    // < 200 chars
    Medium, // 200-1000 chars  
    High,   // > 1000 chars
}

// Re-export key types
pub use analyzers::unified::{UnifiedAnalysisResult, AnalyzerConfig, RoutingDecision};

pub type UnifiedAnalysis = UnifiedAnalysisResult;
pub type PipelineConfig = AnalyzerConfig;
