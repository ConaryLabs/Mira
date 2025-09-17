// src/memory/features/message_analyzer.rs

//! GPT-5-powered message analysis for enriching memory entries.
//! Analyzes messages for mood, intent, salience, topics, and more.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::info;

use crate::llm::client::OpenAIClient;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageAnalysis {
    pub mood: Option<String>,
    pub intensity: Option<f32>,
    pub salience: f32,
    pub intent: Option<String>,
    pub topics: Vec<String>,
    pub summary: Option<String>,
    pub relationship_impact: Option<String>,
    pub contains_code: bool,
    pub programming_lang: Option<String>,
}

/// Analyzes messages using GPT-5 to extract rich metadata
pub struct MessageAnalyzer {
    llm_client: Arc<OpenAIClient>,
}

impl MessageAnalyzer {
    pub fn new(llm_client: Arc<OpenAIClient>) -> Self {
        Self { llm_client }
    }

    /// Analyze a single message with GPT-5
    pub async fn analyze_message(
        &self,
        content: &str,
        role: &str,
        context: Option<&str>,
    ) -> Result<MessageAnalysis> {
        let prompt = self.build_analysis_prompt(content, role, context);
        
        // Use summarize_conversation which is what OpenAIClient actually has
        let response_text: String = self.llm_client
            .summarize_conversation(&prompt, 500)
            .await?;
        
        self.parse_analysis_response(&response_text)
    }

    /// Batch analyze multiple messages for efficiency
    pub async fn analyze_batch(
        &self,
        messages: Vec<(String, String)>, // (content, role)
    ) -> Result<Vec<MessageAnalysis>> {
        if messages.is_empty() {
            return Ok(Vec::new());
        }

        info!("Batch analyzing {} messages", messages.len());
        
        let prompt = self.build_batch_analysis_prompt(&messages);
        let response_text: String = self.llm_client
            .summarize_conversation(&prompt, 2000)
            .await?;
        
        self.parse_batch_analysis_response(&response_text)
    }

    fn build_analysis_prompt(&self, content: &str, role: &str, context: Option<&str>) -> String {
        format!(
            r#"Analyze the following {} message and provide a JSON response with these fields:
- mood: string or null (emotional tone: happy, sad, angry, neutral, excited, etc.)
- intensity: float 0-1 or null (emotional intensity)
- salience: float 0-10 (importance for future recall, 10 being most important)
- intent: string or null (user's goal: question, statement, command, etc.)
- topics: array of strings (key topics/domains discussed)
- summary: string or null (one-line summary if message is long)
- relationship_impact: string or null (if this affects the user-assistant relationship)
- contains_code: boolean
- programming_lang: string or null (if contains_code is true)

Context: {}
Message: {}

Respond with valid JSON only."#,
            role,
            context.unwrap_or("Start of conversation"),
            content
        )
    }

    fn build_batch_analysis_prompt(&self, messages: &[(String, String)]) -> String {
        let messages_text = messages
            .iter()
            .enumerate()
            .map(|(i, (content, role))| format!("Message {}: [{}] {}", i + 1, role, content))
            .collect::<Vec<_>>()
            .join("\n\n");

        format!(
            r#"Analyze these {} messages and provide a JSON array response. Each element should have:
- mood, intensity, salience (0-10), intent, topics, summary, relationship_impact, contains_code, programming_lang

Messages:
{}

Respond with a JSON array matching the message order."#,
            messages.len(),
            messages_text
        )
    }

    fn parse_analysis_response(&self, response: &str) -> Result<MessageAnalysis> {
        // Try to extract JSON from the response
        let json_str = if response.contains('{') && response.contains('}') {
            let start = response.find('{').unwrap();
            let end = response.rfind('}').unwrap() + 1;
            &response[start..end]
        } else {
            response
        };

        serde_json::from_str(json_str)
            .map_err(|e| anyhow::anyhow!("Failed to parse analysis response: {}", e))
    }

    fn parse_batch_analysis_response(&self, response: &str) -> Result<Vec<MessageAnalysis>> {
        // Extract JSON array
        let json_str = if response.contains('[') && response.contains(']') {
            let start = response.find('[').unwrap();
            let end = response.rfind(']').unwrap() + 1;
            &response[start..end]
        } else {
            response
        };

        serde_json::from_str(json_str)
            .map_err(|e| anyhow::anyhow!("Failed to parse batch analysis response: {}", e))
    }
}

/// Analysis orchestrator that coordinates with other components
pub struct AnalysisService {
    analyzer: Arc<MessageAnalyzer>,
    sqlite_store: Arc<crate::memory::storage::sqlite::store::SqliteMemoryStore>,
}

impl AnalysisService {
    pub fn new(
        llm_client: Arc<OpenAIClient>,
        sqlite_store: Arc<crate::memory::storage::sqlite::store::SqliteMemoryStore>,
    ) -> Self {
        Self {
            analyzer: Arc::new(MessageAnalyzer::new(llm_client)),
            sqlite_store,
        }
    }

    /// Process unanalyzed messages in batches
    pub async fn process_pending_messages(&self, session_id: &str) -> Result<usize> {
        // Query for unanalyzed messages
        let pending = self.get_unanalyzed_messages(session_id, 10).await?;
        
        if pending.is_empty() {
            return Ok(0);
        }

        info!("Processing {} pending messages for analysis", pending.len());

        // Prepare for batch analysis
        let messages: Vec<(String, String)> = pending
            .iter()
            .map(|entry| (entry.content.clone(), entry.role.clone()))
            .collect();

        // Analyze in batch
        let analyses = self.analyzer.analyze_batch(messages).await?;

        // Store analysis results
        for (entry, analysis) in pending.iter().zip(analyses.iter()) {
            self.store_analysis(entry.id.unwrap(), analysis).await?;
        }

        Ok(analyses.len())
    }

    async fn get_unanalyzed_messages(
        &self,
        _session_id: &str,
        _limit: usize,
    ) -> Result<Vec<crate::memory::core::types::MemoryEntry>> {
        // Query SQLite for messages without analysis
        // This would be a custom query - for now, returning empty
        Ok(Vec::new())
    }

    async fn store_analysis(
        &self,
        message_id: i64,
        analysis: &MessageAnalysis,
    ) -> Result<()> {
        // Store in message_analysis table
        let analysis_data = crate::memory::storage::sqlite::store::MessageAnalysis {
            mood: analysis.mood.clone(),
            intensity: analysis.intensity,
            salience: Some(analysis.salience),
            intent: analysis.intent.clone(),
            topics: Some(analysis.topics.clone()),
            summary: analysis.summary.clone(),
            relationship_impact: analysis.relationship_impact.clone(),
            contains_code: Some(analysis.contains_code),
            language: Some("en".to_string()),
            programming_lang: analysis.programming_lang.clone(),
            analysis_version: Some("1.0".to_string()),
            routed_to_heads: None,
        };

        self.sqlite_store
            .save_analysis(message_id, &analysis_data)
            .await
    }
}
