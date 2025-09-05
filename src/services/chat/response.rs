// src/services/chat/response.rs
// Response processing logic for chat conversations with GPT-5 structured output integration
// Handles response creation, persistence, and summarization

use std::sync::Arc;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tracing::{debug, info, warn, error};

use crate::services::memory::MemoryService;
use crate::services::summarization::SummarizationService;
use crate::llm::client::OpenAIClient;
use crate::persona::PersonaOverlay;
use crate::api::error::IntoApiError;
use crate::config::CONFIG;

/// Response data structure - matches GPT-5 structured output
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatResponse {
    pub output: String,
    pub persona: String,
    pub mood: String,
    pub salience: usize,
    pub summary: String,
    pub memory_type: String,
    pub tags: Vec<String>,
    pub intent: Option<String>,
    pub monologue: Option<String>,
    pub reasoning_summary: Option<String>,
}

/// GPT-5 structured response format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GPT5StructuredResponse {
    pub output: String,
    pub metadata: ResponseMetadata,
    pub reasoning: Option<ReasoningData>,
}

/// Metadata returned by GPT-5
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseMetadata {
    pub mood: String,
    pub salience: f32,  // 0.0 to 1.0 from GPT-5
    pub summary: String,
    pub memory_type: String,
    pub tags: Vec<String>,
    pub intent: Option<String>,
}

/// Reasoning data from GPT-5
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningData {
    pub monologue: String,
    pub summary: String,
}

/// JSON validation and repair utilities
pub struct JsonValidator;

impl JsonValidator {
    /// Validate and repair JSON if needed
    pub fn validate_and_repair(json_str: &str) -> Result<Value> {
        // First attempt: direct parsing
        match serde_json::from_str::<Value>(json_str) {
            Ok(value) => return Ok(value),
            Err(e) => {
                debug!("Initial JSON parse failed: {}", e);
            }
        }
        
        // Second attempt: common repairs
        let repaired = Self::apply_common_repairs(json_str);
        match serde_json::from_str::<Value>(&repaired) {
            Ok(value) => {
                info!("JSON repaired successfully");
                return Ok(value);
            }
            Err(e) => {
                warn!("JSON repair failed: {}", e);
            }
        }
        
        // Return error if we can't fix it
        Err(anyhow::anyhow!("Invalid JSON that could not be repaired"))
    }
    
    /// Apply common JSON repairs
    fn apply_common_repairs(json_str: &str) -> String {
        let mut repaired = json_str.to_string();
        
        // Remove trailing commas before } or ]
        repaired = repaired.replace(",}", "}");
        repaired = repaired.replace(",]", "]");
        
        // Fix single quotes to double quotes
        // (careful not to break strings with apostrophes)
        if !repaired.contains('"') && repaired.contains('\'') {
            repaired = repaired.replace('\'', "\"");
        }
        
        // Ensure string is properly closed
        if repaired.chars().filter(|c| *c == '"').count() % 2 != 0 {
            repaired.push('"');
        }
        
        // Ensure JSON object/array is properly closed
        let open_braces = repaired.chars().filter(|c| *c == '{').count();
        let close_braces = repaired.chars().filter(|c| *c == '}').count();
        if open_braces > close_braces {
            repaired.push_str(&"}".repeat(open_braces - close_braces));
        }
        
        let open_brackets = repaired.chars().filter(|c| *c == '[').count();
        let close_brackets = repaired.chars().filter(|c| *c == ']').count();
        if open_brackets > close_brackets {
            repaired.push_str(&"]".repeat(open_brackets - close_brackets));
        }
        
        repaired
    }
    
    /// Check if JSON appears truncated
    pub fn is_truncated(json_str: &str) -> bool {
        let trimmed = json_str.trim();
        
        // Check for incomplete JSON endings
        if trimmed.is_empty() {
            return true;
        }
        
        // Check if it doesn't end with a closing character
        let last_char = trimmed.chars().last().unwrap();
        if !matches!(last_char, '}' | ']' | '"' | '0'..='9' | 'e' | 'l' | 'E' | 'L') {
            return true;
        }
        
        // Count braces/brackets to detect imbalance
        let open_braces = trimmed.chars().filter(|c| *c == '{').count();
        let close_braces = trimmed.chars().filter(|c| *c == '}').count();
        let open_brackets = trimmed.chars().filter(|c| *c == '[').count();
        let close_brackets = trimmed.chars().filter(|c| *c == ']').count();
        
        open_braces != close_braces || open_brackets != close_brackets
    }
}

/// Response processor for chat conversations
pub struct ResponseProcessor {
    memory_service: Arc<MemoryService>,
    summarizer: Arc<SummarizationService>,
    persona: PersonaOverlay,
    llm_client: Arc<OpenAIClient>,
}

impl ResponseProcessor {
    /// Create new response processor with LLM client for structured output
    pub fn new(
        memory_service: Arc<MemoryService>,
        summarizer: Arc<SummarizationService>,
        persona: PersonaOverlay,
        llm_client: Arc<OpenAIClient>,
    ) -> Self {
        Self {
            memory_service,
            summarizer,
            persona,
            llm_client,
        }
    }

    /// Persist user message to memory
    pub async fn persist_user_message(
        &self,
        session_id: &str,
        user_text: &str,
        project_id: Option<&str>,
    ) -> Result<()> {
        info!("Persisting user message for session: {}", session_id);
        
        self.memory_service
            .save_user_message(session_id, user_text, project_id)
            .await
            .into_api_error("Failed to persist user message")?;
        
        Ok(())
    }

    /// Process and create response structure with GPT-5 structured output
    pub async fn process_response(
        &self,
        session_id: &str,
        content: String,
        context: &crate::memory::recall::RecallContext,
    ) -> Result<ChatResponse> {
        info!("Processing response with GPT-5 structured output for session: {}", session_id);

        // Request structured metadata from GPT-5
        let structured_response = self.get_structured_metadata(
            &content,
            context,
            session_id
        ).await?;
        
        // Convert GPT-5 response to our ChatResponse format
        let response = ChatResponse {
            output: structured_response.output,
            persona: self.persona.to_string(),
            mood: structured_response.metadata.mood,
            salience: (structured_response.metadata.salience * 10.0) as usize, // Convert 0-1 to 0-10
            summary: structured_response.metadata.summary,
            memory_type: structured_response.metadata.memory_type,
            tags: structured_response.metadata.tags,
            intent: structured_response.metadata.intent,
            monologue: structured_response.reasoning.as_ref().map(|r| r.monologue.clone()),
            reasoning_summary: structured_response.reasoning.map(|r| r.summary),
        };

        // Persist the AI's response to memory with metadata
        self.persist_ai_response(session_id, &response, project_id).await?;
        
        Ok(response)
    }

    /// Get structured metadata from GPT-5
    async fn get_structured_metadata(
        &self,
        content: &str,
        context: &crate::memory::recall::RecallContext,
        session_id: &str,
    ) -> Result<GPT5StructuredResponse> {
        let instructions = self.build_metadata_instructions(context);
        
        let input = json!([
            {
                "role": "user",
                "content": [{
                    "type": "input_text",
                    "text": format!(
                        "Analyze this AI response and provide structured metadata:\n\n{}\n\n{}",
                        content,
                        instructions
                    )
                }]
            }
        ]);

        // Use proper token limit for JSON responses
        let max_tokens = CONFIG.max_json_output_tokens.unwrap_or(2000);
        
        let request_body = json!({
            "model": CONFIG.gpt5_model,
            "input": input,
            "text": {
                "format": "json_object",
                "verbosity": CONFIG.verbosity
            },
            "parameters": {
                "verbosity": CONFIG.verbosity,
                "reasoning_effort": "minimal", // Minimal for metadata extraction
                "max_output_tokens": max_tokens
            }
        });

        // Make request with retry logic for truncation
        let mut attempts = 0;
        let max_attempts = 3;
        let mut accumulated_json = String::new();
        
        loop {
            attempts += 1;
            
            let response = if accumulated_json.is_empty() {
                // Initial request
                self.llm_client.post_response(request_body.clone()).await?
            } else {
                // Continuation request for truncated JSON
                let continuation_input = json!([
                    {
                        "role": "user",
                        "content": [{
                            "type": "input_text",
                            "text": format!(
                                "Continue this truncated JSON from where it left off:\n{}",
                                accumulated_json
                            )
                        }]
                    }
                ]);
                
                let continuation_body = json!({
                    "model": CONFIG.gpt5_model,
                    "input": continuation_input,
                    "text": {
                        "format": "json_object",
                        "verbosity": CONFIG.verbosity
                    },
                    "parameters": {
                        "verbosity": CONFIG.verbosity,
                        "reasoning_effort": "minimal",
                        "max_output_tokens": max_tokens
                    }
                });
                
                self.llm_client.post_response(continuation_body).await?
            };
            
            // Extract text content
            let text_content = crate::llm::client::responses::extract_text_from_responses(&response)
                .ok_or_else(|| anyhow::anyhow!("No text content in GPT-5 response"))?;
            
            accumulated_json.push_str(&text_content);
            
            // Check if JSON is complete
            if !JsonValidator::is_truncated(&accumulated_json) {
                break;
            }
            
            if attempts >= max_attempts {
                warn!("Max continuation attempts reached, attempting repair");
                break;
            }
            
            info!("JSON appears truncated, requesting continuation (attempt {}/{})", attempts, max_attempts);
        }
        
        // Validate and parse JSON
        let json_value = JsonValidator::validate_and_repair(&accumulated_json)?;
        
        // Parse into structured response
        let structured_response: GPT5StructuredResponse = serde_json::from_value(json_value)
            .map_err(|e| {
                error!("Failed to parse GPT-5 structured response: {}", e);
                anyhow::anyhow!("Invalid structured response format: {}", e)
            })?;
        
        Ok(structured_response)
    }

    /// Build instructions for metadata extraction
    fn build_metadata_instructions(&self, context: &crate::memory::recall::RecallContext) -> String {
        let mut instructions = String::from(
            "Return a JSON object with the following structure:\n\
            {\n\
              \"output\": \"the original response text\",\n\
              \"metadata\": {\n\
                \"mood\": \"emotional tone (e.g., helpful, curious, concerned, enthusiastic)\",\n\
                \"salience\": 0.0-1.0 importance score,\n\
                \"summary\": \"one-sentence summary\",\n\
                \"memory_type\": \"conversational|technical|personal|reference\",\n\
                \"tags\": [\"relevant\", \"topic\", \"tags\"],\n\
                \"intent\": \"optional user intent if detectable\"\n\
              },\n\
              \"reasoning\": {\n\
                \"monologue\": \"internal thought process\",\n\
                \"summary\": \"reasoning summary\"\n\
              }\n\
            }"
        );
        
        // Add context hints if available
        if !context.recent.is_empty() {
            instructions.push_str("\n\nConsider recent conversation context for continuity.");
        }
        
        if !context.semantic.is_empty() {
            instructions.push_str("\n\nConsider semantic context for relevance scoring.");
        }
        
        instructions
    }

    /// Persist AI response with metadata
    async fn persist_ai_response(
        &self,
        session_id: &str,
        response: &ChatResponse,
        project_id: Option<&str>,
    ) -> Result<()> {
        debug!("Persisting AI response with metadata for session: {}", session_id);
        
        // Save to memory with full metadata
        self.memory_service
            .save_ai_response(
                session_id,
                &response.output,
                response.salience as f32 / 10.0, // Convert back to 0-1
                &response.tags,
                project_id
            )
            .await
            .into_api_error("Failed to persist AI response")?;
        
        Ok(())
    }

    /// Handle summarization if needed
    pub async fn handle_summarization(&self, session_id: &str) -> Result<()> {
        debug!("Checking if summarization needed for session: {}", session_id);
        
        // Delegate to summarization service
        if self.summarizer.should_summarize(session_id).await? {
            info!("Triggering summarization for session: {}", session_id);
            self.summarizer.summarize_conversation(session_id).await?;
        }
        
        Ok(())
    }

    /// Request GPT-5 to repair malformed JSON
    pub async fn repair_json_with_gpt5(&self, malformed_json: &str) -> Result<String> {
        let input = json!([
            {
                "role": "user",
                "content": [{
                    "type": "input_text",
                    "text": format!(
                        "Fix this malformed JSON and return only the corrected JSON:\n\n{}",
                        malformed_json
                    )
                }]
            }
        ]);

        let request_body = json!({
            "model": CONFIG.gpt5_model,
            "input": input,
            "text": {
                "format": "json_object",
                "verbosity": "minimal"
            },
            "parameters": {
                "verbosity": "minimal",
                "reasoning_effort": "minimal",
                "max_output_tokens": CONFIG.max_json_output_tokens.unwrap_or(2000)
            }
        });

        let response = self.llm_client.post_response(request_body).await?;
        
        let repaired_json = crate::llm::client::responses::extract_text_from_responses(&response)
            .ok_or_else(|| anyhow::anyhow!("No content in repair response"))?;
        
        Ok(repaired_json)
    }
}

// Extension methods for MemoryService to support metadata
impl MemoryService {
    /// Save AI response with metadata
    pub async fn save_ai_response(
        &self,
        session_id: &str,
        content: &str,
        salience: f32,
        tags: &[String],
        project_id: Option<&str>,
    ) -> Result<()> {
        // Implementation would go in the actual MemoryService
        // This is a placeholder showing the expected interface
        debug!("Saving AI response: session={}, salience={}, tags={:?}", 
               session_id, salience, tags);
        
        // Call existing save methods with metadata
        self.save_user_message(session_id, content, project_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_json_validator_repairs() {
        // Test trailing comma removal
        let malformed = r#"{"key": "value",}"#;
        let repaired = JsonValidator::apply_common_repairs(malformed);
        assert_eq!(repaired, r#"{"key": "value"}"#);
        
        // Test unclosed brace
        let malformed = r#"{"key": "value""#;
        let repaired = JsonValidator::apply_common_repairs(malformed);
        assert_eq!(repaired, r#"{"key": "value"}"#);
    }
    
    #[test]
    fn test_truncation_detection() {
        assert!(JsonValidator::is_truncated(r#"{"key": "val"#));
        assert!(JsonValidator::is_truncated(r#"{"key": "#));
        assert!(!JsonValidator::is_truncated(r#"{"key": "value"}"#));
        assert!(!JsonValidator::is_truncated(r#"[]"#));
    }
}
