// src/llm/client.rs
use reqwest::{Client, Method, RequestBuilder};
use std::env;
use serde_json::{json, Value};
use anyhow::{Result, Context};

#[derive(Clone)]
pub struct OpenAIClient {
    pub client: Client,
    pub api_key: String,
    pub api_base: String,
}

// -------------------- GPT‑5 Responses return type (module scope) --------------------
pub struct Gpt5Response {
    pub raw: Value,
    pub text: String,
}
// ------------------------------------------------------------------------------------

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
    
    pub fn auth_header(&self) -> (&'static str, String) {
        ("Authorization", format!("Bearer {}", self.api_key))
    }
    
    /// Universal request builder for all OpenAI JSON endpoints
    pub fn request(&self, method: Method, path: &str) -> RequestBuilder {
        self.client
            .request(
                method,
                format!("{}/{}", self.api_base.trim_end_matches('/'), path.trim_start_matches('/')),
            )
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
    }
    
    /// Multipart request builder for file uploads (Content-Type set by reqwest)
    pub fn request_multipart(&self, path: &str) -> RequestBuilder {
        self.client
            .post(format!("{}/{}", self.api_base.trim_end_matches('/'), path.trim_start_matches('/')))
            .header("Authorization", format!("Bearer {}", self.api_key))
    }

    // =========================================================================
    // GPT‑5 Responses API (canonical path)
    // =========================================================================

    /// Non‑streaming call to /v1/responses using model "gpt-5".
    /// - `messages` here is the Responses **input** (array of role/content parts)
    /// - `instructions` is your persona/system text
    pub async fn respond_gpt5(
        &self,
        messages: Value,
        instructions: Option<&str>,
        functions: Option<Value>,
        reasoning_effort: Option<&str>,
        verbosity: Option<&str>,
        encrypted_reasoning: Option<Value>,
        max_tokens: Option<u32>,
    ) -> Result<Gpt5Response> {
        let mut body = json!({
            "model": "gpt-5",
            "input": messages, // Responses API expects `input`
        });

        if let Some(instr) = instructions {
            body["instructions"] = json!(instr);
        }
        if let Some(fns) = functions {
            body["functions"] = fns;
        }
        if let Some(effort) = reasoning_effort {
            body["reasoning"] = json!({ "effort": effort });
        }
        if let Some(v) = verbosity {
            // moved per API: text.verbosity
            if body.get("text").is_none() {
                body["text"] = json!({});
            }
            body["text"]["verbosity"] = json!(v);
        }
        if let Some(r) = encrypted_reasoning {
            body["reasoning"]["encrypted_content"] = r;
        }
        if let Some(mt) = max_tokens {
            body["max_tokens"] = json!(mt);
        }

        let response = self
            .request(Method::POST, "responses")
            .json(&body)
            .send()
            .await
            .context("Failed to send GPT-5 responses request")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            tracing::error!(%status, body=%error_text, "OpenAI /responses error");
            return Err(anyhow::anyhow!("OpenAI API error {}: {}", status, error_text));
        }

        let raw: Value = response.json().await.context("Failed to parse GPT-5 response JSON")?;
        let text = extract_text_from_responses(&raw).unwrap_or_default();

        Ok(Gpt5Response { raw, text })
    }

    /// Streaming call to /v1/responses using model "gpt-5".
    /// Returns a stream of JSON chunks; you decide how to surface tokens.
    pub async fn respond_stream_gpt5(
        &self,
        messages: Value,
        instructions: Option<&str>,
        functions: Option<Value>,
        reasoning_effort: Option<&str>,
        verbosity: Option<&str>,
        encrypted_reasoning: Option<Value>,
        max_tokens: Option<u32>,
    ) -> Result<impl futures::Stream<Item = Result<Value>>> {
        use futures::StreamExt;

        let mut body = json!({
            "model": "gpt-5",
            "input": messages, // Responses API expects `input`
            "stream": true
        });

        if let Some(instr) = instructions {
            body["instructions"] = json!(instr);
        }
        if let Some(fns) = functions { body["functions"] = fns; }
        if let Some(effort) = reasoning_effort { body["reasoning"] = json!({ "effort": effort }); }
        if let Some(v) = verbosity {
            if body.get("text").is_none() {
                body["text"] = json!({});
            }
            body["text"]["verbosity"] = json!(v);
        }
        if let Some(r) = encrypted_reasoning { body["reasoning"]["encrypted_content"] = r; }
        if let Some(mt) = max_tokens { body["max_tokens"] = json!(mt); }

        let response = self
            .request(Method::POST, "responses")
            .json(&body)
            .send()
            .await
            .context("Failed to send streaming GPT-5 request")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            tracing::error!(%status, body=%error_text, "OpenAI /responses(stream) error");
            return Err(anyhow::anyhow!("OpenAI API error {}: {}", status, error_text));
        }

        let stream = response.bytes_stream().map(move |chunk| {
            match chunk {
                Ok(bytes) => {
                    // Typical SSE lines prefixed by "data: "
                    let text = String::from_utf8_lossy(&bytes);
                    for line in text.lines() {
                        if let Some(rest) = line.strip_prefix("data: ") {
                            if rest != "[DONE]" {
                                if let Ok(json_data) = serde_json::from_str::<Value>(rest) {
                                    return Ok(json_data);
                                }
                            }
                        }
                    }
                    Ok(json!({})) // ignore keep‑alives/empty lines
                }
                Err(e) => Err(anyhow::anyhow!("Stream error: {}", e))
            }
        });

        Ok(stream)
    }

    // =========================================================================
    // Compatibility shims (no import changes elsewhere)
    // These now delegate to GPT‑5 Responses API.
    // =========================================================================

    /// Kept for call-sites: now maps to GPT‑5 Responses with functions.
    pub async fn chat_with_tools(
        &self,
        messages: Vec<Value>,
        tools: Vec<Value>,
        _tool_choice: Option<Value>,
        _model: Option<&str>,
    ) -> Result<Value> {
        let functions = if tools.is_empty() { None } else { Some(Value::Array(tools)) };

        let resp = self.respond_gpt5(
            Value::Array(messages),
            None,            // instructions
            functions,
            Some("medium"),
            Some("medium"),
            None,
            None
        ).await?;

        Ok(resp.raw)
    }

    /// Kept for call-sites: now maps to GPT‑5 Responses (stream).
    pub async fn stream_chat_with_tools(
        &self,
        messages: Vec<Value>,
        tools: Vec<Value>,
        _tool_choice: Option<Value>,
        _model: Option<&str>,
    ) -> Result<impl futures::Stream<Item = Result<Value>>> {
        let functions = if tools.is_empty() { None } else { Some(Value::Array(tools)) };
        let stream = self.respond_stream_gpt5(
            Value::Array(messages),
            None,            // instructions
            functions,
            Some("medium"),
            Some("medium"),
            None,
            None
        ).await?;
        Ok(stream)
    }

    /// Generate images using gpt-image-1 (kept stable; callers expect urls)
    pub async fn generate_image(
        &self,
        prompt: &str,
        quality: Option<&str>,
    ) -> Result<Vec<String>> {
        let quality = quality.unwrap_or("standard");
        
        // Left on chat/completions to avoid breaking downstream parsing logic.
        let payload = json!({
            "model": "gpt-image-1",
            "messages": [
                { "role": "user", "content": prompt }
            ],
            "modalities": ["text", "image"],
            "image_generation": {
                "n": 1,
                "size": "1024x1024",
                "quality": quality,
                "style": "vivid",
                "response_format": "url"
            }
        });

        let response = self
            .request(Method::POST, "chat/completions")
            .json(&payload)
            .send()
            .await
            .context("Failed to send image generation request")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            return Err(anyhow::anyhow!("Image generation failed ({}): {}", status, error_text));
        }

        let response_json: Value = response.json().await.context("Failed to parse image response")?;
        
        // Extract image URLs from the response
        let mut urls = Vec::new();
        if let Some(choices) = response_json["choices"].as_array() {
            for choice in choices {
                if let Some(message) = choice.get("message") {
                    if let Some(content) = message["content"].as_array() {
                        for item in content {
                            if item["type"] == "image_url" {
                                if let Some(url) = item.get("image_url")
                                                      .and_then(|x| x.get("url"))
                                                      .and_then(|x| x.as_str()) {
                                    urls.push(url.to_string());
                                }
                            }
                        }
                    }
                }
            }
        }

        if urls.is_empty() {
            return Err(anyhow::anyhow!("No images generated in response"));
        }

        Ok(urls)
    }
}

// -----------------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------------

/// Extract assistant text from Responses API payloads.
/// Supports multiple shapes defensively.
pub fn extract_text_from_responses(v: &Value) -> Option<String> {
    // 1) New-style: { output: [ { type: "message", content: [ {type:"output_text"| "text", text:"..."} ] } ] }
    if let Some(arr) = v.get("output").and_then(|o| o.as_array()) {
        for item in arr {
            if item.get("type").and_then(|t| t.as_str()) == Some("message") {
                if let Some(content) = item.get("content").and_then(|c| c.as_array()) {
                    let mut buf = String::new();
                    for part in content {
                        let t = part.get("type").and_then(|t| t.as_str()).unwrap_or_default();
                        if t == "output_text" || t == "text" {
                            if let Some(txt) = part.get("text").and_then(|x| x.as_str()) {
                                if !buf.is_empty() { buf.push('\n'); }
                                buf.push_str(txt);
                            }
                        }
                    }
                    if !buf.is_empty() { return Some(buf); }
                }
            }
        }
    }

    // 2) Chat-completions-like fallback: choices[0].message.content (string)
    if let Some(s) = v.pointer("/choices/0/message/content").and_then(|x| x.as_str()) {
        return Some(s.to_string());
    }

    // 3) Chat-completions-like fallback: choices[0].delta/content (string)
    if let Some(s) = v.pointer("/choices/0/delta/content").and_then(|x| x.as_str()) {
        return Some(s.to_string());
    }

    // 4) Generic: top-level "output_text" (some SDKs expose this)
    if let Some(s) = v.get("output_text").and_then(|x| x.as_str()) {
        return Some(s.to_string());
    }

    None
}
