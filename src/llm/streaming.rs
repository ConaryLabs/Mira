// src/llm/streaming.rs

use crate::llm::client::OpenAIClient;
use crate::api::ws::message::WsServerMessage;
use futures::stream::Stream;
use futures::StreamExt;
use std::pin::Pin;
use serde_json::{json, Value};

impl OpenAIClient {
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
                        // Flush any remaining content
                        if !text_buffer.is_empty() {
                            let trimmed = text_buffer.trim();
                            if !trimmed.is_empty() && 
                               !(trimmed.len() == 1 && trimmed.chars().next().unwrap().is_ascii_punctuation()) {
                                yield WsServerMessage::Chunk {
                                    content: trimmed.to_string(),
                                    persona: persona_string.clone(),
                                    mood: current_mood,
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
                        // Process content character by character for mood/aside markers
                        for ch in content.chars() {
                            if ch == '⟨' && !in_mood_marker {
                                // Start of mood marker
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
                                // End of mood marker
                                in_mood_marker = false;
                                if !mood_buffer.is_empty() {
                                    current_mood = Some(mood_buffer.clone());
                                }
                            } else if ch == '⟦' && !in_aside_marker {
                                // Start of aside marker
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
                            }
                        }
                    }
                }
            }

            // Handle any remaining partial line
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

            // Final cleanup
            if !text_buffer.is_empty() {
                let trimmed = text_buffer.trim();
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
