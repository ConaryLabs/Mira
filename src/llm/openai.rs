//! Low-level OpenAI API client for embeddings, moderation, function-calling, and streaming chat.
//! No wrappers; just reqwest and Rust, as the universe intended.

use reqwest::Client;
use anyhow::{Result, anyhow};
use serde_json::{json, Value};
use std::env;

use crate::llm::schema::{EvaluateMemoryRequest, EvaluateMemoryResponse, MiraStructuredReply};
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

    /// Gets OpenAI text embedding (3072d) for a string.
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
        // Extract embedding (3072 floats)
        let embedding = resp_json["data"][0]["embedding"]
            .as_array()
            .ok_or_else(|| anyhow!("No embedding in OpenAI response"))?
            .iter()
            .map(|v| v.as_f64().unwrap_or(0.0) as f32)
            .collect();

        Ok(embedding)
    }

    /// Runs moderation API on user/Mira message, returns flagged true/false.
    /// *Log-only version for private use—never blocks!*
    pub async fn moderate(&self, text: &str) -> Result<Option<ModerationResult>> {
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
            tracing::warn!("OpenAI moderation call failed: {}", resp.text().await.unwrap_or_default());
            return Ok(None);
        }
        let resp_json: serde_json::Value = resp.json().await?;
        let flagged = resp_json["results"][0]["flagged"].as_bool().unwrap_or(false);
        let categories = resp_json["results"][0]["categories"]
            .as_object()
            .map(|map| map.keys().cloned().collect())
            .unwrap_or_else(Vec::new);

        if flagged {
            tracing::warn!("MODERATION (WARN ONLY): flagged categories: {:?}", categories);
        }
        Ok(Some(ModerationResult { flagged, categories }))
    }

    /// Runs GPT-4.1 function-calling for memory evaluation.
    pub async fn evaluate_memory(&self, req: &EvaluateMemoryRequest) -> Result<EvaluateMemoryResponse> {
        let url = format!("{}/chat/completions", self.api_base);

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

        let args_json = resp_json["choices"][0]["message"]["function_call"]["arguments"]
            .as_str()
            .ok_or_else(|| anyhow!("No function_call arguments found in LLM response"))?;

        let result: EvaluateMemoryResponse = serde_json::from_str(args_json)?;

        Ok(result)
    }

    /// Generic chat call to any OpenAI model, returns strict-structured output.
    pub async fn chat_with_model(&self, message: &str, model: &str) -> Result<MiraStructuredReply, anyhow::Error> {
        let url = format!("{}/chat/completions", self.api_base);

        // Build the system prompt (could be improved to be persona/context aware)
        let system_prompt = r#"
You are Mira, an emotionally present AI companion. For every reply, output a single valid JSON object with the following fields:
- output: Your full reply to the user.
- persona: Your current persona name.
- mood: Your current mood or emotional tone.
- salience: How emotionally important is this reply? (0-10)
- summary: A short summary of your reply/context.
- memory_type: "feeling", "fact", "joke", "promise", "event", or "other".
- tags: An array of context/mood tags.
- intent: Your intent in this reply.
- monologue: Your private, inner thoughts (optional).
- reasoning_summary: Your reasoning or chain-of-thought for this reply (optional).

ALWAYS output only a valid JSON object that matches this schema and nothing else.
"#;

        let messages = vec![
            json!({"role": "system", "content": system_prompt}),
            json!({"role": "user", "content": message}),
        ];

        let body = json!({
            "model": model,
            "messages": messages,
            "temperature": 0.8,
            "response_format": { "type": "json_object" }
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
                "OpenAI chat_with_model failed: {}",
                resp.text().await.unwrap_or_default()
            ));
        }
        let resp_json: serde_json::Value = resp.json().await?;

        let content = resp_json["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| anyhow!("No content in OpenAI chat response"))?;

        let reply: MiraStructuredReply = serde_json::from_str(content)
            .map_err(|e| anyhow!("Failed to parse MiraStructuredReply: {}\nRaw content:\n{}", e, content))?;

        Ok(reply)
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
                let error_text = resp.text().await.unwrap_or_else(|_| "Unknown error".to_string());
                eprintln!("OpenAI API error: {}", error_text);
                yield WsServerMessage::Chunk {
                    content: format!("Error: API returned error: {}", error_text),
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
            let mut _last_was_space = false;  // Fixed: Added underscore prefix

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
                        // Flush any remaining content, but clean up trailing punctuation
                        if !text_buffer.is_empty() {
                            // Trim trailing whitespace and single punctuation
                            let trimmed = text_buffer.trim_end();
                            if !trimmed.is_empty() && trimmed.len() > 1 {
                                yield WsServerMessage::Chunk {
                                    content: trimmed.to_string(),
                                    persona: persona_string.clone(),
                                    mood: current_mood.clone(),
                                };
                            }
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
                                
                                // Track if we just added a space
                                _last_was_space = ch.is_whitespace();  // Fixed: Added underscore prefix
                            }
                        }

                        // Just accumulate - no chunking needed!
                    }
                }
            }

            // Final cleanup - handle any partial SSE line
            if !partial_line.is_empty() && partial_line.starts_with("data: ") {
                let data = &partial_line[6..];
                if data != "[DONE]" {
                    if let Ok(json_val) = serde_json::from_str::<Value>(data) {
                        if let Some(content) = json_val["choices"][0]["delta"]["content"].as_str() {
                            text_buffer.push_str(content);
                        }
                    }
                }
            }

            // Final cleanup of remaining buffer
            if !text_buffer.is_empty() {
                let trimmed = text_buffer.trim();
                // Don't send single punctuation marks as their own message
                if !trimmed.is_empty() && 
                   !(trimmed.len() == 1 && trimmed.chars().next().unwrap().is_ascii_punctuation()) {
                    yield WsServerMessage::Chunk {
                        content: trimmed.to_string(),
                        persona: persona_string.clone(),
                        mood: current_mood,
                    };
                }
            }
        };

        Box::pin(stream)
    }
}

#[derive(Debug)]
pub struct ModerationResult {
    pub flagged: bool,
    pub categories: Vec<String>,
}

#[derive(Debug)]
pub struct ChatResponse {
    pub content: String,
    // Add more fields as needed
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
