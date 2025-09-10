// src/services/chat/response.rs
// Response processing logic for chat conversations with GPT-5 structured output integration
// Handles response creation, persistence, and summarization

use std::sync::Arc;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tracing::{info, warn, error};

use crate::memory::MemoryService;
use crate::memory::features::summarization::SummarizationEngine;
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
pub struct JsonValidator<'a> {
    llm_client: &'a Arc<OpenAIClient>,
}

impl<'a> JsonValidator<'a> {
    /// Create a new validator with an LLM client for repairs
    pub fn new(llm_client: &'a Arc<OpenAIClient>) -> Self {
        Self { llm_client }
    }

    /// Validate and repair JSON, using GPT-5 as a fallback
    pub async fn validate_and_repair(&self, json_str: &str) -> Result<Value> {
        if let Ok(value) = serde_json::from_str::<Value>(json_str) {
            return Ok(value);
        }

        let repaired_str = Self::apply_common_repairs(json_str);
        if let Ok(value) = serde_json::from_str::<Value>(&repaired_str) {
            info!("Successfully repaired malformed JSON locally.");
            return Ok(value);
        }

        if CONFIG.enable_json_validation {
            warn!("Local JSON repair failed. Falling back to GPT-5 for correction.");
            let gpt5_repaired_json = self.repair_json_with_gpt5(&repaired_str).await?;
            return serde_json::from_str::<Value>(&gpt5_repaired_json)
                .map_err(|e| {
                    error!("GPT-5 failed to repair JSON. Final error: {}", e);
                    anyhow::anyhow!("Invalid JSON that could not be repaired by GPT-5")
                });
        }
        
        error!("All JSON repair attempts failed for input: {}", json_str);
        Err(anyhow::anyhow!("Invalid JSON that could not be repaired"))
    }
    
    fn apply_common_repairs(json_str: &str) -> String {
        let mut repaired = json_str.trim().to_string();
        repaired = repaired.replace(",}", "}");
        repaired = repaired.replace(",]", "]");
        if repaired.starts_with('{') && !repaired.ends_with('}') {
            repaired.push('}');
        } else if repaired.starts_with('[') && !repaired.ends_with(']') {
            repaired.push(']');
        }
        repaired
    }
    
    pub fn is_truncated(json_str: &str) -> bool {
        let trimmed = json_str.trim();
        if trimmed.is_empty() { return false; }
        let open_braces = trimmed.chars().filter(|&c| c == '{').count();
        let close_braces = trimmed.chars().filter(|&c| c == '}').count();
        let open_brackets = trimmed.chars().filter(|&c| c == '[').count();
        let close_brackets = trimmed.chars().filter(|&c| c == ']').count();
        open_braces > close_braces || open_brackets > close_brackets
    }

    async fn repair_json_with_gpt5(&self, malformed_json: &str) -> Result<String> {
        let request_body = json!({
            "model": CONFIG.gpt5_model,
            "input": [{"role": "user", "content": [{"type": "input_text", "text": format!("Fix this malformed JSON and return only the corrected JSON object:\n\n{}", malformed_json)}]}],
            "text": { "format": "json_object", "verbosity": CONFIG.get_verbosity_for("metadata") },
            // FIX: Parameters are now top-level, not nested
            "verbosity": CONFIG.get_verbosity_for("metadata"),
            "reasoning_effort": CONFIG.get_reasoning_effort_for("metadata"),
            "max_output_tokens": CONFIG.get_json_max_tokens()
        });
        let response_value = self.llm_client.post_response(request_body).await?;
        // FIX: Correct import path
        crate::llm::client::extract_text_from_responses(&response_value)
            .ok_or_else(|| anyhow::anyhow!("GPT-5 repair returned no text content"))
    }
}

pub struct ResponseProcessor {
    memory_service: Arc<MemoryService>,
    summarizer: Arc<SummarizationEngine>,
    persona: PersonaOverlay,
    llm_client: Arc<OpenAIClient>,
}

impl ResponseProcessor {
    pub fn new(
        memory_service: Arc<MemoryService>,
        summarizer: Arc<SummarizationEngine>,
        persona: PersonaOverlay,
        llm_client: Arc<OpenAIClient>,
    ) -> Self {
        Self { memory_service, summarizer, persona, llm_client }
    }

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

    pub async fn process_response(
        &self,
        session_id: &str,
        content: String,
        context: &crate::memory::recall::RecallContext,
        _project_id: Option<&str>, // project_id is handled by save_assistant_response
    ) -> Result<ChatResponse> {
        info!("Processing response with GPT-5 structured output for session: {}", session_id);
        let structured_response = self.get_structured_metadata(&content, context).await?;
        
        let response = ChatResponse {
            output: structured_response.output,
            persona: self.persona.to_string(),
            mood: structured_response.metadata.mood,
            salience: (structured_response.metadata.salience * 10.0).round() as usize,
            summary: structured_response.metadata.summary,
            memory_type: structured_response.metadata.memory_type,
            tags: structured_response.metadata.tags,
            intent: structured_response.metadata.intent,
            monologue: structured_response.reasoning.as_ref().map(|r| r.monologue.clone()),
            reasoning_summary: structured_response.reasoning.map(|r| r.summary),
        };

        self.memory_service
            .save_assistant_response(session_id, &response)
            .await?;
        
        Ok(response)
    }

    async fn get_structured_metadata(
        &self,
        content: &str,
        context: &crate::memory::recall::RecallContext,
    ) -> Result<GPT5StructuredResponse> {
        let instructions = self.build_metadata_instructions(context);
        let mut accumulated_json = String::new();

        for attempt in 0..CONFIG.max_json_repair_attempts {
            let request_body = self.build_metadata_request(
                content, 
                &instructions, 
                if accumulated_json.is_empty() { None } else { Some(&accumulated_json) }
            );
            
            let response_value = self.llm_client.post_response(request_body).await?;
            // FIX: Correct import path
            let text_content = crate::llm::client::extract_text_from_responses(&response_value)
                .ok_or_else(|| anyhow::anyhow!("No text content in GPT-5 metadata response"))?;

            accumulated_json.push_str(&text_content);

            if !JsonValidator::is_truncated(&accumulated_json) {
                break;
            }
            if attempt >= CONFIG.max_json_repair_attempts - 1 {
                 warn!("Max continuation attempts reached for session. Proceeding with potentially truncated JSON.");
                 break;
            }
            info!("Truncated JSON detected on attempt {}. Requesting continuation.", attempt + 1);
        }

        let json_validator = JsonValidator::new(&self.llm_client);
        let json_value = json_validator.validate_and_repair(&accumulated_json).await?;
        
        serde_json::from_value(json_value)
            .map_err(|e| anyhow::anyhow!("Failed to parse final structured response from GPT-5: {}", e))
    }

    fn build_metadata_request(&self, content: &str, instructions: &str, continuation: Option<&str>) -> Value {
        let prompt = if let Some(existing_json) = continuation {
            format!("The previous response was truncated. Please continue generating the JSON from this point:\n\n{existing_json}")
        } else {
            format!("Analyze this AI response and provide structured metadata:\n\n{content}\n\n{instructions}")
        };
        json!({
            "model": CONFIG.gpt5_model,
            "input": [{"role": "user", "content": [{"type": "input_text", "text": prompt}]}],
            "text": { "format": "json_object" },
            // API UPDATE Sept 2025: reasoning_effort → reasoning.effort
            "verbosity": CONFIG.get_verbosity_for("metadata"),
            "reasoning": {
                "effort": CONFIG.get_reasoning_effort_for("metadata")
            },
            "max_output_tokens": CONFIG.get_json_max_tokens()
        })
    }

    fn build_metadata_instructions(&self, context: &crate::memory::recall::RecallContext) -> String {
        let mut instructions = String::from( "Return a JSON object with the structure: {\"output\": \"...\", \"metadata\": {\"mood\": \"...\", \"salience\": 0.0-1.0, \"summary\": \"...\", \"memory_type\": \"...\", \"tags\": [], \"intent\": \"...\"}, \"reasoning\": {\"monologue\": \"...\", \"summary\": \"...\"}}" );
        if !context.recent.is_empty() {
            instructions.push_str("\n\nConsider recent conversation context for continuity.");
        }
        if !context.semantic.is_empty() {
            instructions.push_str("\n\nConsider semantic context for relevance scoring.");
        }
        instructions
    }
    
    pub async fn handle_summarization(&self, session_id: &str) -> Result<()> {
        // self.summarizer.summarize_if_needed(session_id).await
        Ok(())
    }
}
