// src/llm/provider/deepseek.rs
// DeepSeek Reasoner 3.2 provider - specialized for code generation with structured output

use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tracing::{debug, error, info, warn};

use crate::config::CONFIG;
use super::Message;

/// DeepSeek provider for code generation
#[derive(Clone)]
pub struct DeepSeekProvider {
    client: Client,
    api_key: String,
}

impl DeepSeekProvider {
    pub fn new(api_key: String) -> Self {
        Self {
            client: Client::new(),
            api_key,
        }
    }

    /// Check if DeepSeek is configured and available
    pub fn is_available(&self) -> bool {
        CONFIG.use_deepseek_codegen && !self.api_key.is_empty()
    }

    /// Generate code artifact using DeepSeek Reasoner 3.2 with structured output
    pub async fn generate_code(&self, request: CodeGenRequest) -> Result<CodeGenResponse> {
        info!(
            "DeepSeek: Generating {} code at {}",
            request.language, request.path
        );

        let system_prompt = format!(
            "You are a code generation specialist. Generate clean, working code based on the user's requirements.\n\
            Output ONLY valid JSON with this exact structure:\n\
            {{\n  \
              \"path\": \"file/path/here\",\n  \
              \"content\": \"complete file content here\",\n  \
              \"language\": \"{}\",\n  \
              \"explanation\": \"brief explanation of the code\"\n\
            }}\n\n\
            CRITICAL:\n\
            - Generate COMPLETE files, never use '...' or placeholders\n\
            - Include ALL imports, functions, types, and closing braces\n\
            - The content field must contain the entire working file\n\
            - Use proper {} language syntax and best practices",
            request.language, request.language
        );

        let user_prompt = build_user_prompt(&request);

        debug!("DeepSeek user prompt:\n{}", user_prompt);

        // Call DeepSeek API with JSON mode
        let request_body = json!({
            "model": "deepseek-reasoner",
            "messages": [
                {"role": "system", "content": system_prompt},
                {"role": "user", "content": user_prompt}
            ],
            "response_format": {"type": "json_object"},
            "temperature": 0.7,
            "max_tokens": 32000,
        });

        let response = self
            .client
            .post("https://api.deepseek.com/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await
            .context("DeepSeek API request failed")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            error!("DeepSeek API error {}: {}", status, error_text);
            return Err(anyhow::anyhow!(
                "DeepSeek API error {}: {}",
                status,
                error_text
            ));
        }

        let response_json: Value = response
            .json()
            .await
            .context("Failed to parse DeepSeek response")?;

        debug!(
            "DeepSeek raw response: {}",
            serde_json::to_string_pretty(&response_json).unwrap_or_default()
        );

        // Extract content from response
        let content_str = response_json
            .get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
            .ok_or_else(|| anyhow::anyhow!("Invalid DeepSeek response structure"))?;

        // Parse the JSON content
        let artifact: CodeArtifact = serde_json::from_str(content_str)
            .with_context(|| format!("Failed to parse DeepSeek artifact JSON: {}", content_str))?;

        // Extract token usage
        let usage = response_json.get("usage");
        let tokens_input = usage
            .and_then(|u| u.get("prompt_tokens"))
            .and_then(|t| t.as_i64())
            .unwrap_or(0);
        let tokens_output = usage
            .and_then(|u| u.get("completion_tokens"))
            .and_then(|t| t.as_i64())
            .unwrap_or(0);

        info!(
            "DeepSeek: Generated {} lines of code at {}",
            artifact.content.lines().count(),
            artifact.path
        );

        Ok(CodeGenResponse {
            artifact,
            tokens_input,
            tokens_output,
        })
    }
}

/// Request to generate code
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeGenRequest {
    pub path: String,
    pub description: String,
    pub language: String,
    pub framework: Option<String>,
    pub dependencies: Vec<String>,
    pub style_guide: Option<String>,
    pub context: String, // Additional context from memory, files, etc.
}

/// Response from code generation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeGenResponse {
    pub artifact: CodeArtifact,
    pub tokens_input: i64,
    pub tokens_output: i64,
}

/// Code artifact generated by DeepSeek
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeArtifact {
    pub path: String,
    pub content: String,
    pub language: String,
    #[serde(default)]
    pub explanation: Option<String>,
}

/// Build user prompt from request (public for testing)
pub fn build_user_prompt(request: &CodeGenRequest) -> String {
    let mut prompt = format!(
        "Generate a {} file at path: {}\n\n\
        Description: {}\n\n",
        request.language, request.path, request.description
    );

    if let Some(framework) = &request.framework {
        prompt.push_str(&format!("Framework: {}\n\n", framework));
    }

    if !request.dependencies.is_empty() {
        prompt.push_str(&format!(
            "Dependencies: {}\n\n",
            request.dependencies.join(", ")
        ));
    }

    if let Some(style) = &request.style_guide {
        prompt.push_str(&format!("Style preferences: {}\n\n", style));
    }

    if !request.context.is_empty() {
        prompt.push_str(&format!("Additional context:\n{}\n\n", request.context));
    }

    prompt.push_str("Remember: Output ONLY the JSON object, no other text.");

    prompt
}

/// Response from DeepSeek tool calling API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallResponse {
    pub content: Option<String>,
    pub tool_calls: Vec<ToolCall>,
    pub finish_reason: String,
    pub tokens_input: i64,
    pub tokens_output: i64,
}

/// Individual tool call from DeepSeek
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

impl DeepSeekProvider {
    /// Call DeepSeek with tool calling support
    ///
    /// Uses `deepseek-chat` model (NOT `deepseek-reasoner`) for function calling.
    /// Handles tool_calls in response format.
    pub async fn call_with_tools(
        &self,
        messages: Vec<Message>,
        tools: Vec<Value>,
    ) -> Result<ToolCallResponse> {
        info!(
            "DeepSeek: Calling with {} tools, {} messages",
            tools.len(),
            messages.len()
        );

        // Convert our Message format to DeepSeek API format
        let api_messages: Vec<Value> = messages
            .iter()
            .map(|msg| json!({
                "role": msg.role,
                "content": msg.content
            }))
            .collect();

        let request_body = json!({
            "model": "deepseek-chat",  // Use chat model for tool calling
            "messages": api_messages,
            "tools": tools,
            "temperature": 0.7,
            "max_tokens": 16000,
        });

        debug!(
            "DeepSeek tool calling request:\n{}",
            serde_json::to_string_pretty(&request_body).unwrap_or_default()
        );

        let response = self
            .client
            .post("https://api.deepseek.com/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await
            .context("DeepSeek API request failed")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            error!("DeepSeek API error {}: {}", status, error_text);
            return Err(anyhow::anyhow!(
                "DeepSeek API error {}: {}",
                status,
                error_text
            ));
        }

        let response_json: Value = response
            .json()
            .await
            .context("Failed to parse DeepSeek response")?;

        debug!(
            "DeepSeek tool calling response:\n{}",
            serde_json::to_string_pretty(&response_json).unwrap_or_default()
        );

        // Extract usage information
        let usage = response_json.get("usage");
        let tokens_input = usage
            .and_then(|u| u.get("prompt_tokens"))
            .and_then(|t| t.as_i64())
            .unwrap_or(0);
        let tokens_output = usage
            .and_then(|u| u.get("completion_tokens"))
            .and_then(|t| t.as_i64())
            .unwrap_or(0);

        // Extract the choice
        let choice = response_json
            .get("choices")
            .and_then(|c| c.get(0))
            .ok_or_else(|| anyhow::anyhow!("No choices in DeepSeek response"))?;

        let message = choice
            .get("message")
            .ok_or_else(|| anyhow::anyhow!("No message in DeepSeek choice"))?;

        let finish_reason = choice
            .get("finish_reason")
            .and_then(|f| f.as_str())
            .unwrap_or("unknown")
            .to_string();

        // Extract content (may be null if tool calls are present)
        let content = message
            .get("content")
            .and_then(|c| c.as_str())
            .map(|s| s.to_string());

        // Extract tool calls if present
        let mut tool_calls = Vec::new();
        if let Some(calls) = message.get("tool_calls").and_then(|t| t.as_array()) {
            for call in calls {
                let id = call
                    .get("id")
                    .and_then(|i| i.as_str())
                    .ok_or_else(|| anyhow::anyhow!("Missing tool call id"))?
                    .to_string();

                let function = call
                    .get("function")
                    .ok_or_else(|| anyhow::anyhow!("Missing function in tool call"))?;

                let name = function
                    .get("name")
                    .and_then(|n| n.as_str())
                    .ok_or_else(|| anyhow::anyhow!("Missing function name"))?
                    .to_string();

                let arguments_str = function
                    .get("arguments")
                    .and_then(|a| a.as_str())
                    .ok_or_else(|| anyhow::anyhow!("Missing function arguments"))?;

                let arguments: Value = serde_json::from_str(arguments_str)
                    .with_context(|| format!("Failed to parse tool arguments: {}", arguments_str))?;

                tool_calls.push(ToolCall {
                    id,
                    name,
                    arguments,
                });
            }
        }

        if !tool_calls.is_empty() {
            info!(
                "DeepSeek returned {} tool call(s): {:?}",
                tool_calls.len(),
                tool_calls.iter().map(|tc| &tc.name).collect::<Vec<_>>()
            );
        } else if let Some(ref text) = content {
            info!("DeepSeek returned text response: {} chars", text.len());
        } else {
            warn!("DeepSeek returned neither tool calls nor content");
        }

        Ok(ToolCallResponse {
            content,
            tool_calls,
            finish_reason,
            tokens_input,
            tokens_output,
        })
    }
}

// Tests in tests/phase5_providers_test.rs
