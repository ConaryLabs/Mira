// src/llm/memory_eval.rs

use crate::llm::client::OpenAIClient;
use crate::llm::schema::{EvaluateMemoryRequest, EvaluateMemoryResponse};
use anyhow::{Result, anyhow};
use serde_json::json;

impl OpenAIClient {
    /// Runs GPT-4.1 function-calling for memory evaluation.
    pub async fn evaluate_memory(&self, req: &EvaluateMemoryRequest) -> Result<EvaluateMemoryResponse> {
        let url = format!("{}/chat/completions", self.api_base);

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

        let args_json = resp_json["choices"][0]["message"]["function_call"]["arguments"]
            .as_str()
            .ok_or_else(|| anyhow!("No function_call arguments found in LLM response"))?;

        let result: EvaluateMemoryResponse = serde_json::from_str(args_json)?;

        Ok(result)
    }
}
