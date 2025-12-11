// src/memory/features/message_pipeline/analyzers/chat_analyzer.rs
// Chat message analyzer - extracts sentiment, intent, topics, code, and error detection
// FIXED: More explicit prompts for consistent LLM behavior

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use tracing::{debug, error, info};

use crate::llm_compat::provider::{LlmProvider, Message};
use crate::memory::features::prompts::analysis as prompts;

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
        let messages = vec![Message::user(prompt)];

        // Use LLM for analysis
        let system = prompts::MESSAGE_ANALYZER;

        let provider_response = self
            .llm_provider
            .chat(messages, system.to_string())
            .await
            .map_err(|e| {
                error!("LLM analysis failed: {}", e);
                e
            })?;

        self.parse_analysis_response(&provider_response.content, content)
            .await
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
        let llm_messages = vec![Message::user(prompt)];

        // Use LLM for batch analysis
        let system = prompts::BATCH_ANALYZER;

        let provider_response = self
            .llm_provider
            .chat(llm_messages, system.to_string())
            .await
            .map_err(|e| {
                error!("LLM batch analysis failed: {}", e);
                e
            })?;

        self.parse_batch_response(&provider_response.content, messages)
            .await
    }

    fn build_analysis_prompt(&self, content: &str, role: &str, context: Option<&str>) -> String {
        let context_str = context
            .map(|c| format!("\n\nPrevious Context:\n{}", c))
            .unwrap_or_default();

        format!(
            r#"Analyze this {} message and extract metadata:

Message: "{}"{}

Return JSON with these fields:

1. salience (0.0-1.0): How important/memorable this message is
   - CRITICAL: Trivial acknowledgments like "ok", "thanks", "got it", "👍", "k" MUST have salience < 0.3
   - Low value (0.0-0.3): Simple acknowledgments, greetings, filler
   - Medium value (0.4-0.6): Questions, casual discussion
   - High value (0.7-0.9): Technical content, decisions, bugs, errors
   - Critical (0.9-1.0): Security issues, production problems, major decisions

2. topics: Array of main topics discussed (2-5 topics)

3. contains_code: true if message contains actual code OR discusses programming/code
   - IMPORTANT: Messages like "Here's a bug fix in Rust" or "I need to implement OAuth" should have contains_code=true
   - Both code blocks AND discussions about code should be marked as code-related

4. programming_lang: Language name if code-related (rust/python/javascript/typescript/go/java/sql/etc)

5. contains_error: true if discussing an error, bug, or failure

6. error_type: Type if error present (compiler/runtime/test_failure/build_failure/linter/type_error)

7. error_file: Filename if mentioned in error context

8. error_severity: Severity if error (low/medium/high/critical)

9. mood: Emotional tone (excited/frustrated/neutral/curious/confused/etc)

10. intensity: Emotional intensity 0.0-1.0

11. intent: What user wants (question/statement/request/complaint/instruction/etc)

12. summary: Brief 1-sentence summary if notable (null for trivial messages)

13. relationship_impact: How this affects user-assistant relationship (null for trivial messages)

Examples:
- "ok" → salience: 0.1, topics: ["acknowledgment"], contains_code: false
- "Here's a bug fix in Rust" → salience: 0.8, topics: ["bug fix", "rust"], contains_code: true, programming_lang: "rust"
- "Critical auth bug in production" → salience: 0.95, contains_error: true, error_severity: "critical"

Be precise and consistent."#,
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

For each message, provide analysis as JSON array with same fields as single analysis.

CRITICAL RULES:
- Trivial messages ("ok", "thanks", "got it") MUST have salience < 0.3
- Messages about code (not just code blocks) should have contains_code=true
- Be consistent with salience scoring across all messages"#,
            messages.len(),
            message_list
        )
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

        let programming_lang = parsed.programming_lang.as_ref().and_then(|lang| {
            let lower = lang.to_lowercase();
            if matches!(
                lower.as_str(),
                "rust" | "typescript" | "javascript" | "python" | "go" | "java"
            ) {
                Some(lower)
            } else {
                None
            }
        });

        let error_type = parsed
            .error_type
            .as_ref()
            .filter(|t| {
                matches!(
                    t.to_lowercase().as_str(),
                    "compiler"
                        | "runtime"
                        | "test_failure"
                        | "build_failure"
                        | "linter"
                        | "type_error"
                )
            })
            .cloned();

        let error_severity = parsed
            .error_severity
            .as_ref()
            .filter(|s| matches!(s.to_lowercase().as_str(), "critical" | "warning" | "info"))
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
            if let Some((content, _)) =
                original_messages.get(analysis.message_index.saturating_sub(1))
            {
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
        // STRATEGY 0: Handle LLM structured output format
        // LLM returns: {"output": [{"type": "reasoning", ...}, {"type": "message", "content": [{"type": "output_text", "text": "..."}]}]}
        if let Ok(value) = serde_json::from_str::<Value>(response) {
            // Check for LLM output array format
            if let Some(output_array) = value.get("output").and_then(|o| o.as_array()) {
                debug!("Detected LLM structured response format");

                // Find the message object in the output array
                for item in output_array {
                    if item.get("type").and_then(|t| t.as_str()) == Some("message") {
                        // Navigate to content array
                        if let Some(content_array) = item.get("content").and_then(|c| c.as_array())
                        {
                            // Find output_text
                            for content_item in content_array {
                                if content_item.get("type").and_then(|t| t.as_str())
                                    == Some("output_text")
                                {
                                    if let Some(text) =
                                        content_item.get("text").and_then(|t| t.as_str())
                                    {
                                        debug!(
                                            "Extracted JSON from LLM output.content.text: {} chars",
                                            text.len()
                                        );
                                        return Ok(text.to_string());
                                    }
                                }
                            }
                        }
                    }
                }
                return Err(anyhow::anyhow!("No content in structured response"));
            }

            // If not LLM format but valid JSON, assume it's already the analysis
            debug!("Response is already valid JSON (non-LLM structured response)");
            return Ok(response.to_string());
        }

        // STRATEGY 1: Find JSON in markdown code blocks
        if let Some(opening_pos) = response.find("```") {
            let backtick_count = response[opening_pos..]
                .chars()
                .take_while(|&c| c == '`')
                .count();
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

        error!(
            "Failed to extract JSON from response. First 200 chars: {}",
            &response[..response.len().min(200)]
        );

        Err(anyhow::anyhow!("No valid JSON found in LLM response"))
    }
}
