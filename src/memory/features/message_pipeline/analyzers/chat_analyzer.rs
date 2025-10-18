// src/memory/features/message_pipeline/analyzers/chat_analyzer.rs
// Chat message analyzer - extracts sentiment, intent, topics, code, and error detection
// FIXED: Uses GPT-5 json_schema for structured outputs with fallback extraction

use std::sync::Arc;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tracing::{debug, info, error};

use crate::llm::provider::{LlmProvider, Message};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatAnalysisResult {
    pub salience: f32,
    pub topics: Vec<String>,
    pub contains_code: Option<bool>,
    pub programming_lang: Option<String>,
    pub contains_error: Option<bool>,
    pub error_type: Option<String>,
    pub error_file: Option<String>,
    pub error_severity: Option<String>,
    pub mood: Option<String>,
    pub intensity: Option<f32>,
    pub intent: Option<String>,
    pub summary: Option<String>, 
    pub relationship_impact: Option<String>,
    pub content: String,
    pub processed_at: chrono::DateTime<chrono::Utc>,
}

pub struct ChatAnalyzer {
    llm_provider: Arc<dyn LlmProvider>,
}

impl ChatAnalyzer {
    pub fn new(llm_provider: Arc<dyn LlmProvider>) -> Self {
        Self { llm_provider }
    }
    
    pub async fn analyze(
        &self,
        content: &str,
        role: &str,
        context: Option<&str>,
    ) -> Result<ChatAnalysisResult> {
        debug!("Analyzing {} message: {} chars", role, content.len());
        
        let prompt = self.build_analysis_prompt(content, role, context);
        let messages = vec![Message {
            role: "user".to_string(),
            content: prompt,
        }];
        
        // Use GPT-5 structured output if available
        if let Some(gpt5) = self.llm_provider.as_any().downcast_ref::<crate::llm::provider::gpt5::Gpt5Provider>() {
            let schema = Self::get_analysis_schema();
            let provider_response = gpt5
                .chat_with_schema(
                    messages,
                    "You are a precise message analyzer. Output only valid JSON matching the schema.".to_string(),
                    "chat_analysis",
                    schema,
                )
                .await
                .map_err(|e| {
                    error!("GPT-5 structured analysis failed: {}", e);
                    e
                })?;
            
            return self.parse_analysis_response(&provider_response.content, content).await;
        }
        
        // Fallback for other providers
        let provider_response = self.llm_provider
            .chat(
                messages,
                "You are a message analyzer. Return JSON only.".to_string(),
            )
            .await
            .map_err(|e| {
                error!("LLM analysis failed: {}", e);
                e
            })?;
        
        self.parse_analysis_response(&provider_response.content, content).await
    }
    
    pub async fn analyze_batch(
        &self,
        messages: &[(String, String)],
    ) -> Result<Vec<ChatAnalysisResult>> {
        if messages.is_empty() {
            return Ok(Vec::new());
        }
        
        info!("Batch analyzing {} messages", messages.len());
        
        let prompt = self.build_batch_prompt(messages);
        let llm_messages = vec![Message {
            role: "user".to_string(),
            content: prompt,
        }];
        
        // Use GPT-5 structured output if available
        if let Some(gpt5) = self.llm_provider.as_any().downcast_ref::<crate::llm::provider::gpt5::Gpt5Provider>() {
            let schema = Self::get_batch_analysis_schema();
            let provider_response = gpt5
                .chat_with_schema(
                    llm_messages,
                    "You are a precise message analyzer. Output only valid JSON matching the schema.".to_string(),
                    "batch_chat_analysis",
                    schema,
                )
                .await
                .map_err(|e| {
                    error!("GPT-5 batch structured analysis failed: {}", e);
                    e
                })?;
            
            return self.parse_batch_response(&provider_response.content, messages).await;
        }
        
        // Fallback for other providers
        let provider_response = self.llm_provider
            .chat(
                llm_messages,
                "You are a message analyzer. Return JSON only.".to_string(),
            )
            .await?;
        
        self.parse_batch_response(&provider_response.content, messages).await
    }
    
    fn build_analysis_prompt(&self, content: &str, role: &str, context: Option<&str>) -> String {
        let context_str = context
            .map(|c| format!("\n\nContext:\n{}", c))
            .unwrap_or_default();
        
        format!(
            r#"Analyze this {} message:
Content: "{}"{}

Return JSON with:
- salience: 0.0-1.0 (how important/memorable)
- topics: array of main topics
- contains_code: bool (if message has code)
- programming_lang: language if code present
- contains_error: bool (if discussing an error)
- error_type: compiler/runtime/test_failure/build_failure/linter/type_error
- error_file: filename if mentioned
- error_severity: low/medium/high/critical
- mood: emotional tone (excited/frustrated/neutral/curious/etc)
- intensity: 0.0-1.0 emotional intensity
- intent: what user wants (question/statement/request/etc)
- summary: brief summary if notable
- relationship_impact: how this affects user-assistant relationship"#,
            role, content, context_str
        )
    }
    
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

For each message, provide analysis as JSON array."#,
            messages.len(),
            message_list
        )
    }
    
    fn get_analysis_schema() -> Value {
        json!({
            "type": "object",
            "properties": {
                "salience": {
                    "type": "number",
                    "minimum": 0.0,
                    "maximum": 1.0
                },
                "topics": {
                    "type": "array",
                    "items": {"type": "string"}
                },
                "contains_code": {"type": ["boolean", "null"]},
                "programming_lang": {"type": ["string", "null"]},
                "contains_error": {"type": ["boolean", "null"]},
                "error_type": {"type": ["string", "null"]},
                "error_file": {"type": ["string", "null"]},
                "error_severity": {"type": ["string", "null"]},
                "mood": {"type": ["string", "null"]},
                "intensity": {
                    "type": ["number", "null"],
                    "minimum": 0.0,
                    "maximum": 1.0
                },
                "intent": {"type": ["string", "null"]},
                "summary": {"type": ["string", "null"]},
                "relationship_impact": {"type": ["string", "null"]}
            },
            "required": [
                "salience",
                "topics",
                "contains_code",
                "programming_lang",
                "contains_error",
                "error_type",
                "error_file",
                "error_severity",
                "mood",
                "intensity",
                "intent",
                "summary",
                "relationship_impact"
            ],
            "additionalProperties": false
        })
    }
    
    fn get_batch_analysis_schema() -> Value {
        json!({
            "type": "array",
            "items": {
                "type": "object",
                "properties": {
                    "message_index": {"type": "integer", "minimum": 1},
                    "salience": {
                        "type": "number",
                        "minimum": 0.0,
                        "maximum": 1.0
                    },
                    "topics": {
                        "type": "array",
                        "items": {"type": "string"}
                    },
                    "contains_code": {"type": ["boolean", "null"]},
                    "programming_lang": {"type": ["string", "null"]},
                    "contains_error": {"type": ["boolean", "null"]},
                    "error_type": {"type": ["string", "null"]},
                    "error_file": {"type": ["string", "null"]},
                    "error_severity": {"type": ["string", "null"]},
                    "mood": {"type": ["string", "null"]},
                    "intensity": {
                        "type": ["number", "null"],
                        "minimum": 0.0,
                        "maximum": 1.0
                    },
                    "intent": {"type": ["string", "null"]},
                    "summary": {"type": ["string", "null"]},
                    "relationship_impact": {"type": ["string", "null"]}
                },
                "required": [
                    "message_index",
                    "salience",
                    "topics",
                    "contains_code",
                    "programming_lang",
                    "contains_error",
                    "error_type",
                    "error_file",
                    "error_severity",
                    "mood",
                    "intensity",
                    "intent",
                    "summary",
                    "relationship_impact"
                ],
                "additionalProperties": false
            }
        })
    }
    
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
        let topics = if parsed.topics.is_empty() {
            vec!["general".to_string()]
        } else {
            parsed.topics
        };
        
        let programming_lang = parsed.programming_lang.as_ref()
            .filter(|lang| {
                matches!(
                    lang.to_lowercase().as_str(),
                    "rust" | "typescript" | "javascript" | "python" | "go" | "java"
                )
            })
            .cloned();
        
        let error_type = parsed.error_type.as_ref()
            .filter(|t| {
                matches!(
                    t.to_lowercase().as_str(),
                    "compiler" | "runtime" | "test_failure" | "build_failure" | "linter" | "type_error"
                )
            })
            .cloned();
        
        let error_severity = parsed.error_severity.as_ref()
            .filter(|s| {
                matches!(
                    s.to_lowercase().as_str(),
                    "critical" | "warning" | "info"
                )
            })
            .cloned();
        
        Ok(ChatAnalysisResult {
            salience,
            topics,
            contains_code: parsed.contains_code,
            programming_lang,
            contains_error: parsed.contains_error,
            error_type,
            error_file: parsed.error_file,
            error_severity,
            mood: parsed.mood,
            intensity: parsed.intensity.map(|i| i.clamp(0.0, 1.0)),
            intent: parsed.intent,
            summary: parsed.summary,
            relationship_impact: parsed.relationship_impact,
            content: original_content.to_string(),
            processed_at: chrono::Utc::now(),
        })
    }
    
    async fn parse_batch_response(
        &self,
        response: &str,
        original_messages: &[(String, String)],
    ) -> Result<Vec<ChatAnalysisResult>> {
        let json_str = self.extract_json_from_response(response)?;
        
        #[derive(Deserialize)]
        struct BatchItem {
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
        
        let analyses: Vec<BatchItem> = serde_json::from_str(&json_str)
            .map_err(|e| anyhow::anyhow!("Failed to parse batch response: {}", e))?;
        
        let mut results = Vec::new();
        
        for analysis in analyses {
            if let Some((content, _)) = original_messages.get(analysis.message_index.saturating_sub(1)) {
                let salience = analysis.salience.clamp(0.0, 1.0);
                let topics = if analysis.topics.is_empty() {
                    vec!["general".to_string()]
                } else {
                    analysis.topics
                };
                
                results.push(ChatAnalysisResult {
                    salience,
                    topics,
                    contains_code: analysis.contains_code,
                    programming_lang: analysis.programming_lang,
                    contains_error: analysis.contains_error,
                    error_type: analysis.error_type,
                    error_file: analysis.error_file,
                    error_severity: analysis.error_severity,
                    mood: analysis.mood,
                    intensity: analysis.intensity.map(|i| i.clamp(0.0, 1.0)),
                    intent: analysis.intent,
                    summary: analysis.summary,
                    relationship_impact: analysis.relationship_impact,
                    content: content.clone(),
                    processed_at: chrono::Utc::now(),
                });
            }
        }
        
        Ok(results)
    }
    
    fn extract_json_from_response(&self, response: &str) -> Result<String> {
        // STRATEGY 0: Try parsing entire response as JSON first (for structured responses like GPT-5 json_schema)
        if let Ok(_) = serde_json::from_str::<Value>(response) {
            debug!("Response is already valid JSON (structured response)");
            return Ok(response.to_string());
        }
        
        // STRATEGY 1: Find JSON in markdown code blocks
        if let Some(opening_pos) = response.find("```") {
            let backtick_count = response[opening_pos..].chars().take_while(|&c| c == '`').count();
            let after_backticks = &response[opening_pos + backtick_count..];
            
            if after_backticks.trim_start().starts_with("json") {
                let json_keyword_end = after_backticks.find("json").map(|i| i + 4).unwrap_or(0);
                let json_start = opening_pos + backtick_count + json_keyword_end;
                let closing_marker = "`".repeat(backtick_count);
                
                if let Some(relative_closing) = response[json_start..].find(&closing_marker) {
                    let json_end = json_start + relative_closing;
                    
                    if json_start < json_end && json_end <= response.len() {
                        let json_content = &response[json_start..json_end];
                        let trimmed = json_content.trim();
                        
                        if !trimmed.is_empty() && serde_json::from_str::<Value>(trimmed).is_ok() {
                            debug!("Extracted JSON from {} backtick code block", backtick_count);
                            return Ok(trimmed.to_string());
                        }
                    }
                }
            }
        }
        
        // STRATEGY 2: Look for raw JSON object
        if let Some(obj_start) = response.find('{') {
            if let Some(obj_end) = response.rfind('}') {
                if obj_start < obj_end {
                    let json_candidate = &response[obj_start..=obj_end];
                    if serde_json::from_str::<Value>(json_candidate).is_ok() {
                        debug!("Extracted raw JSON object");
                        return Ok(json_candidate.to_string());
                    }
                }
            }
        }
        
        // STRATEGY 3: Look for JSON array
        if let Some(arr_start) = response.find('[') {
            if let Some(arr_end) = response.rfind(']') {
                if arr_start < arr_end {
                    let json_candidate = &response[arr_start..=arr_end];
                    if serde_json::from_str::<Value>(json_candidate).is_ok() {
                        debug!("Extracted JSON array");
                        return Ok(json_candidate.to_string());
                    }
                }
            }
        }
        
        error!("Failed to extract JSON from response. First 200 chars: {}", 
               &response[..response.len().min(200)]);
        
        Err(anyhow::anyhow!("No valid JSON found in LLM response"))
    }
}
