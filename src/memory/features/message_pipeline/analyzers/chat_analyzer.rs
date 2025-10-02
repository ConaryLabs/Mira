// src/memory/features/message_pipeline/analyzers/chat_analyzer.rs

//! Chat message analyzer - extracts sentiment, intent, and topics from conversational content
//! 
//! Uses LLM-based analysis with structured JSON responses for chat message classification.
//! This is separate from code analysis which uses AST parsing via CodeIntelligenceService.

use std::sync::Arc;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use tracing::{debug, info, error};

use crate::llm::client::OpenAIClient;

// ===== CHAT ANALYSIS RESULT =====

/// Result from chat-specific analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatAnalysisResult {
    // Core classification
    pub salience: f32,
    pub topics: Vec<String>,
    
    // Sentiment and mood analysis
    pub mood: Option<String>,
    pub intensity: Option<f32>,
    
    // Intent and meaning
    pub intent: Option<String>,
    pub summary: Option<String>, 
    pub relationship_impact: Option<String>,
    
    // Content for analysis (needed for other analyzers)
    pub content: String,
    
    // Processing metadata
    pub processed_at: chrono::DateTime<chrono::Utc>,
}

// ===== CHAT ANALYZER =====

pub struct ChatAnalyzer {
    llm_client: Arc<OpenAIClient>,
}

impl ChatAnalyzer {
    pub fn new(llm_client: Arc<OpenAIClient>) -> Self {
        Self { llm_client }
    }
    
    /// Analyze chat message content
    pub async fn analyze(
        &self,
        content: &str,
        role: &str,
        context: Option<&str>,
    ) -> Result<ChatAnalysisResult> {
        debug!("Analyzing {} message: {} chars", role, content.len());
        
        let prompt = self.build_analysis_prompt(content, role, context);
        
        let response = self.llm_client
            .summarize_conversation(&prompt, 500)
            .await
            .map_err(|e| {
                error!("LLM analysis failed: {}", e);
                e
            })?;
        
        self.parse_analysis_response(&response, content).await
    }
    
    /// Batch analyze multiple chat messages
    pub async fn analyze_batch(
        &self,
        messages: &[(String, String)], // (content, role) pairs
    ) -> Result<Vec<ChatAnalysisResult>> {
        if messages.is_empty() {
            return Ok(Vec::new());
        }
        
        info!("Batch analyzing {} messages", messages.len());
        
        let prompt = self.build_batch_prompt(messages);
        let response = self.llm_client
            .summarize_conversation(&prompt, 2000)
            .await?;
        
        self.parse_batch_response(&response, messages).await
    }
    
    // ===== PROMPT BUILDING =====
    
    /// Build analysis prompt for single message
    fn build_analysis_prompt(&self, content: &str, role: &str, context: Option<&str>) -> String {
        let context_section = context
            .map(|ctx| format!("**Context:** {}\n\n", ctx))
            .unwrap_or_default();
        
        format!(
            r#"{context_section}**Message to analyze:**
Role: {role}  
Content: "{content}"

Analyze this message and provide:

1. **Salience** (0.0-1.0): How important is this for memory storage?
2. **Topics** (list): Main topics/themes discussed  
3. **Mood** (optional): Overall emotional tone
4. **Intensity** (0.0-1.0): Emotional intensity level
5. **Intent** (optional): What the user is trying to accomplish
6. **Summary** (1-2 sentences): Key content summary
7. **Relationship Impact** (optional): How this affects user-assistant relationship

Respond with valid JSON:
```json
{{
  "salience": 0.75,
  "topics": ["topic1", "topic2"],
  "mood": "neutral",
  "intensity": 0.5,
  "intent": "inform",
  "summary": "brief summary",
  "relationship_impact": null
}}
```

Rate salience based on:
- Information value for future conversations  
- Technical complexity or importance
- User goals and problem-solving needs
- Relationship development moments"#
        )
    }
    
    /// Build batch analysis prompt for multiple messages
    fn build_batch_prompt(&self, messages: &[(String, String)]) -> String {
        let message_list = messages
            .iter()
            .enumerate()
            .map(|(i, (content, role))| format!("{}. [{}]: \"{}\"", i + 1, role, content))
            .collect::<Vec<_>>()
            .join("\n");
        
        format!(
            r#"Batch analyze these {} messages:

{}

For each message, provide analysis as JSON array:
```json
[
  {{
    "message_index": 1,
    "salience": 0.75,
    "topics": ["topic1"],
    "mood": "neutral",
    "intensity": 0.5,
    "intent": "inform",
    "summary": "brief summary",
    "relationship_impact": null
  }}
]
```

Rate salience (0.0-1.0) based on future conversation value, technical importance, and user goals."#,
            messages.len(),
            message_list
        )
    }
    
    // ===== RESPONSE PARSING =====
    
    /// Parse analysis response from LLM
    async fn parse_analysis_response(
        &self,
        response: &str,
        original_content: &str,
    ) -> Result<ChatAnalysisResult> {
        
        let json_str = self.extract_json_from_response(response)?;
        
        #[derive(Deserialize)]
        struct LLMResponse {
            salience: f32,
            topics: Vec<String>,
            mood: Option<String>,
            intensity: Option<f32>,
            intent: Option<String>,
            summary: Option<String>,
            relationship_impact: Option<String>,
        }
        
        let parsed: LLMResponse = serde_json::from_str(&json_str)
            .map_err(|e| anyhow::anyhow!("Failed to parse LLM response: {}", e))?;
        
        let salience = parsed.salience.clamp(0.0, 1.0);
        
        let topics: Vec<String> = parsed.topics
            .into_iter()
            .filter(|t| !t.trim().is_empty())
            .map(|t| t.trim().to_lowercase())
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();
        
        Ok(ChatAnalysisResult {
            salience,
            topics,
            mood: parsed.mood.filter(|s| !s.trim().is_empty()),
            intensity: parsed.intensity.map(|i| i.clamp(0.0, 1.0)),
            intent: parsed.intent.filter(|s| !s.trim().is_empty()),
            summary: parsed.summary.filter(|s| !s.trim().is_empty()),
            relationship_impact: parsed.relationship_impact.filter(|s| !s.trim().is_empty()),
            content: original_content.to_string(),
            processed_at: chrono::Utc::now(),
        })
    }
    
    /// Parse batch response from LLM
    async fn parse_batch_response(
        &self,
        response: &str,
        original_messages: &[(String, String)],
    ) -> Result<Vec<ChatAnalysisResult>> {
        
        let json_str = self.extract_json_from_response(response)?;
        
        #[derive(Deserialize)]
        struct BatchLLMResponse {
            message_index: usize,
            salience: f32,
            topics: Vec<String>,
            mood: Option<String>,
            intensity: Option<f32>,
            intent: Option<String>,
            summary: Option<String>,
            relationship_impact: Option<String>,
        }
        
        let parsed: Vec<BatchLLMResponse> = serde_json::from_str(&json_str)
            .map_err(|e| anyhow::anyhow!("Failed to parse batch LLM response: {}", e))?;
        
        let mut results = Vec::with_capacity(original_messages.len());
        
        for (i, (content, _role)) in original_messages.iter().enumerate() {
            if let Some(analysis) = parsed.iter().find(|a| a.message_index == i + 1) {
                
                let salience = analysis.salience.clamp(0.0, 1.0);
                let topics: Vec<String> = analysis.topics
                    .iter()
                    .filter(|t| !t.trim().is_empty())
                    .map(|t| t.trim().to_lowercase())
                    .collect::<std::collections::HashSet<_>>()
                    .into_iter()
                    .collect();
                
                results.push(ChatAnalysisResult {
                    salience,
                    topics,
                    mood: analysis.mood.as_ref().filter(|s| !s.trim().is_empty()).cloned(),
                    intensity: analysis.intensity.map(|i| i.clamp(0.0, 1.0)),
                    intent: analysis.intent.as_ref().filter(|s| !s.trim().is_empty()).cloned(),
                    summary: analysis.summary.as_ref().filter(|s| !s.trim().is_empty()).cloned(),
                    relationship_impact: analysis.relationship_impact.as_ref().filter(|s| !s.trim().is_empty()).cloned(),
                    content: content.clone(),
                    processed_at: chrono::Utc::now(),
                });
            } else {
                // Fallback for missing analysis
                results.push(ChatAnalysisResult {
                    salience: 0.1,
                    topics: vec!["general".to_string()],
                    mood: None,
                    intensity: None,
                    intent: None,
                    summary: None,
                    relationship_impact: None,
                    content: content.clone(),
                    processed_at: chrono::Utc::now(),
                });
            }
        }
        
        Ok(results)
    }
    
    /// Extract JSON from LLM response (handles various formatting)
    fn extract_json_from_response(&self, response: &str) -> Result<String> {
        // Look for JSON block markers
        if let Some(start) = response.find("```json") {
            let json_start = start + 7;
            if let Some(end) = response[json_start..].find("```") {
                return Ok(response[json_start..json_start + end].trim().to_string());
            }
        }
        
        // Look for direct JSON (starts with { or [)
        if let Some(start) = response.find('{') {
            if let Some(end) = response.rfind('}') {
                if end > start {
                    return Ok(response[start..=end].to_string());
                }
            }
        }
        
        if let Some(start) = response.find('[') {
            if let Some(end) = response.rfind(']') {
                if end > start {
                    return Ok(response[start..=end].to_string());
                }
            }
        }
        
        Err(anyhow::anyhow!("Could not extract JSON from LLM response"))
    }
}
