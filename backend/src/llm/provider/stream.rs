// src/llm/provider/stream.rs
// Stream event types for LLM SSE streaming

use serde_json::Value;

#[derive(Debug, Clone)]
pub enum StreamEvent {
    TextDelta {
        delta: String,
    },
    ReasoningDelta {
        delta: String,
    },
    ToolCallStart {
        id: String,
        name: String,
    },
    ToolCallArgumentsDelta {
        id: String,
        delta: String,
    },
    ToolCallComplete {
        id: String,
        name: String,
        arguments: Value,
    },
    Done {
        response_id: String,
        input_tokens: i64,
        output_tokens: i64,
        reasoning_tokens: i64,
        final_text: Option<String>,
    },
    Error {
        message: String,
    },
}

impl StreamEvent {
    pub fn from_sse_line(line: &str) -> Option<Self> {
        if !line.starts_with("data: ") {
            return None;
        }

        let data = &line[6..];

        if data == "[DONE]" {
            return None;
        }

        let json: Value = match serde_json::from_str(data) {
            Ok(v) => v,
            Err(_) => return None,
        };

        if let Some(status) = json.get("status").and_then(|s| s.as_str()) {
            if status == "completed" {
                let response_id = json["id"].as_str().unwrap_or("").to_string();
                let usage = &json["usage"];

                let final_text = json
                    .pointer("/output/0/arguments")
                    .and_then(|a| a.as_str())
                    .map(|s| s.to_string());

                return Some(StreamEvent::Done {
                    response_id,
                    input_tokens: usage["input_tokens"].as_i64().unwrap_or(0),
                    output_tokens: usage["output_tokens"].as_i64().unwrap_or(0),
                    reasoning_tokens: usage["output_tokens_details"]["reasoning_tokens"]
                        .as_i64()
                        .unwrap_or(0),
                    final_text,
                });
            }
        }

        if let Some(error) = json.get("error") {
            return Some(StreamEvent::Error {
                message: error["message"]
                    .as_str()
                    .unwrap_or("Unknown error")
                    .to_string(),
            });
        }

        if let Some(output_array) = json.get("output").and_then(|o| o.as_array()) {
            for item in output_array {
                let item_type = item.get("type").and_then(|t| t.as_str());

                match item_type {
                    Some("message_delta") => {
                        if let Some(content_array) = item.get("content").and_then(|c| c.as_array())
                        {
                            for content in content_array {
                                if content.get("type").and_then(|t| t.as_str())
                                    == Some("text_delta")
                                {
                                    if let Some(delta) =
                                        content.get("text").and_then(|t| t.as_str())
                                    {
                                        return Some(StreamEvent::TextDelta {
                                            delta: delta.to_string(),
                                        });
                                    }
                                }
                            }
                        }
                    }
                    Some("reasoning_delta") => {
                        if let Some(delta) = item.get("text").and_then(|t| t.as_str()) {
                            return Some(StreamEvent::ReasoningDelta {
                                delta: delta.to_string(),
                            });
                        }
                    }
                    Some("tool_call_delta") | Some("custom_tool_call_delta") => {
                        let id = item
                            .get("id")
                            .or_else(|| item.get("call_id"))
                            .and_then(|i| i.as_str())
                            .unwrap_or("")
                            .to_string();

                        if let Some(name) = item.get("name").and_then(|n| n.as_str()) {
                            return Some(StreamEvent::ToolCallStart {
                                id: id.clone(),
                                name: name.to_string(),
                            });
                        }

                        if let Some(delta) = item
                            .get("arguments_delta")
                            .or_else(|| item.get("delta"))
                            .and_then(|a| a.as_str())
                        {
                            return Some(StreamEvent::ToolCallArgumentsDelta {
                                id,
                                delta: delta.to_string(),
                            });
                        }
                    }
                    Some("tool_call") | Some("custom_tool_call") => {
                        let id = item
                            .get("id")
                            .or_else(|| item.get("call_id"))
                            .and_then(|i| i.as_str())
                            .unwrap_or("")
                            .to_string();
                        let name = item
                            .get("name")
                            .and_then(|n| n.as_str())
                            .unwrap_or("")
                            .to_string();

                        let arguments = item
                            .get("arguments")
                            .or_else(|| item.get("input"))
                            .cloned()
                            .unwrap_or(Value::Null);

                        return Some(StreamEvent::ToolCallComplete {
                            id,
                            name,
                            arguments,
                        });
                    }
                    _ => {}
                }
            }
        }

        None
    }
}
