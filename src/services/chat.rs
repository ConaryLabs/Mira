use crate::llm::client::OpenAIClient;
use crate::persona::PersonaOverlay;
use crate::llm::schema::ChatResponse;
use anyhow::{Result, Context};
use serde_json::json;
use reqwest::Method;
use std::sync::Arc;

pub const DEFAULT_LLM_MODEL: &str = "gpt-4.1";

#[derive(Clone)]
pub struct ChatService {
    pub llm_client: Arc<OpenAIClient>,
}

impl ChatService {
    pub fn new(llm_client: Arc<OpenAIClient>) -> Self {
        Self { llm_client }
    }

    /// Runs a user message through the LLM (Mira) and returns a structured response.
    pub async fn process_message(
        &self,
        session_id: &str,
        content: &str,
        persona: &PersonaOverlay,
        project_id: Option<&str>,
    ) -> Result<ChatResponse> {
        let prompt = format!(
            "{}\n\n[Persona: {:?}]\n[Session ID: {}]\n[Project: {:?}]",
            content, persona, session_id, project_id
        );

        let payload = json!({
            "model": DEFAULT_LLM_MODEL,
            "messages": [
                { "role": "system", "content": "You are Mira. Be witty, warm, and real. Respond like a human friend, not a bot." },
                { "role": "user", "content": prompt }
            ],
            "max_tokens": 2048,
            "temperature": 0.85,
            "stream": false,
        });

        let res = self.llm_client
            .request(Method::POST, "chat/completions")
            .json(&payload)
            .send()
            .await
            .context("Failed to call OpenAI chat API")?
            .error_for_status()
            .context("Non-2xx from OpenAI chat/completions")?
            .json::<serde_json::Value>()
            .await
            .context("Failed to parse OpenAI chat API response")?;

        let output_str = res["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();

        let chat_response: ChatResponse = serde_json::from_str(&output_str)
            .unwrap_or_else(|_| ChatResponse {
                output: output_str.clone(),
                persona: persona.to_string(),
                mood: "neutral".to_string(),
                salience: 5,
                summary: None,
                memory_type: "other".to_string(),
                tags: vec![],
                intent: "chat".to_string(),
                monologue: None,
                reasoning_summary: None,
                aside_intensity: None, // <--- Correct type
            });

        Ok(chat_response)
    }

    /// LLM-powered helper: Use GPT-4.1 to route a document upload.
    pub async fn run_routing_inference(
        &self,
        system_prompt: &str,
        user_prompt: &str,
    ) -> Result<String> {
        let payload = json!({
            "model": DEFAULT_LLM_MODEL,
            "messages": [
                { "role": "system", "content": system_prompt },
                { "role": "user", "content": user_prompt }
            ],
            "max_tokens": 8,
            "temperature": 0.0,
            "stream": false,
        });

        let res = self.llm_client
            .request(Method::POST, "chat/completions")
            .json(&payload)
            .send()
            .await
            .context("Failed to call OpenAI for document routing")?
            .error_for_status()
            .context("Non-2xx from OpenAI for routing")?
            .json::<serde_json::Value>()
            .await
            .context("Failed to parse OpenAI routing response")?;

        let text = res["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();

        Ok(text)
    }

    /// Embedding helper for hybrid memory/context.
    pub async fn get_embedding(&self, content: &str) -> Result<Vec<f32>> {
        let payload = json!({
            "input": content,
            "model": "text-embedding-3-large"
        });

        let res = self.llm_client
            .request(Method::POST, "embeddings")
            .json(&payload)
            .send()
            .await?
            .error_for_status()?
            .json::<serde_json::Value>()
            .await?;

        let embedding = res["data"][0]["embedding"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("Malformed embedding response"))?
            .iter()
            .map(|v| v.as_f64().unwrap_or(0.0) as f32)
            .collect();

        Ok(embedding)
    }
}
