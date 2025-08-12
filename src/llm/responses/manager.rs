use std::sync::Arc;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

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
        response_format: Option<Value>,
        parameters: Option<Value>,
    ) -> Result<ResponseObject> {
        let mut body = json!({ "model": model, "input": messages });
        if let Some(instr) = instructions { body["instructions"] = Value::String(instr); }
        if let Some(fmt) = response_format { body["response_format"] = fmt; }
        if let Some(params) = parameters { body["parameters"] = params; }

        let v = self.client.post_response(body).await?;

        if let Some(output_val) = v.get("output").cloned() {
            let text = extract_text_from_output(&output_val);
            return Ok(ResponseObject { raw: v, text });
        }

        let text = v
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
            .unwrap_or_default();

        Ok(ResponseObject { raw: v, text })
    }

    pub fn get_responses_id(&self) -> Option<&str> {
        self.responses_id.as_deref()
    }
}

fn extract_text_from_output(output: &Value) -> String {
    if let Some(arr) = output.as_array() {
        let mut s = String::new();
        for item in arr {
            if item.get("type").and_then(|t| t.as_str()) == Some("output_text") {
                if let Some(t) = item.get("text").and_then(|t| t.as_str()) {
                    s.push_str(t);
                }
            }
        }
        return s;
    }
    String::new()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseObject {
    pub text: String,
    #[serde(skip)]
    pub raw: Value,
}
