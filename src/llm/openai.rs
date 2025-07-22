// src/llm/openai.rs

//! Low-level OpenAI API client for embeddings, moderation, function-calling.
//! No wrappers; just reqwest and Rust, as the universe intended.

use reqwest::Client;
use anyhow::{Result, anyhow};
use serde_json::json;
use std::env;

use crate::llm::schema::{EvaluateMemoryRequest, EvaluateMemoryResponse};

#[derive(Clone)]
pub struct OpenAIClient {
    pub client: Client,
    pub api_key: String,
    pub api_base: String, // Default "https://api.openai.com/v1", but can be overridden
}

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

    fn auth_header(&self) -> (&'static str, String) {
        ("Authorization", format!("Bearer {}", self.api_key))
    }

    /// Gets OpenAI text embedding (1536d) for a string.
    pub async fn get_embedding(&self, text: &str) -> Result<Vec<f32>> {
        let url = format!("{}/embeddings", self.api_base);
        let req_body = json!({
            "input": text,
            "model": "text-embedding-3-large"
        });
        let resp = self
            .client
            .post(&url)
            .header(self.auth_header().0, self.auth_header().1.clone())
            .json(&req_body)
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(anyhow!("OpenAI embedding failed: {}", resp.text().await.unwrap_or_default()));
        }
        let resp_json: serde_json::Value = resp.json().await?;
        // Extract embedding (1536 floats)
        let embedding = resp_json["data"][0]["embedding"]
            .as_array()
            .ok_or_else(|| anyhow!("No embedding in OpenAI response"))?
            .iter()
            .map(|v| v.as_f64().unwrap_or(0.0) as f32)
            .collect();

        Ok(embedding)
    }

    /// Runs moderation API on user/Mira message, returns flagged true/false.
    pub async fn moderate_message(&self, text: &str) -> Result<bool> {
        let url = format!("{}/moderations", self.api_base);
        let req_body = json!({ "input": text });
        let resp = self
            .client
            .post(&url)
            .header(self.auth_header().0, self.auth_header().1.clone())
            .json(&req_body)
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(anyhow!("OpenAI moderation failed: {}", resp.text().await.unwrap_or_default()));
        }
        let resp_json: serde_json::Value = resp.json().await?;
        let flagged = resp_json["results"][0]["flagged"].as_bool().unwrap_or(false);
        Ok(flagged)
    }

    /// Runs GPT-4.1 function-calling for memory evaluation.
    /// Calls LLM with system prompt, user message, and function schema, gets back structured metadata.
    pub async fn evaluate_memory(&self, req: &EvaluateMemoryRequest) -> Result<EvaluateMemoryResponse> {
        let url = format!("{}/chat/completions", self.api_base);

        // Compose the system prompt (feel free to refine this for your project's voice)
        let system_prompt = r#"You are an emotionally intelligent AI. For every message you receive, extract the following:
- Salience (how important is this to the user's emotional world, 1-10)
- Tags (context, relationships, mood)
- A one-sentence summary (optional)
- Memory type (choose one: feeling, fact, joke, promise, event, or other)
Use only the message, its context, and your intuition—do not rely on keywords. Return your answer as a valid JSON object conforming to the schema."#;

        let messages = vec![
            json!({"role": "system", "content": system_prompt}),
            json!({"role": "user", "content": req.content}),
        ];

        let function_schema = req.function_schema.clone();

        let body = json!({
            "model": "gpt-4.1",
            "messages": messages,
            "functions": [function_schema],
            "function_call": { "name": "evaluate_memory" },
            "response_format": { "type": "json_object" },
            "temperature": 0.2
        });

        let resp = self
            .client
            .post(&url)
            .header(self.auth_header().0, self.auth_header().1.clone())
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(anyhow!(
                "OpenAI LLM call failed: {}",
                resp.text().await.unwrap_or_default()
            ));
        }
        let resp_json: serde_json::Value = resp.json().await?;

        // Extract function_call.arguments as JSON and parse into EvaluateMemoryResponse
        let args_json = resp_json["choices"][0]["message"]["function_call"]["arguments"]
            .as_str()
            .ok_or_else(|| anyhow!("No function_call arguments found in LLM response"))?;

        let result: EvaluateMemoryResponse = serde_json::from_str(args_json)?;

        Ok(result)
    }
}
