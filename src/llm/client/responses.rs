// src/llm/client/responses.rs

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::{debug, error};

#[derive(Debug, Clone)]
pub struct ResponseOutput {
    pub content: String,
    pub raw: Option<Value>,
}

impl ResponseOutput {
    pub fn new(content: String) -> Self {
        Self {
            content,
            raw: None,
        }
    }

    pub fn with_raw(content: String, raw: Value) -> Self {
        Self {
            content,
            raw: Some(raw),
        }
    }
}

pub fn extract_text_from_responses(response: &Value) -> Option<String> {
    // PRIMARY PATH: Legacy response format parsing
    // output[1].content[0].text (where output[0] is reasoning, output[1] is message)
                        if let Some(text) = first_content.get("text").and_then(|t| t.as_str()) {
                            debug!("Extracted text using: output[message].content[0].text");
                            return Some(text.to_string());
                        }
                    }
                }
            }
        }
    }
    
    // FALLBACK: Try the old paths for backwards compatibility
    
    // Try /output/1/content/0/text directly (based on logs)
    {
        debug!("Extracted text using: output.message.content[0].text");
        return Some(text.to_string());
    }
    
    // message.content[0].text.value
    {
        debug!("Extracted text using: message.content[0].text");
        return Some(text.to_string());
    }
    
    // Fallback: choices[0].message.content (older API format)
            }
            
            if let Some(content_array) = item.get("content").and_then(|c| c.as_array()) {
                for content_item in content_array {
                    if let Some(text) = content_item.get("text").and_then(|t| t.as_str()) {
                        debug!("Extracted text using: output[{}].content[].text", i);
                        return Some(text.to_string());
                    }
                    if content_item.get("type").and_then(|t| t.as_str()) == Some("output_text") {
                        if let Some(text) = content_item.get("text").and_then(|t| t.as_str()) {
                            debug!("Extracted text using: output array format");
                            return Some(text.to_string());
                        }
                    }
                }
            }
        }
    }

    error!("Failed to extract text from response. Tried all known extraction paths.");
    None
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: FunctionCall,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageInfo {
    pub prompt_tokens: i32,
    pub completion_tokens: i32,
    pub total_tokens: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_tokens: Option<i32>,
}

pub fn extract_tool_calls(response: &Value) -> Vec<ToolCall> {
    let mut tool_calls = Vec::new();

    }

            }
        }
    }

    tool_calls
}

pub fn extract_usage_info(response: &Value) -> Option<UsageInfo> {

pub fn validate_response(response: &Value) -> Result<()> {
    if response.get("error").is_some() {

    if response.get("output").is_none() && response.get("choices").is_none() {
        return Err(anyhow!("Response missing both 'output' and 'choices' fields"));
    }

    Ok(())
}
