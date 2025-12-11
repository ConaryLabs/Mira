// src/memory/features/message_pipeline/mod.rs

//! Message Pipeline - Unified analysis for all message types

pub mod analyzers;

use anyhow::Result;
use sqlx::SqlitePool;
use std::sync::Arc;
use tracing::{debug, error, info, warn};

use crate::llm::LlmProvider;
use crate::memory::storage::sqlite::core::MessageAnalysis;

use self::analyzers::unified::UnifiedAnalyzer;

/// Main MessagePipeline - coordinates analysis
pub struct MessagePipeline {
    analyzer: UnifiedAnalyzer,
    pool: Option<SqlitePool>,
}

impl MessagePipeline {
    /// Create new message pipeline (without database - batch processing disabled)
    pub fn new(llm_provider: Arc<dyn LlmProvider>) -> Self {
        let analyzer = UnifiedAnalyzer::new(llm_provider);
        Self {
            analyzer,
            pool: None,
        }
    }

    /// Create message pipeline with database pool (enables batch processing)
    pub fn with_pool(llm_provider: Arc<dyn LlmProvider>, pool: SqlitePool) -> Self {
        let analyzer = UnifiedAnalyzer::new(llm_provider);
        Self {
            analyzer,
            pool: Some(pool),
        }
    }

    /// Create message pipeline with custom configuration
    pub fn with_config(
        llm_provider: Arc<dyn LlmProvider>,
        analyzer_config: AnalyzerConfig,
    ) -> Self {
        let analyzer = UnifiedAnalyzer::with_config(llm_provider, analyzer_config);
        Self {
            analyzer,
            pool: None,
        }
    }

    /// Create message pipeline with custom configuration and database pool
    pub fn with_config_and_pool(
        llm_provider: Arc<dyn LlmProvider>,
        analyzer_config: AnalyzerConfig,
        pool: SqlitePool,
    ) -> Self {
        let analyzer = UnifiedAnalyzer::with_config(llm_provider, analyzer_config);
        Self {
            analyzer,
            pool: Some(pool),
        }
    }

    /// Main analysis entry point
    pub async fn analyze_message(
        &self,
        content: &str,
        role: &str,
        context: Option<&str>,
    ) -> Result<MessagePipelineResult> {
        info!("Processing message through unified pipeline: role={}", role);

        let analysis_result = self
            .analyzer
            .analyze_message(content, role, context)
            .await
            .map_err(|e| {
                error!("Analysis failed: {}", e);
                e
            })?;

        debug!(
            "Analysis complete: salience={}, is_code={}",
            analysis_result.salience, analysis_result.is_code
        );

        let pipeline_result = MessagePipelineResult {
            analysis: analysis_result.clone(),
            should_embed: analysis_result.routing.should_embed,
        };

        info!(
            "Message processing complete: should_embed={}",
            pipeline_result.should_embed
        );

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
    ///
    /// Finds messages in memory_entries that don't have corresponding message_analysis
    /// records, analyzes them, and stores the results.
    pub async fn process_pending_messages(&self, session_id: &str) -> Result<usize> {
        let pool = match &self.pool {
            Some(p) => p,
            None => {
                debug!("No database pool available for batch processing");
                return Ok(0);
            }
        };

        // Find pending messages (those without analysis)
        let pending_messages = sqlx::query!(
            r#"
            SELECT m.id, m.content, m.role
            FROM memory_entries m
            LEFT JOIN message_analysis ma ON m.id = ma.memory_entry_id
            WHERE m.session_id = ? AND ma.id IS NULL
            ORDER BY m.created_at ASC
            LIMIT 10
            "#,
            session_id
        )
        .fetch_all(pool)
        .await?;

        if pending_messages.is_empty() {
            return Ok(0);
        }

        info!(
            "Processing {} pending messages for session {}",
            pending_messages.len(),
            session_id
        );

        let mut processed_count = 0;

        for msg in pending_messages {
            let msg_id = msg.id.unwrap_or(0);
            let content = msg.content;
            let role = msg.role;

            // Skip very short messages
            if content.len() < 10 {
                debug!("Skipping short message {}", msg_id);
                continue;
            }

            // Analyze the message
            match self.analyze_message(&content, &role, None).await {
                Ok(result) => {
                    // Convert to storage format
                    let analysis = result.to_storage_analysis();

                    // Store the analysis
                    let topics_json =
                        serde_json::to_string(&analysis.topics.unwrap_or_default()).ok();
                    let routed_heads_json = analysis
                        .routed_to_heads
                        .and_then(|h| serde_json::to_string(&h).ok());
                    let analyzed_at = chrono::Utc::now().timestamp();

                    match sqlx::query!(
                        r#"
                        INSERT INTO message_analysis (
                            memory_entry_id, mood, intensity, salience, original_salience,
                            intent, topics, summary, relationship_impact, language,
                            contains_code, contains_error, error_type, error_severity,
                            error_file, programming_lang, routed_to_heads, analyzed_at
                        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                        "#,
                        msg_id,
                        analysis.mood,
                        analysis.intensity,
                        analysis.salience,
                        analysis.original_salience,
                        analysis.intent,
                        topics_json,
                        analysis.summary,
                        analysis.relationship_impact,
                        analysis.language,
                        analysis.contains_code,
                        analysis.contains_error,
                        analysis.error_type,
                        analysis.error_severity,
                        analysis.error_file,
                        analysis.programming_lang,
                        routed_heads_json,
                        analyzed_at,
                    )
                    .execute(pool)
                    .await
                    {
                        Ok(_) => {
                            processed_count += 1;
                            debug!("Stored analysis for message {}", msg_id);
                        }
                        Err(e) => {
                            warn!("Failed to store analysis for message {}: {}", msg_id, e);
                        }
                    }
                }
                Err(e) => {
                    warn!("Failed to analyze message {}: {}", msg_id, e);
                }
            }
        }

        if processed_count > 0 {
            info!(
                "Processed {} messages for session {}",
                processed_count, session_id
            );
        }

        Ok(processed_count)
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
            routed_to_heads: None, // LLM's routed_to_heads used directly from structured response
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
pub use analyzers::unified::{AnalyzerConfig, RoutingDecision, UnifiedAnalysisResult};

pub type UnifiedAnalysis = UnifiedAnalysisResult;
pub type PipelineConfig = AnalyzerConfig;
