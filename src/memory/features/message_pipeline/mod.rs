// src/memory/features/message_pipeline/mod.rs

//! Message Pipeline - Unified analysis and routing for all message types
//! 
//! This module replaces the monolithic message_pipeline.rs with a clean modular architecture:
//! - `analyzers/` - Chat and code analysis logic
//! - `routing/` - Memory embedding routing decisions  
//! 
//! The pipeline coordinates analysis and routing to provide a single entry point
//! for processing user messages, maintaining backward compatibility while enabling
//! future code intelligence features.

pub mod analyzers;
pub mod routing;

use std::sync::Arc;
use anyhow::Result;
use tracing::{info, debug, error};

use crate::llm::client::OpenAIClient;
use crate::memory::storage::sqlite::core::MessageAnalysis;

use self::{
    analyzers::{
        unified::UnifiedAnalyzer,
    },
    routing::{
        memory_routing::MemoryRouter,
    },
};

// ===== MAIN MESSAGE PIPELINE =====

/// Main MessagePipeline - coordinates analysis and routing
/// 
/// This replaces the old monolithic MessagePipeline with a clean modular design.
/// Maintains the same external interface for backward compatibility.
pub struct MessagePipeline {
    analyzer: UnifiedAnalyzer,
    router: MemoryRouter,
}

impl MessagePipeline {
    /// Create new message pipeline with clean single-parameter interface
    pub fn new(llm_client: Arc<OpenAIClient>) -> Self {
        let analyzer = UnifiedAnalyzer::new(llm_client);
        let router = MemoryRouter::new(RoutingConfig::default());
        
        Self { analyzer, router }
    }
    
    /// Create message pipeline with custom configuration
    pub fn with_config(
        llm_client: Arc<OpenAIClient>,
        analyzer_config: AnalyzerConfig,
        routing_config: RoutingConfig,
    ) -> Self {
        let analyzer = UnifiedAnalyzer::with_config(llm_client, analyzer_config);
        let router = MemoryRouter::new(routing_config);
        
        Self { analyzer, router }
    }
    
    /// Main analysis entry point - maintains backward compatibility
    /// 
    /// This method signature matches the old MessagePipeline::analyze_message
    /// to ensure seamless migration from the monolithic implementation.
    pub async fn analyze_message(
        &self,
        content: &str,
        role: &str,
        context: Option<&str>,
    ) -> Result<MessagePipelineResult> {
        info!("Processing message through unified pipeline: role={}", role);
        
        // Step 1: Run unified analysis
        let analysis_result = self.analyzer
            .analyze_message(content, role, context)
            .await
            .map_err(|e| {
                error!("Analysis failed: {}", e);
                e
            })?;
        
        debug!("Analysis complete: salience={}, is_code={}", 
               analysis_result.salience, analysis_result.is_code);
        
        // Step 2: Determine routing strategy
        let routing_strategy = self.router
            .determine_routing(&analysis_result)
            .await
            .map_err(|e| {
                error!("Routing determination failed: {}", e);
                e
            })?;
        
        // Step 3: Validate routing makes sense
        self.router
            .validate_routing(&routing_strategy)
            .map_err(|e| {
                error!("Routing validation failed: {}", e);
                e
            })?;
        
        debug!("Routing complete: primary={:?}, secondary={:?}", 
               routing_strategy.primary_head, routing_strategy.secondary_heads);
        
        // Convert to result format expected by callers
        let pipeline_result = MessagePipelineResult {
            analysis: analysis_result.clone(),
            routing: routing_strategy.clone(),
            should_embed: analysis_result.routing.should_embed,
            embedding_heads: self.router.get_all_heads(&routing_strategy),
        };
        
        info!("Message processing complete: should_embed={}, heads={}", 
              pipeline_result.should_embed, pipeline_result.embedding_heads.len());
        
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
    
    /// Process pending messages in batch (maintains backward compatibility)
    pub async fn process_pending_messages(&self, _session_id: &str) -> Result<usize> {
        // TODO: Implement batch processing once storage layer is integrated
        // For now, return 0 to maintain compatibility
        Ok(0)
    }
    
    /// Quick content classification without full analysis
    /// 
    /// Useful for routing decisions that don't require full LLM analysis
    /// TODO: Enhance with proper parsing when code intelligence is implemented  
    pub fn classify_content(&self, content: &str) -> ContentClassification {
        // TEMPORARY: Use simple heuristics for quick classification
        let is_code = content.contains("```") || 
                      content.contains("fn ") ||
                      content.contains("impl ") ||
                      content.contains("function ");
        
        let is_question = content.contains("?") || 
                         content.to_lowercase().starts_with("how ") ||
                         content.to_lowercase().starts_with("what ") ||
                         content.to_lowercase().starts_with("why ");
        
        let estimated_complexity = if content.len() > 1000 { 
            ContentComplexity::High 
        } else if content.len() > 200 { 
            ContentComplexity::Medium 
        } else { 
            ContentComplexity::Low 
        };
        
        ContentClassification {
            is_code,
            is_question,
            estimated_complexity,
            content_length: content.len(),
        }
    }
}

// ===== PIPELINE RESULT TYPES =====

/// Complete result from message pipeline processing
#[derive(Debug, Clone)]
pub struct MessagePipelineResult {
    pub analysis: UnifiedAnalysisResult,
    pub routing: routing::memory_routing::RoutingStrategy,
    pub should_embed: bool,
    pub embedding_heads: Vec<crate::llm::embeddings::EmbeddingHead>,
}

impl MessagePipelineResult {
    /// Convert to the MessageAnalysis format expected by storage layer
    /// 
    /// This maintains compatibility with the existing storage interface
    pub fn to_storage_analysis(&self) -> MessageAnalysis {
        MessageAnalysis {
            salience: Some(self.analysis.salience),
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
            routed_to_heads: Some(
                self.embedding_heads
                    .iter()
                    .map(|h| h.as_str().to_string())
                    .collect::<Vec<_>>()
            ),
        }
    }
    
    /// Check if this message has high value for memory storage
    pub fn is_high_value(&self) -> bool {
        self.analysis.salience > 0.7 || 
        self.analysis.is_code ||
        self.analysis.topics.iter().any(|t| {
            matches!(t.to_lowercase().as_str(), 
                    "architecture" | "design" | "bug" | "error" | "important")
        })
    }
}

/// Quick content classification without full analysis
#[derive(Debug, Clone)]
pub struct ContentClassification {
    pub is_code: bool,
    pub is_question: bool,
    pub estimated_complexity: ContentComplexity,
    pub content_length: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ContentComplexity {
    Low,    // < 200 chars
    Medium, // 200-1000 chars  
    High,   // > 1000 chars
}

// Re-export key types for backward compatibility
pub use analyzers::unified::{UnifiedAnalysisResult, AnalyzerConfig, RoutingDecision};
pub use routing::memory_routing::{RoutingStrategy, RoutingConfig};

// Clean aliases
pub type UnifiedAnalysis = UnifiedAnalysisResult;
pub type PipelineConfig = AnalyzerConfig;

// ===== BACKWARD COMPATIBILITY =====

/// Legacy MessageAnalysis result for backward compatibility
pub type LegacyMessageAnalysis = MessageAnalysis;

impl From<MessagePipelineResult> for MessageAnalysis {
    fn from(result: MessagePipelineResult) -> Self {
        result.to_storage_analysis()
    }
}
