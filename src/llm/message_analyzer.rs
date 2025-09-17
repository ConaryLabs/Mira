// src/llm/message_analyzer.rs

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{info, warn};

use crate::llm::client::OpenAIClient;
use crate::state::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageAnalysis {
    pub mood: String,
    pub salience: f32,
    pub tags: Vec<String>,
    pub summary: Option<String>,
    pub memory_type: String,
    pub intent: Option<String>,
}

pub struct MessageAnalyzer {
    client: Arc<OpenAIClient>,
}

impl MessageAnalyzer {
    pub fn new(client: Arc<OpenAIClient>) -> Self {
        Self { client }
    }
    
    /// Analyze a message and extract metadata
    pub async fn analyze(&self, content: &str) -> Result<MessageAnalysis> {
        let max_retries = 3;
        let mut attempt = 0;
        
        loop {
            attempt += 1;
            match self.analyze_attempt(content).await {
                Ok(response) => return Ok(response),
                Err(e) if attempt < max_retries => {
                    let error_str = e.to_string();
                    if error_str.contains("429") || error_str.contains("5") {
                        let jitter = Duration::from_millis(100 * attempt as u64 + rand::random::<u64>() % 100);
                        warn!("Message analysis attempt {} failed ({}), retrying after {:?}...", 
                              attempt, error_str, jitter);
                        sleep(jitter).await;
                        continue;
                    }
                    return Err(e);
                }
                Err(e) => return Err(e),
            }
        }
    }

    async fn analyze_attempt(&self, content: &str) -> Result<MessageAnalysis> {
        let json_schema = json!({
            "type": "json_schema",
            "json_schema": {
                "name": "message_analysis",
                "strict": true,
                "schema": {
                    "type": "object",
                    "properties": {
                        "mood": {
                            "type": "string",
                            "description": "Emotional tone (e.g., flirty, sarcastic, angry, helpful, excited)"
                        },
                        "salience": {
                            "type": "number",
                            "minimum": 0,
                            "maximum": 10,
                            "description": "Emotional importance (0=trivial, 10=life-changing)"
                        },
                        "tags": {
                            "type": "array",
                            "items": {"type": "string"},
                            "description": "Relevant keywords and themes"
                        },
                        "memory_type": {
                            "type": "string",
                            "enum": ["feeling", "fact", "joke", "promise", "event", "other"]
                        },
                        "summary": {
                            "type": "string",
                            "description": "One-sentence summary if message is long"
                        },
                        "intent": {
                            "type": "string",
                            "description": "What the speaker is trying to achieve"
                        }
                    },
                    "required": ["mood", "salience", "tags", "memory_type"]
                }
            }
        });

        let body = json!({
            "model": "gpt-5",
            "input": [{
                "role": "user",
                "content": [{ 
                    "type": "input_text", 
                    "text": format!(
                        "Analyze this message comprehensively:\n\n\"{}\"\n\n\
                        Extract mood, emotional salience (0-10), relevant tags, \
                        memory type, intent, and a summary if needed.",
                        content
                    )
                }]
            }],
            "instructions": "Analyze the message and return a comprehensive JSON object with all metadata.",
            "max_output_tokens": 256,
            "temperature": 0.3,
            "text": {
                "verbosity": "low",
                "format": json_schema
            },
            "reasoning": {
                "effort": "minimal"
            }
        });

        let v = self.client.post_response(body).await
            .context("Failed to call GPT-5 for message analysis")?;

        // Extract text content from response
        let response_text = if let Some(text) = v.pointer("/output/1/content/0/text").and_then(|t| t.as_str()) {
            text
        } else if let Some(text) = v.pointer("/output/message/content/0/text/value").and_then(|t| t.as_str()) {
            text
        } else if let Some(text) = v.pointer("/output/message/content/0/text").and_then(|t| t.as_str()) {
            text
        } else if let Some(text) = v.get("output").and_then(|o| o.as_str()) {
            text
        } else {
            return Err(anyhow!(
                "Could not extract text from response. Raw response: {:?}",
                v
            ));
        };

        // Parse the JSON response
        serde_json::from_str::<MessageAnalysis>(response_text)
            .context("Failed to parse message analysis JSON response")
    }
    
    /// Fire-and-forget async message enrichment
    pub fn analyze_async(
        app_state: Arc<AppState>,
        _session_id: String,
        message_id: i64,
        content: String,
    ) {
        tokio::spawn(async move {
            // Create analyzer with the LLM client from app_state
            let analyzer = MessageAnalyzer::new(app_state.llm_client.clone());
            
            // Add slight delay to avoid hammering API
            tokio::time::sleep(Duration::from_millis(100)).await;
            
            match analyzer.analyze(&content).await {
                Ok(analysis) => {
                    info!("Message {} analyzed: mood={}, salience={}", 
                          message_id, analysis.mood, analysis.salience);
                    
                    // TODO: Update message in database with analysis
                    // This will need a new method in memory_service
                    // app_state.memory_service.update_message_analysis(
                    //     &session_id,
                    //     message_id,
                    //     analysis
                    // ).await;
                }
                Err(e) => {
                    warn!("Failed to analyze message {}: {}", message_id, e);
                }
            }
        });
    }
}
