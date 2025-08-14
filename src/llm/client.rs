// src/llm/client.rs
use std::pin::Pin;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use futures::{Stream, StreamExt};
use reqwest::{header, Client};
use serde::{Deserialize, Serialize};
use serde_json::{self, json, Value};
use tracing::{debug, info};

/// Stream of JSON payloads from the OpenAI Responses SSE.
pub type ResponseStream = Pin<Box<dyn Stream<Item = Result<Value>> + Send>>;

pub struct OpenAIClient {
    client: Client,
    api_key: String,
    base_url: String,
    model: String,
    verbosity: String,
    reasoning_effort: String,
    max_output_tokens: usize,
}

impl OpenAIClient {
    pub fn new() -> Result<Arc<Self>> {
        let api_key = std::env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY must be set");
        let model = std::env::var("MIRA_MODEL").unwrap_or_else(|_| "gpt-5".to_string());
        let verbosity = std::env::var("MIRA_VERBOSITY").unwrap_or_else(|_| "medium".to_string());
        let reasoning_effort =
            std::env::var("MIRA_REASONING_EFFORT").unwrap_or_else(|_| "medium".to_string());
        let max_output_tokens = std::env::var("MIRA_MAX_OUTPUT_TOKENS")
            .unwrap_or_else(|_| "128000".to_string())
            .parse()
            .unwrap_or(128000);

        info!(
            "🚀 Initializing GPT-5 client (model={}, verbosity={}, reasoning={}, max_tokens={})",
            model, verbosity, reasoning_effort, max_output_tokens
        );

        Ok(Arc::new(Self {
            client: Client::new(),
            api_key,
            base_url: std::env::var("OPENAI_BASE_URL")
                .unwrap_or_else(|_| "https://api.openai.com".to_string()),
            model,
            verbosity,
            reasoning_effort,
            max_output_tokens,
        }))
    }

    // Small getters used by the streaming module
    pub fn model(&self) -> &str { &self.model }
    pub fn verbosity(&self) -> &str { &self.verbosity }
    pub fn reasoning_effort(&self) -> &str { &self.reasoning_effort }
    pub fn max_output_tokens(&self) -> usize { self.max_output_tokens }

    /// Generate a response using the GPT-5 Responses API (non-streaming).
    pub async fn generate_response(
        &self,
        user_text: &str,
        system_prompt: Option<&str>,
        request_structured: bool,
    ) -> Result<ResponseOutput> {
        let mut input = vec![json!({
            "role": "user",
            "content": [{ "type": "input_text", "text": user_text }]
        })];

        if let Some(system) = system_prompt {
            input.insert(
                0,
                json!({
                    "role": "system",
                    "content": [{ "type": "input_text", "text": system }]
                }),
            );
        }

        let mut request = json!({
            "model": self.model,
            "input": input,
            "text": {
                "verbosity": sanitize_verbosity(&self.verbosity)
            },
            "reasoning": {
                "effort": sanitize_reasoning(&self.reasoning_effort)
            },
            "max_output_tokens": self.max_output_tokens
        });
        
        if request_structured {
            // For GPT-5 Responses API, structured output uses this format with schema
            request["text"]["format"] = json!({
                "type": "json_schema",
                "name": "mira_response",
                "schema": {
                    "type": "object",
                    "properties": {
                        "output": {
                            "type": "string",
                            "description": "The main response text"
                        },
                        "mood": {
                            "type": "string",
                            "description": "The emotional tone of the response"
                        },
                        "salience": {
                            "type": "integer",
                            "minimum": 1,
                            "maximum": 10,
                            "description": "Importance score from 1-10"
                        },
                        "summary": {
                            "type": "string",
                            "description": "Brief summary of the interaction"
                        },
                        "memory_type": {
                            "type": "string",
                            "enum": ["event", "fact", "emotion", "preference", "context"],
                            "description": "Category of memory"
                        },
                        "tags": {
                            "type": "array",
                            "items": {
                                "type": "string"
                            },
                            "description": "Relevant tags for this interaction"
                        },
                        "intent": {
                            "type": "string",
                            "description": "The user's apparent intent"
                        },
                        "monologue": {
                            "type": ["string", "null"],
                            "description": "Internal reasoning or thoughts"
                        },
                        "reasoning_summary": {
                            "type": ["string", "null"],
                            "description": "Summary of reasoning process"
                        }
                    },
                    "required": ["output", "mood", "salience", "summary", "memory_type", "tags", "intent", "monologue", "reasoning_summary"],
                    "additionalProperties": false
                },
                "strict": true
            });
        }

        debug!("📤 Sending request to GPT-5 Responses API");

        let response = self
            .client
            .post(format!("{}/v1/responses", self.base_url))
            .header(header::AUTHORIZATION, format!("Bearer {}", self.api_key))
            .header(header::CONTENT_TYPE, "application/json")
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_else(|_| "<no body>".into());
            return Err(anyhow!("OpenAI API error ({}): {}", status, error_text));
        }

        let api_response: Value = response.json().await?;

        // Extract the text from the response
        let output_text = extract_text_from_responses(&api_response)
            .unwrap_or_default();

        Ok(ResponseOutput {
            output: output_text,
            reasoning_summary: api_response.get("reasoning_summary")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
        })
    }

    /// Generate a response as an **SSE stream** from the GPT-5 Responses API.
    pub async fn generate_response_stream(
        &self,
        user_text: &str,
        system_prompt: Option<&str>,
        request_structured: bool,
    ) -> Result<ResponseStream> {
        let mut input = vec![json!({
            "role": "user",
            "content": [{ "type": "input_text", "text": user_text }]
        })];

        if let Some(system) = system_prompt {
            input.insert(
                0,
                json!({
                    "role": "system",
                    "content": [{ "type": "input_text", "text": system }]
                }),
            );
        }

        let mut request = json!({
            "model": self.model,
            "input": input,
            "text": {
                "verbosity": sanitize_verbosity(&self.verbosity)
            },
            "reasoning": {
                "effort": sanitize_reasoning(&self.reasoning_effort)
            },
            "max_output_tokens": self.max_output_tokens,
            "stream": true
        });
        
        if request_structured {
            // For GPT-5 Responses API, structured output uses this format with schema
            request["text"]["format"] = json!({
                "type": "json_schema",
                "name": "mira_response",
                "schema": {
                    "type": "object",
                    "properties": {
                        "output": {
                            "type": "string",
                            "description": "The main response text"
                        },
                        "mood": {
                            "type": "string",
                            "description": "The emotional tone of the response"
                        },
                        "salience": {
                            "type": "integer",
                            "minimum": 1,
                            "maximum": 10,
                            "description": "Importance score from 1-10"
                        },
                        "summary": {
                            "type": "string",
                            "description": "Brief summary of the interaction"
                        },
                        "memory_type": {
                            "type": "string",
                            "enum": ["event", "fact", "emotion", "preference", "context"],
                            "description": "Category of memory"
                        },
                        "tags": {
                            "type": "array",
                            "items": {
                                "type": "string"
                            },
                            "description": "Relevant tags for this interaction"
                        },
                        "intent": {
                            "type": "string",
                            "description": "The user's apparent intent"
                        },
                        "monologue": {
                            "type": ["string", "null"],
                            "description": "Internal reasoning or thoughts"
                        },
                        "reasoning_summary": {
                            "type": ["string", "null"],
                            "description": "Summary of reasoning process"
                        }
                    },
                    "required": ["output", "mood", "salience", "summary", "memory_type", "tags", "intent", "monologue", "reasoning_summary"],
                    "additionalProperties": false
                },
                "strict": true
            });
        }

        self.post_response_stream(request).await
    }

    /// Low-level helper to POST a Responses request and return an SSE JSON stream.
    pub async fn post_response_stream(&self, body: Value) -> Result<ResponseStream> {
        let req = self
            .client
            .post(format!("{}/v1/responses", self.base_url))
            .header(header::AUTHORIZATION, format!("Bearer {}", self.api_key))
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::ACCEPT, "text/event-stream")
            .json(&body);

        let resp = req.send().await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let error_text = resp.text().await.unwrap_or_else(|_| "<no body>".into());
            return Err(anyhow!("OpenAI API error ({}): {}", status, error_text));
        }

        let bytes_stream = resp.bytes_stream();
        let s = sse_json_stream(bytes_stream);
        Ok(Box::pin(s))
    }

    /// Helper method for making POST requests (used by other modules)
    pub async fn post_response(&self, body: serde_json::Value) -> Result<serde_json::Value> {
        let response = self
            .client
            .post(format!("{}/v1/responses", self.base_url))
            .header(header::AUTHORIZATION, format!("Bearer {}", self.api_key))
            .header(header::CONTENT_TYPE, "application/json")
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_else(|_| "<no body>".into());
            return Err(anyhow::anyhow!("OpenAI API error ({}): {}", status, error_text));
        }

        response.json().await.map_err(Into::into)
    }

    /// Helper method for making generic requests (for other modules) - NOT ASYNC
    pub fn request(&self, method: reqwest::Method, endpoint: &str) -> reqwest::RequestBuilder {
        self.client
            .request(method, format!("{}/v1/{}", self.base_url, endpoint))
            .header(header::AUTHORIZATION, format!("Bearer {}", self.api_key))
            .header(header::CONTENT_TYPE, "application/json")
    }

    /// Helper method for multipart requests (for file uploads) - NOT ASYNC
    pub fn request_multipart(&self, endpoint: &str) -> reqwest::RequestBuilder {
        self.client
            .post(format!("{}/v1/{}", self.base_url, endpoint))
            .header(header::AUTHORIZATION, format!("Bearer {}", self.api_key))
    }

    /// Get embedding for text
    pub async fn get_embedding(&self, text: &str) -> Result<Vec<f32>> {
        let body = json!({
            "model": "text-embedding-3-small",
            "input": text
        });

        let response = self
            .client
            .post(format!("{}/v1/embeddings", self.base_url))
            .header(header::AUTHORIZATION, format!("Bearer {}", self.api_key))
            .header(header::CONTENT_TYPE, "application/json")
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_else(|_| "<no body>".into());
            return Err(anyhow!("OpenAI embedding API error ({}): {}", status, error_text));
        }

        let result: serde_json::Value = response.json().await?;
        
        let embedding = result
            .get("data")
            .and_then(|d| d.get(0))
            .and_then(|e| e.get("embedding"))
            .and_then(|e| e.as_array())
            .ok_or_else(|| anyhow!("Invalid embedding response format"))?
            .iter()
            .filter_map(|v| v.as_f64().map(|f| f as f32))
            .collect();

        Ok(embedding)
    }
}

// Data structures for request/response
#[derive(Debug)]
pub struct ResponseOutput {
    pub output: String,
    pub reasoning_summary: Option<String>,
}

/// SSE stream parser
fn sse_json_stream<S>(mut raw: S) -> impl Stream<Item = Result<Value>> + Send
where
    S: Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Unpin + Send + 'static,
{
    use bytes::Buf;

    let mut buf = bytes::BytesMut::new();

    async_stream::stream! {
        while let Some(chunk_res) = raw.next().await {
            match chunk_res {
                Ok(chunk) => {
                    buf.extend_from_slice(&chunk);

                    loop {
                        if let Some(pos) = find_frame_boundary(&buf) {
                            let frame = buf.split_to(pos);
                            // Drop the "\n\n"
                            if buf.remaining() >= 2 { let _ = buf.split_to(2); }

                            let text = String::from_utf8(frame.to_vec())
                                .unwrap_or_else(|_| String::new());

                            // Collect data: lines
                            let mut data_lines = Vec::new();
                            for line in text.lines() {
                                let l = line.trim_start();
                                if l.starts_with("data:") {
                                    let v = l["data:".len()..].trim_start();
                                    data_lines.push(v);
                                }
                            }

                            if data_lines.is_empty() { continue; }

                            let data_joined = data_lines.join("\n");

                            if data_joined == "[DONE]" {
                                yield Ok(json!({ "done": true }));
                                continue;
                            }

                            match serde_json::from_str::<Value>(&data_joined) {
                                Ok(v) => yield Ok(v),
                                Err(e) => yield Err(anyhow!("SSE data parse error: {e}; raw={}", data_joined)),
                            }
                        } else { break; }
                    }
                }
                Err(e) => { yield Err(anyhow!("SSE transport error: {e}")); }
            }
        }
    }
}

/// Find "\n\n" (or "\r\n\r\n") boundary in buffer; return index of frame end.
fn find_frame_boundary(buf: &bytes::BytesMut) -> Option<usize> {
    if let Some(i) = twowin(buf, b'\n', b'\n') { return Some(i); }
    if let Some(i) = fourwin(buf, b'\r', b'\n', b'\r', b'\n') { return Some(i); }
    None
}

fn twowin(buf: &bytes::BytesMut, a: u8, b: u8) -> Option<usize> {
    let bytes = &buf[..];
    for i in 0..bytes.len().saturating_sub(1) {
        if bytes[i] == a && bytes[i + 1] == b { return Some(i); }
    }
    None
}

fn fourwin(buf: &bytes::BytesMut, a: u8, b: u8, c: u8, d: u8) -> Option<usize> {
    let bytes = &buf[..];
    for i in 0..bytes.len().saturating_sub(3) {
        if bytes[i] == a && bytes[i + 1] == b && bytes[i + 2] == c && bytes[i + 3] == d { return Some(i); }
    }
    None
}

/// Helper to extract text from GPT-5 Responses API output
pub fn extract_text_from_responses(resp_json: &serde_json::Value) -> Option<String> {
    // Try output array first (standard Responses API format)
    if let Some(output) = resp_json.get("output").and_then(|o| o.as_array()) {
        let mut text_parts = vec![];
        for item in output {
            if item.get("type").and_then(|t| t.as_str()) == Some("message") {
                if let Some(content) = item.get("content").and_then(|c| c.as_array()) {
                    for content_item in content {
                        if content_item.get("type").and_then(|t| t.as_str()) == Some("output_text") {
                            if let Some(text) = content_item.get("text").and_then(|t| t.as_str()) {
                                text_parts.push(text.to_string());
                            }
                        }
                    }
                }
            } else if item.get("type").and_then(|t| t.as_str()) == Some("output_text") {
                if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                    text_parts.push(text.to_string());
                }
            }
        }
        if !text_parts.is_empty() { 
            return Some(text_parts.join("\n")); 
        }
    }

    // Fallback to choices format (Chat Completions compatibility)
    resp_json
        .pointer("/choices/0/message/content")
        .and_then(|c| c.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|part| {
                    if part.get("type").and_then(|t| t.as_str()) == Some("output_text") {
                        part.get("text").and_then(|t| t.as_str()).map(|s| s.to_string())
                    } else { None }
                })
                .collect::<Vec<_>>()
                .join("")
        })
        .or_else(|| {
            resp_json
                .pointer("/choices/0/message/content")
                .and_then(|c| c.as_str())
                .map(|s| s.to_string())
        })
}

fn sanitize_verbosity(v: &str) -> &'static str {
    match v.trim().to_ascii_lowercase().as_str() {
        "low" => "low",
        "high" => "high",
        _ => "medium",
    }
}

fn sanitize_reasoning(v: &str) -> &'static str {
    match v.trim().to_ascii_lowercase().as_str() {
        "low" | "minimal" => "minimal",
        "high" => "high",
        _ => "medium",
    }
}
