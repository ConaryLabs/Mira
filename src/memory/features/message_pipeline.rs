// src/memory/features/message_pipeline.rs

//! Unified message processing pipeline for analysis, classification, and routing.
//! Consolidates all message analysis into a single, efficient pipeline.

use std::sync::Arc;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use tracing::{debug, info, error};

use crate::llm::client::OpenAIClient;
use crate::llm::embeddings::EmbeddingHead;
use crate::memory::{
    storage::sqlite::store::{SqliteMemoryStore, MessageAnalysis as SqliteAnalysis},
};

// ===== UNIFIED ANALYSIS RESULT =====

/// Complete analysis result from the pipeline - combines MessageAnalyzer and MessageClassifier outputs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedAnalysis {
    // Core classification
    pub salience: f32,
    pub topics: Vec<String>,
    pub is_code: bool,
    pub programming_lang: Option<String>,
    
    // Sentiment and mood (from MessageAnalyzer)
    pub mood: Option<String>,
    pub intensity: Option<f32>,
    
    // Intent and meaning
    pub intent: Option<String>,
    pub summary: Option<String>,
    pub relationship_impact: Option<String>,
    
    // Routing decision (from MessageClassifier)
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

// ===== PIPELINE CONFIGURATION =====

#[derive(Debug, Clone)]
pub struct PipelineConfig {
    /// Minimum salience to embed
    pub min_salience_threshold: f32,
    /// Enable code routing to dedicated head
    pub code_routing_enabled: bool,
    /// Enable summary routing to dedicated head  
    pub summary_routing_enabled: bool,
    /// Batch size for processing
    pub batch_size: usize,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            min_salience_threshold: 0.2,
            code_routing_enabled: true,
            summary_routing_enabled: true,
            batch_size: 10,
        }
    }
}

// ===== MESSAGE PROCESSING PIPELINE =====

/// Unified message processing pipeline - replaces MessageAnalyzer and MessageClassifier
pub struct MessagePipeline {
    llm_client: Arc<OpenAIClient>,
    sqlite_store: Arc<SqliteMemoryStore>,
    config: PipelineConfig,
}

impl MessagePipeline {
    /// Creates a new message processing pipeline
    pub fn new(
        llm_client: Arc<OpenAIClient>,
        sqlite_store: Arc<SqliteMemoryStore>,
    ) -> Self {
        Self {
            llm_client,
            sqlite_store,
            config: PipelineConfig::default(),
        }
    }
    
    /// Creates pipeline with custom config
    pub fn with_config(
        llm_client: Arc<OpenAIClient>,
        sqlite_store: Arc<SqliteMemoryStore>,
        config: PipelineConfig,
    ) -> Self {
        Self {
            llm_client,
            sqlite_store,
            config,
        }
    }

    /// Process a single message through the entire pipeline
    pub async fn analyze_message(
        &self,
        content: &str,
        role: &str,
        context: Option<&str>,
    ) -> Result<UnifiedAnalysis> {
        info!("Processing {} message through unified pipeline", role);
        
        // Single LLM call for ALL analysis (combines MessageAnalyzer + MessageClassifier)
        let prompt = self.build_unified_prompt(content, role, context);
        
        let response = self.llm_client
            .summarize_conversation(&prompt, 500)
            .await?;
        
        // Parse the unified response
        let mut analysis = self.parse_unified_response(&response)?;
        
        // Determine routing based on analysis
        analysis.routing = self.determine_routing(&analysis, role);
        
        // Add metadata
        analysis.processed_at = chrono::Utc::now();
        analysis.analysis_version = "2.0".to_string();
        
        debug!("Message processed - salience: {:.2}, routing to {} heads", 
            analysis.salience, analysis.routing.embedding_heads.len());
        
        Ok(analysis)
    }

    /// Process multiple messages in batch (replaces AnalysisService::process_pending_messages)
    pub async fn process_pending_messages(&self, session_id: &str) -> Result<usize> {
        // Get unanalyzed messages
        let pending = self.sqlite_store
            .get_unanalyzed_messages(session_id, self.config.batch_size)
            .await?;
        
        if pending.is_empty() {
            return Ok(0);
        }
        
        info!("Processing {} pending messages for session {}", pending.len(), session_id);
        
        // Prepare batch
        let messages: Vec<(String, String)> = pending
            .iter()
            .map(|entry| (entry.content.clone(), entry.role.clone()))
            .collect();
        
        // Analyze in batch with single LLM call
        let analyses = self.analyze_batch(messages).await?;
        
        // Store results
        for (entry, analysis) in pending.iter().zip(analyses.iter()) {
            if let Some(id) = entry.id {
                self.store_analysis(id, analysis).await?;
            }
        }
        
        Ok(analyses.len())
    }
    
    /// Batch analyze multiple messages
    async fn analyze_batch(&self, messages: Vec<(String, String)>) -> Result<Vec<UnifiedAnalysis>> {
        if messages.is_empty() {
            return Ok(Vec::new());
        }
        
        let prompt = self.build_batch_prompt(&messages);
        let response = self.llm_client
            .summarize_conversation(&prompt, 2000)
            .await?;
        
        self.parse_batch_response(&response, &messages)
    }

    // ===== ROUTING LOGIC (from MessageClassifier) =====
    
    fn determine_routing(&self, analysis: &UnifiedAnalysis, role: &str) -> RoutingDecision {
        // Check salience threshold
        if analysis.salience < self.config.min_salience_threshold {
            return RoutingDecision {
                should_embed: false,
                embedding_heads: vec![],
                skip_reason: Some(format!(
                    "Below salience threshold ({:.2} < {:.2})",
                    analysis.salience, self.config.min_salience_threshold
                )),
            };
        }
        
        // Skip trivial content
        if analysis.topics.is_empty() && !analysis.is_code && analysis.salience < 3.0 {
            return RoutingDecision {
                should_embed: false,
                embedding_heads: vec![],
                skip_reason: Some("Trivial content".to_string()),
            };
        }
        
        let mut heads = Vec::new();
        
        // Semantic head for sufficient salience
        if analysis.salience >= 0.3 {
            heads.push(EmbeddingHead::Semantic);
        }
        
        // Code head for code content
        if self.config.code_routing_enabled && analysis.is_code {
            heads.push(EmbeddingHead::Code);
        }
        
        // Summary head for system summaries
        if self.config.summary_routing_enabled 
            && role == "system"
            && analysis.topics.iter().any(|t| t.contains("summary")) {
            heads.push(EmbeddingHead::Summary);
        }
        
        // Default to semantic if high salience but no heads selected
        if heads.is_empty() && analysis.salience >= 0.5 {
            heads.push(EmbeddingHead::Semantic);
        }
        
        // FIXED: Clone heads before moving
        RoutingDecision {
            should_embed: !heads.is_empty(),
            embedding_heads: heads.clone(),
            skip_reason: if heads.is_empty() {
                Some("No suitable heads for content".to_string())
            } else {
                None
            },
        }
    }

    // ===== PROMPT BUILDING =====
    
    fn build_unified_prompt(&self, content: &str, role: &str, context: Option<&str>) -> String {
        format!(
            r#"Analyze this {} message comprehensively and provide a JSON response with ALL these fields:
- salience: float 0-10 (importance for future recall)
- topics: array of strings (key topics/domains)
- is_code: boolean
- programming_lang: string or null (if is_code is true)
- mood: string or null (emotional tone: happy, sad, angry, neutral, excited, etc.)
- intensity: float 0-1 or null (emotional intensity)
- intent: string or null (user's goal: question, statement, command, etc.)
- summary: string or null (one-line summary if message is long)
- relationship_impact: string or null (if this affects the user-assistant relationship)

Context: {}
Message: "{}"

Respond with valid JSON only."#,
            role,
            context.unwrap_or("Start of conversation"),
            content
        )
    }
    
    fn build_batch_prompt(&self, messages: &[(String, String)]) -> String {
        let messages_text = messages
            .iter()
            .enumerate()
            .map(|(i, (content, role))| format!("Message {}: [{}] {}", i + 1, role, content))
            .collect::<Vec<_>>()
            .join("\n\n");

        format!(
            r#"Analyze these {} messages and provide a JSON array. Each element should have ALL fields:
salience, topics, is_code, programming_lang, mood, intensity, intent, summary, relationship_impact

Messages:
{}

Respond with a JSON array matching the message order."#,
            messages.len(),
            messages_text
        )
    }

    // ===== PARSING =====
    
    fn parse_unified_response(&self, response: &str) -> Result<UnifiedAnalysis> {
        // Extract JSON from response
        let json_str = if response.contains('{') && response.contains('}') {
            let start = response.find('{').unwrap();
            let end = response.rfind('}').unwrap() + 1;
            &response[start..end]
        } else {
            response
        };
        
        // Parse into intermediate structure, then convert to UnifiedAnalysis
        #[derive(Deserialize)]
        struct TempAnalysis {
            salience: f32,
            topics: Vec<String>,
            is_code: bool,
            programming_lang: Option<String>,
            mood: Option<String>,
            intensity: Option<f32>,
            intent: Option<String>,
            summary: Option<String>,
            relationship_impact: Option<String>,
        }
        
        let temp: TempAnalysis = serde_json::from_str(json_str)?;
        
        Ok(UnifiedAnalysis {
            salience: temp.salience,
            topics: temp.topics,
            is_code: temp.is_code,
            programming_lang: temp.programming_lang,
            mood: temp.mood,
            intensity: temp.intensity,
            intent: temp.intent,
            summary: temp.summary,
            relationship_impact: temp.relationship_impact,
            routing: RoutingDecision::default(),  // Will be filled by determine_routing
            processed_at: chrono::Utc::now(),
            analysis_version: String::new(),
        })
    }
    
    fn parse_batch_response(&self, response: &str, messages: &[(String, String)]) -> Result<Vec<UnifiedAnalysis>> {
        // Extract JSON array
        let json_str = if response.contains('[') && response.contains(']') {
            let start = response.find('[').unwrap();
            let end = response.rfind(']').unwrap() + 1;
            &response[start..end]
        } else {
            response
        };
        
        let temp_analyses: Vec<serde_json::Value> = serde_json::from_str(json_str)?;
        
        let mut results = Vec::new();
        for (i, (json_val, (_content, role))) in temp_analyses.iter().zip(messages.iter()).enumerate() {
            // FIXED: Add type annotation for from_value
            match serde_json::from_value::<UnifiedAnalysis>(json_val.clone()) {
                Ok(mut analysis) => {
                    // Determine routing for each message
                    analysis.routing = self.determine_routing(&analysis, role);
                    analysis.processed_at = chrono::Utc::now();
                    analysis.analysis_version = "2.0".to_string();
                    results.push(analysis);
                }
                Err(e) => {
                    error!("Failed to parse analysis for message {}: {}", i, e);
                    // Create default analysis on parse failure
                    results.push(UnifiedAnalysis::default());
                }
            }
        }
        
        Ok(results)
    }

    // ===== DATABASE OPERATIONS =====
    
    async fn store_analysis(&self, message_id: i64, analysis: &UnifiedAnalysis) -> Result<()> {
        // Convert to SQLite format
        let sqlite_analysis = SqliteAnalysis {
            mood: analysis.mood.clone(),
            intensity: analysis.intensity,
            salience: Some(analysis.salience),
            intent: analysis.intent.clone(),
            topics: Some(analysis.topics.clone()),
            summary: analysis.summary.clone(),
            relationship_impact: analysis.relationship_impact.clone(),
            contains_code: Some(analysis.is_code),
            language: Some("en".to_string()),
            programming_lang: analysis.programming_lang.clone(),
            analysis_version: Some(analysis.analysis_version.clone()),
            routed_to_heads: Some(
                analysis.routing.embedding_heads
                    .iter()
                    .map(|h| h.as_str().to_string())
                    .collect()
            ),
        };
        
        self.sqlite_store.save_analysis(message_id, &sqlite_analysis).await
    }
}

// Implement Default for easy construction
impl Default for RoutingDecision {
    fn default() -> Self {
        Self {
            should_embed: false,
            embedding_heads: vec![],
            skip_reason: None,
        }
    }
}

impl Default for UnifiedAnalysis {
    fn default() -> Self {
        Self {
            salience: 0.5,
            topics: vec![],
            is_code: false,
            programming_lang: None,
            mood: None,
            intensity: None,
            intent: None,
            summary: None,
            relationship_impact: None,
            routing: RoutingDecision::default(),
            processed_at: chrono::Utc::now(),
            analysis_version: "2.0".to_string(),
        }
    }
}
