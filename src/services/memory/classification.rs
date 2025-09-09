// src/services/memory/classification.rs
// Message classification and routing logic for multi-head memory system

use std::sync::Arc;
use anyhow::Result;
use tracing::{debug, info, error};
use crate::llm::client::OpenAIClient;
use crate::llm::classification::Classification;
use crate::llm::embeddings::EmbeddingHead;
use crate::memory::types::MemoryType;
use super::types::{RoutingDecision, ClassificationResult};

/// Handles message classification and determines routing to appropriate memory heads
pub struct MessageClassifier {
    llm_client: Arc<OpenAIClient>,
    min_salience_threshold: f32,
    code_routing_enabled: bool,
    summary_routing_enabled: bool,
}

impl MessageClassifier {
    /// Creates a new message classifier with default settings
    pub fn new(llm_client: Arc<OpenAIClient>) -> Self {
        Self {
            llm_client,
            min_salience_threshold: 0.2,  // Skip content below this threshold
            code_routing_enabled: true,
            summary_routing_enabled: true,
        }
    }
    
    /// Creates a classifier with custom configuration
    pub fn with_config(
        llm_client: Arc<OpenAIClient>,
        min_salience: f32,
        enable_code: bool,
        enable_summary: bool,
    ) -> Self {
        Self {
            llm_client,
            min_salience_threshold: min_salience,
            code_routing_enabled: enable_code,
            summary_routing_enabled: enable_summary,
        }
    }
    
    /// Classifies message content using GPT-5
    pub async fn classify_message(&self, content: &str) -> Result<Classification> {
        info!("Classifying message with GPT-5 (length: {} chars)", content.len());
        
        match self.llm_client.classify_text(content).await {
            Ok(classification) => {
                debug!(
                    "Classification complete - salience: {:.2}, is_code: {}, topics: {} found",
                    classification.salience,
                    classification.is_code,
                    classification.topics.len()
                );
                Ok(classification)
            }
            Err(e) => {
                error!("Classification failed, using defaults: {}", e);
                // Return sensible defaults on failure
                Ok(Classification {
                    salience: 0.5,
                    is_code: false,
                    lang: String::new(),
                    topics: vec![],
                })
            }
        }
    }
    
    /// Makes routing decision based on classification and role
    pub async fn make_routing_decision(
        &self,
        content: &str,
        role: &str,
        custom_salience: Option<f32>,
    ) -> Result<RoutingDecision> {
        let classification = self.classify_message(content).await?;
        let effective_salience = custom_salience.unwrap_or(classification.salience);
        
        // Check if content should be embedded at all
        if !self.should_embed_content(&classification, effective_salience) {
            return Ok(RoutingDecision {
                heads: vec![],
                should_embed: false,
                skip_reason: Some(format!(
                    "Below salience threshold ({:.2} < {:.2})",
                    effective_salience,
                    self.min_salience_threshold
                )),
            });
        }
        
        // Determine which heads to route to
        let heads = self.determine_embedding_heads(&classification, role);
        
        if heads.is_empty() {
            return Ok(RoutingDecision {
                heads: vec![],
                should_embed: false,
                skip_reason: Some("No suitable heads for content".to_string()),
            });
        }
        
        Ok(RoutingDecision {
            heads,
            should_embed: true,
            skip_reason: None,
        })
    }
    
    /// Determines if content should be embedded based on salience
    pub fn should_embed_content(&self, classification: &Classification, salience: f32) -> bool {
        // Skip very low salience content
        if salience < self.min_salience_threshold {
            info!("Skipping embedding for low-salience content ({:.2})", salience);
            return false;
        }
        
        // Skip trivial content without topics or code
        if classification.topics.is_empty() && !classification.is_code {
            if salience < 3.0 {
                info!("Skipping embedding for trivial content");
                return false;
            }
        }
        
        true
    }
    
    /// Determines which embedding heads should receive this content
    pub fn determine_embedding_heads(
        &self,
        classification: &Classification,
        role: &str,
    ) -> Vec<EmbeddingHead> {
        let mut heads = Vec::new();
        
        // Semantic head for sufficient salience (primary memory)
        if classification.salience >= 0.3 {
            heads.push(EmbeddingHead::Semantic);
            debug!("Routing to Semantic collection (salience: {:.2})", classification.salience);
        }
        
        // Code head for code content
        if self.code_routing_enabled && classification.is_code {
            heads.push(EmbeddingHead::Code);
            info!("Routing to Code collection - language: {}", classification.lang);
        }
        
        // Summary head for system summaries
        if self.summary_routing_enabled 
            && role == "system" 
            && classification.topics.iter().any(|t| t.contains("summary")) {
            heads.push(EmbeddingHead::Summary);
            info!("Routing to Summary collection (system message with summary tag)");
        }
        
        // Default to semantic if nothing else matches but salience is high
        if heads.is_empty() && classification.salience >= 0.5 {
            heads.push(EmbeddingHead::Semantic);
            debug!("Default routing to Semantic collection (high salience: {:.2})", 
                classification.salience);
        }
        
        info!("Routing decision: {} collection(s) - {:?}", heads.len(), 
            heads.iter().map(|h| h.as_str()).collect::<Vec<_>>());
        
        heads
    }
    
    /// Parses memory type from string
    pub fn parse_memory_type(&self, type_str: &str) -> MemoryType {
        match type_str.to_lowercase().as_str() {
            "feeling" => MemoryType::Feeling,
            "fact" => MemoryType::Fact,
            "joke" => MemoryType::Joke,
            "promise" => MemoryType::Promise,
            "event" => MemoryType::Event,
            "summary" => MemoryType::Summary,
            _ => MemoryType::Other,
        }
    }
    
    /// Creates a ClassificationResult from raw classification
    pub fn to_classification_result(
        &self,
        classification: Classification,
        role: &str,
    ) -> ClassificationResult {
        let heads = self.determine_embedding_heads(&classification, role);
        let memory_type = if classification.topics.iter().any(|t| t.contains("summary")) {
            MemoryType::Summary
        } else if classification.is_code {
            MemoryType::Other  // Could be refined to Code type if added
        } else {
            MemoryType::Other
        };
        
        ClassificationResult {
            salience: classification.salience,
            is_code: classification.is_code,
            lang: Some(classification.lang),
            topics: classification.topics,
            memory_type,
            suggested_heads: heads,
        }
    }
    
    /// Gets routing statistics for monitoring
    pub fn get_routing_stats(&self) -> String {
        format!(
            "Classifier Config - Min Salience: {:.2}, Code: {}, Summary: {}",
            self.min_salience_threshold,
            if self.code_routing_enabled { "enabled" } else { "disabled" },
            if self.summary_routing_enabled { "enabled" } else { "disabled" }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_low_salience_rejection() {
        let classifier = MessageClassifier::new(Arc::new(OpenAIClient::mock()));
        
        let classification = Classification {
            salience: 0.1,
            is_code: false,
            lang: String::new(),
            topics: vec![],
        };
        
        assert!(!classifier.should_embed_content(&classification, 0.1));
    }
    
    #[tokio::test]
    async fn test_code_routing() {
        let classifier = MessageClassifier::new(Arc::new(OpenAIClient::mock()));
        
        let classification = Classification {
            salience: 0.5,
            is_code: true,
            lang: Some("rust".to_string()),
            topics: vec!["programming".to_string()],
        };
        
        let heads = classifier.determine_embedding_heads(&classification, "user");
        assert!(heads.contains(&EmbeddingHead::Code));
        assert!(heads.contains(&EmbeddingHead::Semantic));
    }
    
    #[tokio::test]
    async fn test_summary_routing() {
        let classifier = MessageClassifier::new(Arc::new(OpenAIClient::mock()));
        
        let classification = Classification {
            salience: 1.0,
            is_code: false,
            lang: String::new(),
            topics: vec!["summary".to_string(), "rolling".to_string()],
        };
        
        let heads = classifier.determine_embedding_heads(&classification, "system");
        assert!(heads.contains(&EmbeddingHead::Summary));
        assert!(heads.contains(&EmbeddingHead::Semantic));
    }
    
    #[tokio::test]
    async fn test_multi_head_routing() {
        let classifier = MessageClassifier::new(Arc::new(OpenAIClient::mock()));
        
        // Code summary should route to all three heads
        let classification = Classification {
            salience: 0.8,
            is_code: true,
            lang: Some("python".to_string()),
            topics: vec!["summary".to_string(), "code".to_string()],
        };
        
        let heads = classifier.determine_embedding_heads(&classification, "system");
        assert_eq!(heads.len(), 3);  // Semantic, Code, and Summary
    }
}
