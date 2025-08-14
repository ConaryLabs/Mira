// src/llm/responses/manager.rs
use std::sync::Arc;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tracing::{debug, error, info};

use crate::llm::client::OpenAIClient;

#[derive(Clone)]
pub struct ResponsesManager {
    client: Arc<OpenAIClient>,
    responses_id: Option<String>,
}

impl ResponsesManager {
    pub fn new(client: Arc<OpenAIClient>) -> Self {
        Self { client, responses_id: None }
    }

    pub async fn create_response(
        &self,
        model: &str,
        messages: Vec<Value>,
        instructions: Option<String>,
        response_format: Option<Value>,  // kept for compatibility
        parameters: Option<Value>,       // verbosity, reasoning_effort, max_output_tokens
    ) -> Result<ResponseObject> {
        let mut body = json!({ "model": model, "input": messages });

        if let Some(ref instr) = instructions {
            body["instructions"] = Value::String(instr.clone());
        }

        // ---- Parameters (validated) ----------------------------------------------------------
        let mut verbosity = "medium";
        let mut reasoning_effort = "medium";
        let mut max_output_tokens: usize = 128_000;

        if let Some(params) = &parameters {
            if let Some(v) = params.get("verbosity").and_then(|v| v.as_str()) {
                verbosity = match v.trim() {
                    "low" => "low",
                    "high" => "high",
                    _ => "medium",
                };
            }
            if let Some(r) = params.get("reasoning_effort").and_then(|r| r.as_str()) {
                reasoning_effort = match r.trim() {
                    "minimal" | "low" => "low",
                    "high" => "high",
                    _ => "medium",
                };
            }
            if let Some(m) = params.get("max_output_tokens") {
                if let Some(num) = m.as_u64() {
                    max_output_tokens = num as usize;
                } else if let Some(num) = m.as_i64() {
                    max_output_tokens = num as usize;
                }
            }
        }

        // ---- Text / format -------------------------------------------------------------------
        let mut text_obj = json!({ "verbosity": verbosity });

        if let Some(fmt) = response_format {
            if fmt.get("type").and_then(|t| t.as_str()) == Some("json_object") {
                text_obj["format"] = json!({ "type": "json_object" });
            }
        }
        body["text"] = text_obj.clone();

        // ---- Reasoning / tokens --------------------------------------------------------------
        body["reasoning"] = json!({ "effort": reasoning_effort });
        body["max_output_tokens"] = json!(max_output_tokens);

        // ---- Debug request -------------------------------------------------------------------
        info!("🔍 Sending request to OpenAI:");
        info!("   Model: {}", model);
        info!("   Verbosity: {}", verbosity);
        info!("   Reasoning effort: {}", reasoning_effort);
        info!("   Max output tokens: {}", max_output_tokens);
        info!("   Has instructions: {}", instructions.is_some());
        info!("   Has format: {}", text_obj.get("format").is_some());
        info!("   Input messages count: {}", messages.len());

        if let Some(first_msg) = messages.first() {
            debug!("   First message: {}", first_msg.to_string());
        }
        debug!(
            "📤 Full request body: {}",
            serde_json::to_string_pretty(&body).unwrap_or_default()
        );

        // ---- Call API (non-streaming) --------------------------------------------------------
        let v = match self.client.post_response(body).await {
            Ok(response) => {
                info!("✅ Got response from OpenAI");
                response
            }
            Err(e) => {
                error!("❌ OpenAI API error: {:?}", e);
                return Err(e);
            }
        };

        // ---- Extract output (GPT‑5 Responses shape) -----------------------------------------
        if let Some(output_val) = v.get("output").cloned() {
            debug!(
                "🔎 output: {}",
                serde_json::to_string_pretty(&output_val).unwrap_or_default()
            );
            let text = extract_text_from_output(&output_val);
            info!("📝 Extracted text from output (length: {} chars)", text.len());
            return Ok(ResponseObject { raw: v, text });
        }

        // ---- Fallback: legacy Chat Completions shapes ----------------------------------------
        let text = if let Some(content) = v.pointer("/choices/0/message/content") {
            if let Some(arr) = content.as_array() {
                let mut buf = String::new();
                for part in arr {
                    let ptype = part.get("type").and_then(|t| t.as_str());
                    if ptype == Some("output_text") || ptype == Some("text") {
                        if let Some(t) = part.get("text").and_then(|t| t.as_str()) {
                            if !buf.is_empty() { buf.push('\n'); }
                            buf.push_str(t);
                        }
                    } else if ptype == Some("output_json") {
                        if let Some(j) = part.get("json") {
                            if !buf.is_empty() { buf.push('\n'); }
                            buf.push_str(&j.to_string());
                        }
                    }
                }
                buf
            } else {
                content.as_str().unwrap_or_default().to_string()
            }
        } else {
            // tool call arguments as last-ditch (string)
            v.pointer("/choices/0/message/tool_calls/0/function/arguments")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string()
        };

        info!(
            "📝 Extracted text from fallback format (length: {} chars)",
            text.len()
        );
        Ok(ResponseObject { raw: v, text })
    }

    pub fn get_responses_id(&self) -> Option<&str> {
        self.responses_id.as_deref()
    }
}

/// Robust extractor for GPT‑5 Responses payloads.
/// Handles:
/// - Top-level parts: [{ type: "output_text", text }, { type:"message", content:[...] }, ...]
/// - Nested message.content[*] parts with type "output_text" | "text" | "output_json"
/// - Direct item.text strings
fn extract_text_from_output(output: &Value) -> String {
    fn push_line(buf: &mut String, s: &str) {
        if !buf.is_empty() {
            buf.push('\n');
        }
        buf.push_str(s);
    }

    if let Some(arr) = output.as_array() {
        let mut s = String::new();
        for item in arr {
            let itype = item.get("type").and_then(|t| t.as_str());

            // A) Direct part at top level
            if itype == Some("output_text") {
                if let Some(t) = item.get("text").and_then(|t| t.as_str()) {
                    push_line(&mut s, t);
                    continue;
                }
            }
            if let Some(t) = item.get("text").and_then(|t| t.as_str()) {
                // Some SDKs may still return a direct "text" field
                push_line(&mut s, t);
                continue;
            }

            // B) Wrapped message with content parts
            if itype == Some("message") || item.get("content").is_some() {
                if let Some(content) = item.get("content").and_then(|c| c.as_array()) {
                    for part in content {
                        let ptype = part.get("type").and_then(|t| t.as_str());
                        match ptype {
                            Some("output_text") | Some("text") => {
                                if let Some(t) = part.get("text").and_then(|v| v.as_str()) {
                                    push_line(&mut s, t);
                                }
                            }
                            Some("output_json") => {
                                if let Some(j) = part.get("json") {
                                    push_line(&mut s, &j.to_string());
                                }
                            }
                            _ => {
                                // ignore other part types for now
                            }
                        }
                    }
                    continue;
                }
            }
        }
        return s;
    }

    // Unexpected shapes: try a few safe fallbacks
    if let Some(s) = output.get("text").and_then(|v| v.as_str()) {
        return s.to_string();
    }
    String::new()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseObject {
    pub text: String,
    #[serde(skip)]
    pub raw: Value,
}
