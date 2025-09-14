// src/llm/chat_service/response.rs

use std::sync::Arc;
use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::{debug, info, warn};

use crate::config::CONFIG;
use crate::llm::client::OpenAIClient;
use crate::memory::recall::RecallContext;
use crate::memory::MemoryService;
use crate::persona::PersonaOverlay;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatResponse {
    pub output: String,
    pub persona: String,
    pub mood: String,
    pub salience: u8,
    pub summary: String,
    pub memory_type: String,
    pub tags: Vec<String>,
    pub intent: Option<String>,
    pub monologue: Option<String>,
    pub reasoning_summary: Option<String>,
}

impl Default for ChatResponse {
    fn default() -> Self {
        Self {
            output: String::new(),
            persona: "mira".to_string(),
            mood: "neutral".to_string(),
            salience: 5,
            summary: String::new(),
            memory_type: "Response".to_string(),
            tags: vec![],
            intent: None,
            monologue: None,
            reasoning_summary: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetadataRequest {
    pub content: String,
    pub context: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetadataResponse {
    pub mood: String,
    pub salience: u8,
    pub summary: String,
    pub memory_type: String,
    pub tags: Vec<String>,
    pub intent: String,
}

pub fn repair_json_with_gpt5(json_str: &str, llm_client: &OpenAIClient) -> impl std::future::Future<Output = Result<String>> {
    let json_str = json_str.to_string();
    let llm_client = llm_client.clone();
    
    async move {
        let prompt = format!(
            "Fix this malformed JSON and return ONLY the corrected JSON with no other text:\n\n{}",
            json_str
        );
        
        let body = json!({
            "model": CONFIG.gpt5_model,
            "input": [{
                "role": "user",
                "content": [{
                    "type": "input_text",
                    "text": prompt
                }]
            }],
            "max_output_tokens": 2000,
            "text": {
                "verbosity": "low",
                "format": {
                    "type": "json_object"
                }
            },
            "reasoning": {
                "effort": "minimal"
            }
        });
        
        let response = llm_client.post_response(body).await?;
        
        if let Some(text) = response.pointer("/output/1/content/0/text").and_then(|t| t.as_str()) {
            Ok(text.to_string())
        } else if let Some(text) = response.get("output").and_then(|o| o.as_str()) {
            Ok(text.to_string())
        } else {
            Err(anyhow::anyhow!("Failed to extract repaired JSON from response"))
        }
    }
}

async fn build_metadata_request(
    llm_client: &OpenAIClient,
    response_content: &str,
    context: &RecallContext,
) -> Result<MetadataResponse> {
    let context_str = if !context.recent.is_empty() {
        context.recent.iter()
            .take(5)
            .map(|m| format!("[{}]: {}", m.role, m.content))
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        "No recent context".to_string()
    };
    
    let prompt = format!(
        r#"Analyze this AI assistant response and provide metadata.

Response: "{}"
Context: {}

Provide JSON with:
- mood: emotional tone (happy/sad/neutral/excited/frustrated/playful/serious)
- salience: importance 0-10
- summary: one sentence summary
- memory_type: feeling/fact/joke/promise/event/other
- tags: 2-5 relevant keywords
- intent: primary purpose of response"#,
        response_content,
        context_str
    );
    
    let body = json!({
        "model": CONFIG.gpt5_model,
        "input": [{
            "role": "user",
            "content": [{
                "type": "input_text",
                "text": prompt
            }]
        }],
        "max_output_tokens": 256,
        "temperature": 0.3,
        "text": {
            "verbosity": "low",
            "format": {
                "type": "json_schema",
                "json_schema": {
                    "name": "metadata",
                    "strict": true,
                    "schema": {
                        "type": "object",
                        "properties": {
                            "mood": {"type": "string"},
                            "salience": {"type": "integer", "minimum": 0, "maximum": 10},
                            "summary": {"type": "string"},
                            "memory_type": {"type": "string", "enum": ["feeling", "fact", "joke", "promise", "event", "other"]},
                            "tags": {"type": "array", "items": {"type": "string"}},
                            "intent": {"type": "string"}
                        },
                        "required": ["mood", "salience", "summary", "memory_type", "tags", "intent"]
                    }
                }
            }
        },
        "reasoning": {
            "effort": "minimal"
        }
    });
    
    let response = llm_client.post_response(body).await?;
    
    let metadata_json = if let Some(text) = response.pointer("/output/1/content/0/text").and_then(|t| t.as_str()) {
        text
    } else if let Some(text) = response.get("output").and_then(|o| o.as_str()) {
        text
    } else {
        return Err(anyhow::anyhow!("Failed to extract metadata from response"));
    };
    
    serde_json::from_str(metadata_json)
        .map_err(|e| anyhow::anyhow!("Failed to parse metadata JSON: {}", e))
}

pub struct ResponseProcessor {
    memory_service: Arc<MemoryService>,
    persona: PersonaOverlay,
    llm_client: Arc<OpenAIClient>,
}

impl ResponseProcessor {
    pub fn new(
        memory_service: Arc<MemoryService>,
        persona: PersonaOverlay,
        llm_client: Arc<OpenAIClient>,
    ) -> Self {
        Self { memory_service, persona, llm_client }
    }
    
    pub async fn persist_user_message(
        &self,
        session_id: &str,
        content: &str,
        project_id: Option<&str>,
    ) -> Result<()> {
        self.memory_service.save_user_message(session_id, content, project_id).await.map(|_| ())
    }
    
    pub async fn process_response(
        &self,
        session_id: &str,
        response_content: String,
        context: &RecallContext,
        project_id: Option<&str>,
    ) -> Result<ChatResponse> {
        info!("Processing response for session: {}", session_id);
        
        let metadata = match build_metadata_request(&self.llm_client, &response_content, context).await {
            Ok(meta) => meta,
            Err(e) => {
                warn!("Failed to get metadata, using defaults: {}", e);
                MetadataResponse {
                    mood: "neutral".to_string(),
                    salience: 5,
                    summary: response_content.chars().take(100).collect::<String>(),
                    memory_type: "other".to_string(),
                    tags: vec!["chat".to_string()],
                    intent: "response".to_string(),
                }
            }
        };
        
        let response = ChatResponse {
            output: response_content.clone(),
            persona: "mira".to_string(),  // Always use "mira" since that's the actual persona
            mood: metadata.mood,
            salience: metadata.salience,
            summary: metadata.summary,
            memory_type: metadata.memory_type.clone(),
            tags: metadata.tags,
            intent: Some(metadata.intent),
            monologue: None,
            reasoning_summary: None,
        };
        
        self.save_to_memory(session_id, &response, project_id).await?;
        
        Ok(response)
    }
    
    async fn save_to_memory(
        &self,
        session_id: &str,
        response: &ChatResponse,
        _project_id: Option<&str>,
    ) -> Result<()> {
        self.memory_service.save_assistant_response(session_id, response).await?;
        
        if response.salience >= 7 {
            debug!("High salience response ({}), will be indexed", response.salience);
        }
        
        Ok(())
    }
    
    pub async fn handle_summarization(&self, _session_id: &str) -> Result<()> {
        Ok(())
    }
}
