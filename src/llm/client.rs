// src/llm/client.rs
use std::pin::Pin;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use futures::{Stream, StreamExt, stream};
use reqwest::{header, Client};
use serde_json::{self, json, Value};
use tracing::{debug, info, error};

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

    // Small getters used by other modules
    pub fn model(&self) -> &str { &self.model }
    pub fn verbosity(&self) -> &str { &self.verbosity }
    pub fn reasoning_effort(&self) -> &str { &self.reasoning_effort }
    pub fn max_output_tokens(&self) -> usize { self.max_output_tokens }

    /// Generate a response using the GPT-5 Responses API (non-streaming).
    /// If `request_structured` is true, we attach a proper `json_schema` TEXT FORMAT OBJECT.
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
            "model": &self.model,
            "input": input,
            "text": {
                "verbosity": norm_verbosity(&self.verbosity)
            },
            "reasoning": {
                "effort": norm_effort(&self.reasoning_effort)
            },
            "max_output_tokens": self.max_output_tokens
        });

        if request_structured {
            // IMPORTANT: format must be an OBJECT, not a string
            request["text"]["format"] = json!({
                "type": "json_schema",
                "name": "mira_response",
                "schema": {
                    "type": "object",
                    "properties": {
                        "output": { "type":"string" },
                        "mood": { "type":"string" },
                        "salience": { "type":"integer", "minimum": 0, "maximum": 10 },
                        "summary": { "type":"string" },
                        "memory_type": { "type":"string" },
                        "tags": { "type":"array", "items": { "type":"string" } },
                        "intent": { "type":"string" },
                        "monologue": { "type":["string","null"] },
                        "reasoning_summary": { "type":["string","null"] }
                    },
                    "required": ["output","mood","salience","summary","memory_type","tags","intent","monologue","reasoning_summary"],
                    "additionalProperties": false
                },
                "strict": true
            });
        }

        debug!("📤 Sending request to GPT-5 Responses API (non-streaming)");
        let response_value = self.post_response(request).await?;

        // Stronger text extraction to handle multiple possible shapes
        let text_content = extract_text_from_responses(&response_value)
            .ok_or_else(|| {
                error!("Failed to extract text from API response. Raw response: {}", serde_json::to_string_pretty(&response_value).unwrap_or_default());
                anyhow!("Failed to extract text from API response")
            })?;

        Ok(ResponseOutput {
            content: text_content,
            raw: Some(response_value),
        })
    }

    /// Stream a response using the GPT-5 Responses API (SSE).
    pub async fn stream_response(
        &self,
        body: serde_json::Value,
    ) -> Result<ResponseStream> {
        self.post_response_stream(body).await
    }

    pub async fn post_response_stream(&self, body: Value) -> Result<ResponseStream> {
        let req = self.client
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

    pub fn request(&self, method: reqwest::Method, endpoint: &str) -> reqwest::RequestBuilder {
        self.client
            .request(method, format!("{}/v1/{}", self.base_url, endpoint))
            .header(header::AUTHORIZATION, format!("Bearer {}", self.api_key))
            .header(header::CONTENT_TYPE, "application/json")
    }

    pub fn request_multipart(&self, endpoint: &str) -> reqwest::RequestBuilder {
        self.client
            .post(format!("{}/v1/{}", self.base_url, endpoint))
            .header(header::AUTHORIZATION, format!("Bearer {}", self.api_key))
    }

    /// Get embedding for text - text-embedding-3-large with 3072 dims
    pub async fn get_embedding(&self, text: &str) -> Result<Vec<f32>> {
        let body = json!({
            "model": "text-embedding-3-large",
            "input": text,
            "dimensions": 3072
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

    pub async fn summarize_conversation(
        &self,
        prompt: &str,
        max_output_tokens: usize,
    ) -> Result<String> {
        let body = json!({
            "model": &self.model,
            "input": [{
                "role": "user",
                "content": [{ "type": "input_text", "text": prompt }]
            }],
            "text": { "verbosity": "low" },
            "reasoning": { "effort": "minimal" },
            "max_output_tokens": max_output_tokens
        });

        debug!("📤 Sending summarization request to GPT-5 Responses API");
        let response_value = self.post_response(body).await?;
        
        extract_text_from_responses(&response_value)
            .ok_or_else(|| anyhow!("Failed to extract summary from API response"))
    }
}

// === strict literal normalizers ===
fn norm_verbosity(v: &str) -> &'static str {
    match v.to_ascii_lowercase().as_str() {
        "low" => "low",
        "medium" => "medium",
        "high" => "high",
        _ => "medium",
    }
}

fn norm_effort(r: &str) -> &'static str {
    match r.to_ascii_lowercase().as_str() {
        "minimal" => "minimal",
        "medium" => "medium",
        "high" => "high",
        _ => "medium",
    }
}

/// Helper function to extract text content from various Responses API shapes.
pub fn extract_text_from_responses(response: &Value) -> Option<String> {
    // New primary path based on logs
    if let Some(text) = response.pointer("/output/1/content/0/text").and_then(|t| t.as_str()) {
        return Some(text.to_string());
    }
    
    // 1) Newer shape: output.message.content[0].text.value
    if let Some(text) = response.pointer("/output/message/content/0/text/value").and_then(|t| t.as_str()) {
        return Some(text.to_string());
    }
    // 2) output.message.content[0].text
    if let Some(text) = response
        .get("output").and_then(|o| o.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|c| c.get(0))
        .and_then(|part| part.get("text"))
        .and_then(|t| t.as_str())
    {
        return Some(text.to_string());
    }
    // 3) message.content[0].text.value
    if let Some(text) = response.pointer("/message/content/0/text/value").and_then(|t| t.as_str()) {
        return Some(text.to_string());
    }
    // 4) message.content[0].text
    if let Some(text) = response
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(|c| c.get(0))
        .and_then(|part| part.get("text"))
        .and_then(|t| t.as_str())
    {
        return Some(text.to_string());
    }
    // 5) Fallback: choices[0].message.content
    if let Some(text) = response.pointer("/choices/0/message/content").and_then(|t| t.as_str()) {
        return Some(text.to_string());
    }
    // 6) Fallback: output as a raw string
    if let Some(text) = response.get("output").and_then(|o| o.as_str()) {
        return Some(text.to_string());
    }
    // 7) Fallback for tool_calls
    if let Some(text) = response.pointer("/choices/0/message/tool_calls/0/function/arguments").and_then(|t| t.as_str()) {
        return Some(text.to_string());
    }

    None
}

/// Helper: Parse SSE stream of JSON into a Stream of Value.
fn sse_json_stream(
    bytes_stream: impl Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Send + 'static,
) -> impl Stream<Item = Result<Value>> + Send {
    use futures::stream;
    
    bytes_stream
        .map(|res| res.map_err(Into::into))
        .scan(Vec::new(), |buffer, res: Result<bytes::Bytes, anyhow::Error>| {
            let bytes = match res {
                Ok(b) => b,
                Err(e) => return futures::future::ready(Some(Some(Err(e)))),
            };

            buffer.extend_from_slice(&bytes);
            let events = parse_sse_from_buffer(buffer);
            futures::future::ready(Some(Some(Ok(events))))
        })
        .filter_map(|item| async move { item })
        .flat_map(|res: Result<Vec<Value>, anyhow::Error>| {
            let items: Vec<Result<Value>> = match res {
                Ok(events) => events.into_iter().map(Ok).collect(),
                Err(e) => vec![Err(e)],
            };
            stream::iter(items)
        })
}

fn parse_sse_from_buffer(buffer: &mut Vec<u8>) -> Vec<Value> {
    let mut events = Vec::new();
    let data = String::from_utf8_lossy(buffer);
    let mut lines = data.lines().peekable();

    while let Some(line) = lines.next() {
        if line.starts_with("data: ") {
            let json_str = &line[6..];
            if json_str == "[DONE]" {
                continue;
            }
            if let Ok(parsed) = serde_json::from_str::<Value>(json_str) {
                events.push(parsed);
            }
        }
    }

    buffer.clear();
    events
}

#[derive(Debug)]
pub struct ResponseOutput {
    pub content: String,
    pub raw: Option<Value>,
}
