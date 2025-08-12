use std::env;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use serde_json::{json, Value};

use crate::llm::client::OpenAIClient;
use crate::llm::responses::thread::{ResponseMessage, ThreadManager};
use crate::persona::PersonaOverlay;

#[derive(Clone)]
pub struct ChatService {
    client: Arc<OpenAIClient>,
    threads: Arc<ThreadManager>,
    persona: PersonaOverlay,
    model: String,
    default_verbosity: String,       // "low" | "medium" | "high"
    default_reasoning: String,       // "minimal" | "medium" | "high"
    default_max_output_tokens: u32,
    history_message_cap: usize,
}

impl ChatService {
    pub fn new(
        client: Arc<OpenAIClient>,
        threads: Arc<ThreadManager>,
        persona: PersonaOverlay,
    ) -> Self {
        let model = env::var("MIRA_MODEL").unwrap_or_else(|_| "gpt-5".to_string());
        let default_verbosity =
            env::var("MIRA_VERBOSITY").unwrap_or_else(|_| "medium".to_string());
        let default_reasoning =
            env::var("MIRA_REASONING_EFFORT").unwrap_or_else(|_| "medium".to_string());
        let default_max_output_tokens: u32 = env::var("MIRA_MAX_OUTPUT_TOKENS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(1024);
        let history_message_cap: usize = env::var("MIRA_HISTORY_CAP")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(24);

        Self {
            client,
            threads,
            persona,
            model,
            default_verbosity,
            default_reasoning,
            default_max_output_tokens,
            history_message_cap,
        }
    }

    pub async fn process_message(
        &self,
        thread_id: &str,
        user_text: &str,
        structured_json: bool,
    ) -> Result<ChatResult> {
        self.threads
            .add_message(thread_id, ResponseMessage::user(user_text))
            .await?;

        // Old signature only takes &str; we fetch all then cap locally.
        let mut history = self.threads.get_conversation(thread_id).await;
        if history.len() > self.history_message_cap {
            let start = history.len() - self.history_message_cap;
            history = history.split_off(start);
        }

        let input_messages = self.build_gpt5_input(&history)?;
        let instructions = self.persona.prompt();

        let parameters = json!({
            "verbosity": self.default_verbosity,
            "reasoning_effort": self.default_reasoning,
            "max_output_tokens": self.default_max_output_tokens,
            "persona": self.persona.name(),
            "temperature": self.persona.temperature()
        });

        let response_format = if structured_json {
            json!({ "type": "json_object" })
        } else {
            json!({ "type": "text" })
        };

        let body = json!({
            "model": self.model,
            "input": input_messages,
            "instructions": instructions,
            "parameters": parameters,
            "response_format": response_format
        });

        let v = self.client.post_response(body).await?;

        let text = extract_output_text(&v)
            .or_else(|| extract_message_text(&v))
            .unwrap_or_default();

        if text.is_empty() {
            return Err(anyhow!(
                "GPT‑5 returned no output text for thread '{}'",
                thread_id
            ));
        }

        self.threads
            .add_message(thread_id, ResponseMessage::assistant(&text))
            .await?;

        Ok(ChatResult {
            thread_id: thread_id.to_string(),
            text,
            raw: v,
        })
    }

    pub async fn process_message_gpt5(
        &self,
        thread_id: &str,
        user_text: &str,
        structured_json: bool,
    ) -> Result<ChatResult> {
        self.process_message(thread_id, user_text, structured_json).await
    }

    fn build_gpt5_input(&self, history: &[ResponseMessage]) -> Result<Vec<Value>> {
        let mut out = Vec::with_capacity(history.len());
        for msg in history {
            let role = if msg.role == "user" { "user" } else { "assistant" };
            let text = msg
                .content
                .as_deref()
                .ok_or_else(|| anyhow!("empty message content in history"))?;
            out.push(json!({
                "role": role,
                "content": [
                    { "type": if role == "user" { "input_text" } else { "output_text" }, "text": text }
                ]
            }));
        }
        Ok(out)
    }
}

pub struct ChatResult {
    pub thread_id: String,
    pub text: String,
    pub raw: Value,
}

fn extract_output_text(v: &Value) -> Option<String> {
    let arr = v.get("output")?.as_array()?;
    let mut buf = String::new();
    for item in arr {
        let is_text = item
            .get("type")
            .and_then(|t| t.as_str())
            .map(|t| t == "output_text")
            .unwrap_or(false);
        if is_text {
            if let Some(s) = item.get("text").and_then(|t| t.as_str()) {
                buf.push_str(s);
            }
        }
    }
    if buf.is_empty() { None } else { Some(buf) }
}

fn extract_message_text(v: &Value) -> Option<String> {
    let parts = v.pointer("/choices/0/message/content")?.as_array()?;
    let mut buf = String::new();
    for part in parts {
        let is_text = part
            .get("type")
            .and_then(|t| t.as_str())
            .map(|t| t == "output_text")
            .unwrap_or(false);
        if is_text {
            if let Some(s) = part.get("text").and_then(|t| t.as_str()) {
                buf.push_str(s);
            }
        }
    }
    if buf.is_empty() { None } else { Some(buf) }
}

// convenience constructors if you don't already have them
impl ResponseMessage {
    pub fn user(text: &str) -> Self {
        Self { role: "user".into(), content: Some(text.to_string()) }
    }
    pub fn assistant(text: &str) -> Self {
        Self { role: "assistant".into(), content: Some(text.to_string()) }
    }
}
