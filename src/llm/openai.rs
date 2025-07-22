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
    /// Each chunk is sent as a WsServerMessage, with asides for emotional_cue.
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
            "max_tokens": 32768 // FULL SEND for GPT-4.1
        });

        let api_key = self.api_key.clone();
        let client = self.client.clone();

        let request = client
            .post(&url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&req_body);

        let resp = match request.send().await {
            Ok(res) => res,
            Err(err) => {
                let err_stream = futures::stream::once(async move {
                    WsServerMessage::Chunk {
                        content: format!("Error: failed to reach LLM API: {err}"),
                        persona: persona_string,
                        mood: Some("error".to_string()),
                    }
                });
                return Box::pin(err_stream);
            }
        };

        let stream = async_stream::stream! {
            let mut buffer = String::new();
            let mut seen_emotional_cue = false;
            let mut response = resp;
            while let Ok(Some(chunk)) = response.chunk().await {
                for line in chunk.split(|b| *b == b'\n') {
                    if line.starts_with(b"data: ") {
                        let json_str = &line[6..];
                        if json_str == b"[DONE]" {
                            yield WsServerMessage::Done;
                            return;
                        }
                        let json_val: Value = match serde_json::from_slice(json_str) {
                            Ok(v) => v,
                            Err(_) => continue,
                        };
                        let choices = json_val["choices"].as_array();
                        if let Some(choices) = choices {
                            for choice in choices {
                                if let Some(content) = choice["delta"]["content"].as_str() {
                                    buffer.push_str(content);
                                    yield WsServerMessage::Chunk {
                                        content: content.to_string(),
                                        persona: persona_string.clone(),
                                        mood: None,
                                    };
                                }
                                if let Some(fn_args) = choice["delta"]["function_call"]["arguments"].as_str() {
                                    if let Ok(args_json) = serde_json::from_str::<Value>(fn_args) {
                                        let output = args_json["output"].as_str().unwrap_or("").to_string();
                                        let persona_field = args_json["persona"].as_str().unwrap_or("Default").to_string();
                                        let mood = args_json["mood"].as_str().map(|s| s.to_string());
                                        if !seen_emotional_cue {
                                            if let Some(as_str) = args_json.get("emotional_cue").and_then(|v| v.as_str()) {
                                                if !as_str.is_empty() {
                                                    yield WsServerMessage::Aside {
                                                        emotional_cue: as_str.to_string(),
                                                        intensity: None,
                                                    };
                                                    seen_emotional_cue = true;
                                                }
                                            }
                                        }
                                        if !output.is_empty() {
                                            yield WsServerMessage::Chunk {
                                                content: output,
                                                persona: persona_field,
                                                mood,
                                            };
                                        }
                                        yield WsServerMessage::Done;
                                        return;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        };
        Box::pin(stream)
    }
}
