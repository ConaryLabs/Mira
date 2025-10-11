// src/llm/provider/deepseek.rs
// DeepSeek provider - specialized for cheap code generation with JSON mode

use anyhow::Result;
use reqwest::Client;
use serde_json::{json, Value};
use tracing::{debug, info, error};

use crate::config::CONFIG;

pub struct DeepSeekProvider {
    client: Client,
}

impl DeepSeekProvider {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
        }
    }
    
    /// Generate code artifact using DeepSeek reasoner with JSON mode
    /// Returns JSON: {"path": "...", "content": "...", "language": "..."}
    pub async fn generate_code_artifact(
        &self,
        tool_input: &Value,
        context: Option<&str>,
    ) -> Result<Value> {
        info!("DeepSeek: Generating code artifact via reasoner");
        
        // Extract parameters from tool input
        let description = tool_input.get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let path = tool_input.get("path")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let language = tool_input.get("language")
            .and_then(|v| v.as_str())
            .unwrap_or("typescript");
        
        // Build prompt for code generation
        let system_prompt = format!(
            "You are a code generation specialist. Generate clean, working code based on the user's requirements.\n\
            Output ONLY valid JSON with this exact structure:\n\
            {{\n  \
              \"path\": \"file/path/here\",\n  \
              \"content\": \"complete file content here\",\n  \
              \"language\": \"typescript|rust|python|javascript\"\n\
            }}\n\n\
            CRITICAL:\n\
            - Generate COMPLETE files, never use '...' or placeholders\n\
            - Include ALL imports, functions, types, and closing braces\n\
            - The content field must contain the entire working file\n\
            - Use proper {} language syntax and best practices",
            language
        );
        
        let user_prompt = if let Some(ctx) = context {
            format!(
                "Generate a {} file at path: {}\n\n\
                Description: {}\n\n\
                Additional context:\n{}\n\n\
                Remember: Output ONLY the JSON object, no other text.",
                language, path, description, ctx
            )
        } else {
            format!(
                "Generate a {} file at path: {}\n\n\
                Description: {}\n\n\
                Remember: Output ONLY the JSON object, no other text.",
                language, path, description
            )
        };
        
        debug!("DeepSeek request: {} chars of context", context.map(|c| c.len()).unwrap_or(0));
        
        // Call DeepSeek API with JSON mode
        let request_body = json!({
            "model": "deepseek-reasoner",
            "messages": [
                {"role": "system", "content": system_prompt},
                {"role": "user", "content": user_prompt}
            ],
            "response_format": {"type": "json_object"},
            "max_tokens": 32000,
        });
        
        let response = self.client
            .post("https://api.deepseek.com/chat/completions")
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", CONFIG.deepseek_api_key))
            .json(&request_body)
            .send()
            .await
            .map_err(|e| {
                error!("DeepSeek API request failed: {}", e);
                anyhow::anyhow!("DeepSeek API request failed: {}", e)
            })?;
        
        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            error!("DeepSeek API error ({}): {}", status, error_text);
            return Err(anyhow::anyhow!("DeepSeek API error ({}): {}", status, error_text));
        }
        
        let response_json: Value = response.json().await
            .map_err(|e| {
                error!("Failed to parse DeepSeek response: {}", e);
                anyhow::anyhow!("Failed to parse DeepSeek response: {}", e)
            })?;
        
        debug!("DeepSeek response: {:?}", response_json);
        
        // Extract content from response
        let content_str = response_json
            .get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
            .ok_or_else(|| {
                error!("Invalid DeepSeek response structure: {:?}", response_json);
                anyhow::anyhow!("Invalid DeepSeek response structure")
            })?;
        
        // Parse the JSON content
        let artifact_json: Value = serde_json::from_str(content_str)
            .map_err(|e| {
                error!("Failed to parse DeepSeek artifact JSON: {} - Content: {}", e, content_str);
                anyhow::anyhow!("Failed to parse DeepSeek artifact JSON: {}", e)
            })?;
        
        // Validate required fields
        if artifact_json.get("path").is_none() || 
           artifact_json.get("content").is_none() || 
           artifact_json.get("language").is_none() {
            error!("DeepSeek artifact missing required fields: {:?}", artifact_json);
            return Err(anyhow::anyhow!("DeepSeek artifact missing required fields"));
        }
        
        info!("DeepSeek: Successfully generated artifact at {}", 
              artifact_json["path"].as_str().unwrap_or("unknown"));
        
        Ok(artifact_json)
    }
    
    /// Check if DeepSeek is configured and available
    pub fn is_available() -> bool {
        CONFIG.use_deepseek_codegen && !CONFIG.deepseek_api_key.is_empty()
    }
}
