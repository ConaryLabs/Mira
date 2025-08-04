// src/llm/streaming.rs

use crate::llm::client::OpenAIClient;
use crate::api::ws::message::WsServerMessage;
use crate::tools::web_search::{web_search_tool_definition, ToolCall};
use crate::tools::web_search::handler::WebSearchHandler;
use futures::stream::Stream;
use futures::StreamExt;
use std::pin::Pin;
use serde_json::{json, Value};
use std::sync::Arc;

impl OpenAIClient {
    /// Streams GPT-4.1 response with tool support for WebSocket
    pub async fn stream_gpt4_ws_with_tools(
        &self,
        prompt: String,
        system_prompt: String,
        web_search_handler: Option<Arc<WebSearchHandler>>,
        model: Option<&str>,
    ) -> Pin<Box<dyn Stream<Item = WsServerMessage> + Send + 'static>> {
        let model = model.unwrap_or("gpt-4.1").to_string();
        
        let messages = vec![
            json!({"role": "system", "content": system_prompt}),
            json!({"role": "user", "content": prompt}),
        ];

        // Build tools if web search is available
        let tools = if web_search_handler.is_some() {
            vec![web_search_tool_definition()]
        } else {
            vec![]
        };

        let _api_key = self.api_key.clone();
        let client = self.clone();
        let search_handler = web_search_handler.clone();

        let stream = async_stream::stream! {
            // First call - might request tools
            match client.chat_with_tools(messages.clone(), tools.clone(), None, Some(&model)).await {
                Ok(response) => {
                    // Check for tool calls
                    if let Some(tool_calls) = response["choices"][0]["message"]["tool_calls"].as_array() {
                        // Notify user that search is happening
                        yield WsServerMessage::Chunk {
                            content: "Searching for current information...".to_string(),
                            mood: Some("searching".to_string()),
                        };

                        // Process tool calls
                        let mut updated_messages = messages.clone();
                        updated_messages.push(response["choices"][0]["message"].clone());

                        for tool_call_json in tool_calls {
                            if let Ok(tool_call) = serde_json::from_value::<ToolCall>(tool_call_json.clone()) {
                                if tool_call.function.name == "web_search" {
                                    if let Some(handler) = &search_handler {
                                        match handler.handle_tool_call(&tool_call).await {
                                            Ok(result) => {
                                                updated_messages.push(json!({
                                                    "role": "tool",
                                                    "tool_call_id": tool_call.id,
                                                    "content": result.content,
                                                }));
                                            }
                                            Err(e) => {
                                                eprintln!("Tool call failed: {:?}", e);
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        // Second call with tool results - stream the response
                        let stream_response = client.stream_chat_with_tools(
                            updated_messages,
                            vec![], // No tools for final response
                            None,
                            Some(&model)  // Pass model as &str
                        ).await;

                        match stream_response {
                            Ok(mut response_stream) => {
                                let mut text_buffer = String::new();
                                let current_mood = Some("thoughtful".to_string());
                                
                                while let Some(chunk_result) = response_stream.next().await {
                                    if let Ok(chunk) = chunk_result {
                                        if let Some(delta) = chunk["choices"][0]["delta"]["content"].as_str() {
                                            text_buffer.push_str(delta);
                                            
                                            // Send chunks of reasonable size
                                            if text_buffer.len() > 50 || delta.contains('\n') {
                                                yield WsServerMessage::Chunk {
                                                    content: text_buffer.clone(),
                                                    mood: current_mood.clone(),
                                                };
                                                text_buffer.clear();
                                            }
                                        }
                                    }
                                }
                                
                                // Send any remaining text
                                if !text_buffer.is_empty() {
                                    yield WsServerMessage::Chunk {
                                        content: text_buffer,
                                        mood: current_mood,
                                    };
                                }
                            }
                            Err(e) => {
                                yield WsServerMessage::Error {
                                    message: format!("Stream error: {}", e),
                                    code: Some("STREAM_ERROR".to_string()),
                                };
                            }
                        }
                    } else {
                        // No tools needed - stream direct response
                        let content = response["choices"][0]["message"]["content"]
                            .as_str()
                            .unwrap_or("");
                        
                        // Stream the content in chunks
                        for chunk in content.chars().collect::<Vec<_>>().chunks(100) {
                            yield WsServerMessage::Chunk {
                                content: chunk.iter().collect(),
                                mood: Some("present".to_string()),
                            };
                        }
                    }
                }
                Err(e) => {
                    yield WsServerMessage::Error {
                        message: format!("API error: {}", e),
                        code: Some("API_ERROR".to_string()),
                    };
                }
            }

            // Send done message
            yield WsServerMessage::Done;
        };

        Box::pin(stream)
    }

    /// Original streaming method without tools (backward compatibility)
    pub async fn stream_gpt4_ws_messages(
        &self,
        prompt: String,
        persona: Option<String>,
        system_prompt: String,
        model: Option<&str>,
    ) -> Pin<Box<dyn Stream<Item = WsServerMessage> + Send>> {
        let url = format!("{}/chat/completions", self.api_base);

        let model = model.unwrap_or("gpt-4.1");
        let _persona_string = persona.clone().unwrap_or_else(|| "Default".to_string());

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
                        mood: Some("error".to_string()),
                    };
                    return;
                }
            };

            if !resp.status().is_success() {
                let error_text = resp.text().await.unwrap_or_else(|_| "Unknown error".to_string());
                eprintln!("OpenAI API error: {}", error_text);
                yield WsServerMessage::Chunk {
                    content: format!("I'm having trouble connecting to my thoughts right now. Let me try again in a moment."),
                    mood: Some("concerned".to_string()),
                };
                return;
            }

            let mut byte_stream = resp.bytes_stream();
            let mut partial_line = String::new();
            let mut text_buffer = String::new();
            let mut current_mood = Some("thoughtful".to_string());
            let mut mood_buffer = String::new();
            let mut aside_buffer = String::new();
            let mut in_mood_marker = false;
            let mut in_aside_marker = false;

            while let Some(chunk_result) = byte_stream.next().await {
                let bytes = match chunk_result {
                    Ok(b) => b,
                    Err(e) => {
                        eprintln!("Stream error: {}", e);
                        break;
                    }
                };

                partial_line.push_str(&String::from_utf8_lossy(&bytes));

                // Process complete lines
                while let Some(newline_pos) = partial_line.find('\n') {
                    let line = partial_line[..newline_pos].to_string();
                    partial_line = partial_line[newline_pos + 1..].to_string();

                    if line.starts_with("data: ") {
                        let data = &line[6..];
                        if data == "[DONE]" {
                            break;
                        }

                        if let Ok(json_data) = serde_json::from_str::<Value>(data) {
                            if let Some(delta_content) = json_data["choices"][0]["delta"]["content"].as_str() {
                                // Process markers
                                for ch in delta_content.chars() {
                                    if ch == '⟨' {
                                        in_mood_marker = true;
                                        mood_buffer.clear();
                                    } else if ch == '⟩' && in_mood_marker {
                                        in_mood_marker = false;
                                        if !mood_buffer.is_empty() {
                                            current_mood = Some(mood_buffer.clone());
                                        }
                                    } else if ch == '〈' {
                                        in_aside_marker = true;
                                        aside_buffer.clear();
                                    } else if ch == '〉' && in_aside_marker {
                                        in_aside_marker = false;
                                        if !aside_buffer.is_empty() {
                                            yield WsServerMessage::Aside {
                                                emotional_cue: aside_buffer.clone(),
                                                intensity: Some(0.5),
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

                                // Send chunk when buffer is large enough
                                if text_buffer.len() > 50 {
                                    yield WsServerMessage::Chunk {
                                        content: text_buffer.clone(),
                                        mood: current_mood.clone(),
                                    };
                                    text_buffer.clear();
                                }
                            }
                        }
                    }
                }
            }

            // Handle any remaining content
            if !text_buffer.is_empty() {
                yield WsServerMessage::Chunk {
                    content: text_buffer,
                    mood: current_mood,
                };
            }
        };

        Box::pin(stream)
    }

    /// Streams the chat response as simple text chunks (for internal use)
    pub async fn stream_chat(
        &self,
        message: &str,
        model: &str,
        system_prompt: &str,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<String, anyhow::Error>> + Send>>, anyhow::Error> {
        let url = format!("{}/chat/completions", self.api_base);

        let messages = vec![
            json!({"role": "system", "content": system_prompt}),
            json!({"role": "user", "content": message}),
        ];

        let req_body = json!({
            "model": model,
            "messages": messages,
            "stream": true,
            "temperature": 0.9
        });

        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&req_body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let error_text = resp.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            return Err(anyhow::anyhow!("OpenAI API error: {}", error_text));
        }

        let stream = async_stream::stream! {
            let mut byte_stream = resp.bytes_stream();
            let mut partial_line = String::new();
            
            while let Some(chunk_result) = byte_stream.next().await {
                let bytes = match chunk_result {
                    Ok(b) => b,
                    Err(e) => {
                        yield Err(anyhow::anyhow!("Stream error: {}", e));
                        break;
                    }
                };

                partial_line.push_str(&String::from_utf8_lossy(&bytes));

                while let Some(newline_pos) = partial_line.find('\n') {
                    let line = partial_line[..newline_pos].to_string();
                    partial_line = partial_line[newline_pos + 1..].to_string();

                    if line.starts_with("data: ") {
                        let data = &line[6..];
                        if data == "[DONE]" {
                            break;
                        }

                        if let Ok(json_data) = serde_json::from_str::<Value>(data) {
                            if let Some(content) = json_data["choices"][0]["delta"]["content"].as_str() {
                                yield Ok(content.to_string());
                            }
                        }
                    }
                }
            }
        };

        Ok(Box::pin(stream))
    }
}
