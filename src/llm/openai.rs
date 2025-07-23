// src/llm/openai.rs

//! Low-level OpenAI API client for embeddings, moderation, function-calling, and streaming chat.
//! No wrappers; just reqwest and Rust, as the universe intended.

use reqwest::Client;
use anyhow::{Result, anyhow};
use serde_json::{json, Value};
use std::env;

use crate::llm::schema::{EvaluateMemoryRequest, EvaluateMemoryResponse};
use crate::api::ws::message::WsServerMessage;
use futures::stream::Stream;
use futures::StreamExt;
use std::pin::Pin;

#[derive(Clone)]
pub struct OpenAIClient {
    pub client: Client,
    pub api_key: String,
    pub api_base: String, // Default "https://api.openai.com/v1", but can be overridden
}

impl OpenAIClient {
    pub fn new() -> Self {
        let api_key = env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY not set");
        let api_base = env::var("OPENAI_BASE_URL").unwrap_or_else(|_| "https://api.openai.com/v1".to_string());
        Self {
            client: Client::new(),
            api_key,
            api_base,
        }
    }

    fn auth_header(&self) -> (&'static str, String) {
        ("Authorization", format!("Bearer {}", self.api_key))
    }

    /// Gets OpenAI text embedding (1536d) for a string.
    pub async fn get_embedding(&self, text: &str) -> Result<Vec<f32>> {
        let url = format!("{}/embeddings", self.api_base);
        let req_body = json!({
            "input": text,
            "model": "text-embedding-3-large"
        });
        let resp = self
            .client
            .post(&url)
            .header(self.auth_header().0, self.auth_header().1.clone())
            .json(&req_body)
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(anyhow!("OpenAI embedding failed: {}", resp.text().await.unwrap_or_default()));
        }
        let resp_json: serde_json::Value = resp.json().await?;
        // Extract embedding (1536 floats)
        let embedding = resp_json["data"][0]["embedding"]
            .as_array()
            .ok_or_else(|| anyhow!("No embedding in OpenAI response"))?
            .iter()
            .map(|v| v.as_f64().unwrap_or(0.0) as f32)
            .collect();

        Ok(embedding)
    }

    /// Runs moderation API on user/Mira message, returns flagged true/false.
    pub async fn moderate_message(&self, text: &str) -> Result<bool> {
        let url = format!("{}/moderations", self.api_base);
        let req_body = json!({ "input": text });
        let resp = self
            .client
            .post(&url)
            .header(self.auth_header().0, self.auth_header().1.clone())
            .json(&req_body)
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(anyhow!("OpenAI moderation failed: {}", resp.text().await.unwrap_or_default()));
        }
        let resp_json: serde_json::Value = resp.json().await?;
        let flagged = resp_json["results"][0]["flagged"].as_bool().unwrap_or(false);
        Ok(flagged)
    }

    /// Runs GPT-4.1 function-calling for memory evaluation.
    /// Calls LLM with system prompt, user message, and function schema, gets back structured metadata.
    pub async fn evaluate_memory(&self, req: &EvaluateMemoryRequest) -> Result<EvaluateMemoryResponse> {
        let url = format!("{}/chat/completions", self.api_base);

        // Compose the system prompt (feel free to refine this for your project's voice)
        let system_prompt = r#"You are an emotionally intelligent AI. For every message you receive, extract the following:
- Salience (how important is this to the user's emotional world, 1-10)
- Tags (context, relationships, mood)
- A one-sentence summary (optional)
- Memory type (choose one: feeling, fact, joke, promise, event, or other)
Use only the message, its context, and your intuition—do not rely on keywords. Return your answer as a valid JSON object conforming to the schema."#;

        let messages = vec![
            json!({"role": "system", "content": system_prompt}),
            json!({"role": "user", "content": req.content}),
        ];

        let function_schema = req.function_schema.clone();

        let body = json!({
            "model": "gpt-4.1",
            "messages": messages,
            "functions": [function_schema],
            "function_call": { "name": "evaluate_memory" },
            "response_format": { "type": "json_object" },
            "temperature": 0.2
        });

        let resp = self
            .client
            .post(&url)
            .header(self.auth_header().0, self.auth_header().1.clone())
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(anyhow!(
                "OpenAI LLM call failed: {}",
                resp.text().await.unwrap_or_default()
            ));
        }
        let resp_json: serde_json::Value = resp.json().await?;

        // Extract function_call.arguments as JSON and parse into EvaluateMemoryResponse
        let args_json = resp_json["choices"][0]["message"]["function_call"]["arguments"]
            .as_str()
            .ok_or_else(|| anyhow!("No function_call arguments found in LLM response"))?;

        let result: EvaluateMemoryResponse = serde_json::from_str(args_json)?;

        Ok(result)
    }

    /// Streams GPT-4.1 response for chat (WebSocket streaming).
    /// Uses a robust marker-based approach for extracting mood and emotional cues.
    pub async fn stream_gpt4_ws_messages(
        &self,
        prompt: String,
        persona: Option<String>,
        system_prompt: String,
        model: Option<&str>,
    ) -> Pin<Box<dyn Stream<Item = WsServerMessage> + Send>> {
        let url = format!("{}/chat/completions", self.api_base);

        let model = model.unwrap_or("gpt-4.1");
        let persona_string = persona.clone().unwrap_or_else(|| "Default".to_string());

        let messages = vec![
            json!({"role": "system", "content": system_prompt}),
            json!({"role": "user", "content": prompt}),
        ];

        let req_body = json!({
            "model": model,
            "messages": messages,
            "stream": true,
            "max_tokens": 32768,
            "temperature": 0.9
        });

        let api_key = self.api_key.clone();
        let client = self.client.clone();

        let stream = async_stream::stream! {
            let resp = match client
                .post(&url)
                .header("Authorization", format!("Bearer {}", api_key))
                .header("Content-Type", "application/json")
                .json(&req_body)
                .send()
                .await 
            {
                Ok(res) => res,
                Err(err) => {
                    yield WsServerMessage::Chunk {
                        content: format!("Error: failed to reach LLM API: {}", err),
                        persona: persona_string,
                        mood: Some("error".to_string()),
                    };
                    return;
                }
            };

            if !resp.status().is_success() {
                yield WsServerMessage::Chunk {
                    content: format!("Error: API returned status {}", resp.status()),
                    persona: persona_string.clone(),
                    mood: Some("error".to_string()),
                };
                return;
            }

            let mut text_buffer = String::new();
            let mut current_mood: Option<String> = None;
            let mut in_mood_marker = false;
            let mut in_aside_marker = false;
            let mut mood_buffer = String::new();
            let mut aside_buffer = String::new();
            let mut partial_line = String::new();

            // Convert response to text stream
            let mut byte_stream = resp.bytes_stream();
            
            while let Some(chunk_result) = byte_stream.next().await {
                let bytes = match chunk_result {
                    Ok(b) => b,
                    Err(e) => {
                        eprintln!("Error reading stream: {}", e);
                        break;
                    }
                };

                let chunk_str = match std::str::from_utf8(&bytes) {
                    Ok(s) => s,
                    Err(_) => continue,
                };

                // Handle partial lines from streaming
                partial_line.push_str(chunk_str);
                
                while let Some(newline_pos) = partial_line.find('\n') {
                    let line = partial_line[..newline_pos].to_string();
                    partial_line = partial_line[newline_pos + 1..].to_string();

                    if !line.starts_with("data: ") {
                        continue;
                    }

                    let data = &line[6..];
                    if data == "[DONE]" {
                        // Flush any remaining content
                        if !text_buffer.is_empty() {
                            yield WsServerMessage::Chunk {
                                content: text_buffer.clone(),
                                persona: persona_string.clone(),
                                mood: current_mood.clone(),
                            };
                        }
                        yield WsServerMessage::Done;
                        return;
                    }

                    let json_val: Value = match serde_json::from_str(data) {
                        Ok(v) => v,
                        Err(_) => continue,
                    };

                    if let Some(content) = json_val["choices"][0]["delta"]["content"].as_str() {
                        // Process content character by character to detect markers
                        for ch in content.chars() {
                            if ch == '⟨' && !in_mood_marker && !in_aside_marker {
                                // Start mood marker
                                if !text_buffer.is_empty() {
                                    yield WsServerMessage::Chunk {
                                        content: text_buffer.clone(),
                                        persona: persona_string.clone(),
                                        mood: current_mood.clone(),
                                    };
                                    text_buffer.clear();
                                }
                                in_mood_marker = true;
                                mood_buffer.clear();
                            } else if ch == '⟩' && in_mood_marker {
                                // End of mood marker - extract mood but don't include in output
                                in_mood_marker = false;
                                if !mood_buffer.is_empty() {
                                    current_mood = Some(mood_buffer.clone());
                                    // Don't add the mood text to the buffer
                                }
                            } else if ch == '⟦' && !in_aside_marker && !in_mood_marker {
                                // Start aside marker
                                if !text_buffer.is_empty() {
                                    yield WsServerMessage::Chunk {
                                        content: text_buffer.clone(),
                                        persona: persona_string.clone(),
                                        mood: current_mood.clone(),
                                    };
                                    text_buffer.clear();
                                }
                                in_aside_marker = true;
                                aside_buffer.clear();
                            } else if ch == '⟧' && in_aside_marker {
                                // End of aside marker
                                in_aside_marker = false;
                                if !aside_buffer.is_empty() {
                                    let intensity = calculate_emotional_intensity(&aside_buffer);
                                    yield WsServerMessage::Aside {
                                        emotional_cue: aside_buffer.clone(),
                                        intensity: Some(intensity),
                                    };
                                }
                            } else if in_mood_marker {
                                mood_buffer.push(ch);
                            } else if in_aside_marker {
                                aside_buffer.push(ch);
                            } else {
                                text_buffer.push(ch);
                            }
                        }

                        // Yield accumulated content periodically
                        if text_buffer.len() > 50 && !in_mood_marker && !in_aside_marker {
                            yield WsServerMessage::Chunk {
                                content: text_buffer.clone(),
                                persona: persona_string.clone(),
                                mood: current_mood.clone(),
                            };
                            text_buffer.clear();
                        }
                    }
                }
            }

            // Final cleanup
            if !text_buffer.is_empty() {
                yield WsServerMessage::Chunk {
                    content: text_buffer,
                    persona: persona_string.clone(),
                    mood: current_mood,
                };
            }
        };

        Box::pin(stream)
    }
}

/// Calculate emotional intensity based on cue content
fn calculate_emotional_intensity(cue: &str) -> f32 {
    let cue_lower = cue.to_lowercase();
    
    // High intensity indicators
    if cue_lower.contains("fuck") || cue_lower.contains("damn") || 
       cue_lower.contains("shit") || cue_lower.contains("!") ||
       cue_lower.contains("...") {
        return 0.8;
    }
    
    // Medium intensity
    if cue_lower.contains("really") || cue_lower.contains("very") ||
       cue_lower.contains("so ") || cue_lower.contains("?") {
        return 0.5;
    }
    
    // Default low intensity
    0.3
}
