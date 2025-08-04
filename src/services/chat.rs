// src/services/chat.rs

use crate::llm::client::OpenAIClient;
use crate::persona::PersonaOverlay;
use crate::llm::schema::ChatResponse;
use anyhow::{Result, Context};
use serde_json::json;
use reqwest::Method;
use std::sync::Arc;

pub const DEFAULT_LLM_MODEL: &str = "gpt-4.1";  // Keep as gpt-4.1

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
        eprintln!("🎭 ChatService using persona: {}", persona);
        
        // Build the complete system prompt with Mira's personality
        let mut system_prompt = String::new();
        
        // Add Mira's persona - THIS IS THE KEY FIX!
        system_prompt.push_str(persona.prompt());
        system_prompt.push_str("\n\n");
        
        // Add structured output requirements
        system_prompt.push_str("CRITICAL: Your entire reply MUST be a single valid JSON object with these fields:\n");
        system_prompt.push_str("- output: Your actual reply to the user (string)\n");
        system_prompt.push_str("- persona: The persona overlay in use (string)\n");
        system_prompt.push_str("- mood: The emotional tone of your reply (string)\n");
        system_prompt.push_str("- salience: How emotionally important this reply is (integer 0-10)\n");
        system_prompt.push_str("- summary: Short summary of your reply/context (string or null)\n");
        system_prompt.push_str("- memory_type: \"feeling\", \"fact\", \"joke\", \"promise\", \"event\", or \"other\" (string)\n");
        system_prompt.push_str("- tags: List of context/mood tags (array of strings)\n");
        system_prompt.push_str("- intent: Your intent in this reply (string)\n");
        system_prompt.push_str("- monologue: Your private inner thoughts, not shown to user (string or null)\n");
        system_prompt.push_str("- reasoning_summary: Your reasoning/chain-of-thought, if any (string or null)\n\n");
        system_prompt.push_str("Never add anything before or after the JSON. No markdown, no natural language, no commentary—just the JSON object.\n");

        // Include project context if available
        let user_message = if let Some(proj_id) = project_id {
            format!("[Project: {}]\n{}", proj_id, content)
        } else {
            content.to_string()
        };

        // Debug: Log the full system prompt
        eprintln!("📝 Full system prompt being sent:");
        eprintln!("=====================================");
        eprintln!("{}", system_prompt);
        eprintln!("=====================================");
        eprintln!("📨 User message: {}", user_message);

        // Debug: Log the first part of the system prompt to verify it's Mira
        eprintln!("📝 System prompt preview: {}", 
            system_prompt.chars().take(200).collect::<String>());
        
        let payload = json!({
            "model": DEFAULT_LLM_MODEL,
            "messages": [
                { "role": "system", "content": system_prompt },
                { "role": "user", "content": user_message }
            ],
            // No max_tokens limit - let Mira be herself fully
            "temperature": 0.85,
            "stream": false,
            "response_format": { "type": "json_object" }  // Enforce JSON response
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

        eprintln!("📝 Raw LLM response: {}", output_str.chars().take(200).collect::<String>());

        let chat_response: ChatResponse = serde_json::from_str(&output_str)
            .unwrap_or_else(|e| {
                eprintln!("⚠️ Failed to parse ChatResponse: {}", e);
                eprintln!("   Raw output was: {}", output_str);
                
                // Fallback that still maintains Mira's personality
                ChatResponse {
                    output: output_str.clone(),
                    persona: persona.to_string(),
                    mood: "confused".to_string(),
                    salience: 5,
                    summary: Some(format!("Session: {}", session_id)),
                    memory_type: "other".to_string(),
                    tags: vec!["fallback".to_string()],
                    intent: "chat".to_string(),
                    monologue: Some("My JSON formatting got messed up, but I'm still here!".to_string()),
                    reasoning_summary: None,
                    aside_intensity: None,
                }
            });

        Ok(chat_response)
    }

    /// LLM-powered helper: Use GPT-4 to route a document upload.
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
            // No token limit for routing either
            "temperature": 0.3,
            "stream": false,
        });

        let res = self.llm_client
            .request(Method::POST, "chat/completions")
            .json(&payload)
            .send()
            .await
            .context("Failed to call OpenAI chat API for routing")?
            .error_for_status()
            .context("Non-2xx from OpenAI chat/completions for routing")?
            .json::<serde_json::Value>()
            .await
            .context("Failed to parse OpenAI routing response")?;

        let output = res["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();

        Ok(output)
    }
}
