// src/memory/features/message_pipeline/analyzers/chat_analyzer.rs

//! Chat message analyzer - extracts sentiment, intent, topics, code, and error detection
//! 
//! Uses LLM-based analysis with structured JSON responses for complete message classification.
//! Code and error detection handled by LLM - no regex heuristics.

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
    
    // Code detection (LLM-determined)
    pub contains_code: Option<bool>,
    pub programming_lang: Option<String>,
    
    // Error detection (LLM-determined)
    pub contains_error: Option<bool>,
    pub error_type: Option<String>,
    pub error_file: Option<String>,
    pub error_severity: Option<String>,
    
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
    
    /// Build analysis prompt - LLM detects code and errors
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
3. **Contains Code** (boolean): Actual code blocks/snippets? NOT just technical terms.
4. **Programming Language** (string or null): If contains_code=true, ONE of: 'rust', 'typescript', 'javascript', 'python', 'go', 'java'. Otherwise null.
5. **Contains Error** (boolean): Actual error needing fixing (compiler error, runtime error, stack trace, build failure)? NOT just discussing errors.
6. **Error Type** (string or null): If contains_error=true, ONE of: 'compiler', 'runtime', 'test_failure', 'build_failure', 'linter', 'type_error'. Otherwise null.
7. **Error File** (string or null): If contains_error=true and file path mentioned, extract it. Otherwise null.
8. **Error Severity** (string or null): If contains_error=true, rate as 'critical' (blocking), 'warning' (should fix), or 'info' (minor). Otherwise null.
9. **Mood** (optional): Overall emotional tone
10. **Intensity** (0.0-1.0): Emotional intensity level
11. **Intent** (optional): What the user is trying to accomplish
12. **Summary** (1-2 sentences): Key content summary
13. **Relationship Impact** (optional): How this affects user-assistant relationship

Respond with valid JSON:
```json
{{
  "salience": 0.75,
  "topics": ["topic1", "topic2"],
  "contains_code": false,
  "programming_lang": null,
  "contains_error": false,
  "error_type": null,
  "error_file": null,
  "error_severity": null,
  "mood": "neutral",
  "intensity": 0.5,
  "intent": "inform",
  "summary": "brief summary",
  "relationship_impact": null
}}
```

Guidelines:
- Code: Technical discussion ≠ code. Only true for actual code blocks/snippets.
- Errors: Discussing errors ≠ error. Only true for actual errors needing fixing.
- If unsure, set to null/false."#
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
    "contains_code": false,
    "programming_lang": null,
    "contains_error": false,
    "error_type": null,
    "error_file": null,
    "error_severity": null,
    "mood": "neutral",
    "intensity": 0.5,
    "intent": "inform",
    "summary": "brief summary",
    "relationship_impact": null
  }}
]
```

Guidelines:
- contains_code: only true if actual code, not technical discussion
- contains_error: only true if actual error needing fixing, not discussion about errors
- programming_lang: 'rust', 'typescript', 'javascript', 'python', 'go', 'java' or null
- error_type: 'compiler', 'runtime', 'test_failure', 'build_failure', 'linter', 'type_error' or null
- error_severity: 'critical', 'warning', 'info' or null"#,
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
            contains_code: Option<bool>,
            programming_lang: Option<String>,
            contains_error: Option<bool>,
            error_type: Option<String>,
            error_file: Option<String>,
            error_severity: Option<String>,
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
        
        // Validate programming_lang
        let programming_lang = parsed.programming_lang
            .filter(|lang| {
                matches!(
                    lang.to_lowercase().as_str(),
                    "rust" | "typescript" | "javascript" | "python" | "go" | "java"
                )
            });
        
        // Validate error_type
        let error_type = parsed.error_type
            .filter(|t| {
                matches!(
                    t.to_lowercase().as_str(),
                    "compiler" | "runtime" | "test_failure" | "build_failure" | "linter" | "type_error"
                )
            });
        
        // Validate error_severity
        let error_severity = parsed.error_severity
            .filter(|s| {
                matches!(
                    s.to_lowercase().as_str(),
                    "critical" | "warning" | "info"
                )
            });
        
        Ok(ChatAnalysisResult {
            salience,
            topics,
            contains_code: parsed.contains_code,
            programming_lang,
            contains_error: parsed.contains_error,
            error_type,
            error_file: parsed.error_file.filter(|s| !s.trim().is_empty()),
            error_severity,
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
            contains_code: Option<bool>,
            programming_lang: Option<String>,
            contains_error: Option<bool>,
            error_type: Option<String>,
            error_file: Option<String>,
            error_severity: Option<String>,
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
                
                // Validate fields
                let programming_lang = analysis.programming_lang.as_ref()
                    .filter(|lang| {
                        matches!(
                            lang.to_lowercase().as_str(),
                            "rust" | "typescript" | "javascript" | "python" | "go" | "java"
                        )
                    })
                    .cloned();
                
                let error_type = analysis.error_type.as_ref()
                    .filter(|t| {
                        matches!(
                            t.to_lowercase().as_str(),
                            "compiler" | "runtime" | "test_failure" | "build_failure" | "linter" | "type_error"
                        )
                    })
                    .cloned();
                
                let error_severity = analysis.error_severity.as_ref()
                    .filter(|s| {
                        matches!(
                            s.to_lowercase().as_str(),
                            "critical" | "warning" | "info"
                        )
                    })
                    .cloned();
                
                results.push(ChatAnalysisResult {
                    salience,
                    topics,
                    contains_code: analysis.contains_code,
                    programming_lang,
                    contains_error: analysis.contains_error,
                    error_type,
                    error_file: analysis.error_file.as_ref().filter(|s| !s.trim().is_empty()).cloned(),
                    error_severity,
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
                    contains_code: Some(false),
                    programming_lang: None,
                    contains_error: Some(false),
                    error_type: None,
                    error_file: None,
                    error_severity: None,
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
