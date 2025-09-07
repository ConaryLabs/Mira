// src/llm/responses/manager.rs
// Updated for GPT-5 Responses API - August 15, 2025
// Changes:
// - Added previous_response_id support for conversation continuity
// - Integrated with ThreadManager for response ID tracking
// - Added tool calling support
// - Streamlined response generation (removed duplication with OpenAIClient)

use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::sync::Arc;
use tracing::{debug, info};

use crate::llm::client::OpenAIClient;
use crate::llm::responses::thread::ThreadManager;
use crate::llm::responses::types::{
    CodeInterpreterConfig, ContainerConfig, FunctionDefinition, Message, ResponsesResponse, Tool,
};

/// High-level manager for the GPT-5 Responses API
pub struct ResponsesManager {
    client: Arc<OpenAIClient>,
    thread_manager: Option<Arc<ThreadManager>>, // Optional thread manager for response ID tracking
}

impl ResponsesManager {
    /// Create a new ResponsesManager
    pub fn new(client: Arc<OpenAIClient>) -> Self {
        Self {
            client,
            thread_manager: None,
        }
    }

    /// Create a ResponsesManager with ThreadManager for response ID tracking
    pub fn with_thread_manager(
        client: Arc<OpenAIClient>,
        thread_manager: Arc<ThreadManager>,
    ) -> Self {
        Self {
            client,
            thread_manager: Some(thread_manager),
        }
    }

    /// Create a response using the Responses API with full parameter support
    pub async fn create_response(
        &self,
        model: &str,
        input: Vec<Message>,
        instructions: Option<String>,
        response_format: Option<Value>,
        parameters: Option<Value>,
    ) -> Result<String> {
        // This method maintains backward compatibility
        self.create_response_with_context(
            model,
            input,
            instructions,
            None, // No session_id for backward compatibility
            response_format,
            parameters,
            None, // No tools
        )
        .await
    }

    /// Create a response with session tracking and previous_response_id
    pub async fn create_response_with_context(
        &self,
        model: &str,
        input: Vec<Message>,
        instructions: Option<String>,
        session_id: Option<&str>,
        response_format: Option<Value>,
        parameters: Option<Value>,
        tools: Option<Vec<Tool>>,
    ) -> Result<String> {
        // Get previous_response_id if we have a session
        let previous_response_id =
            if let (Some(session_id), Some(thread_mgr)) = (session_id, &self.thread_manager) {
                thread_mgr.get_previous_response_id(session_id).await
            } else {
                None
            };

        if let Some(ref prev_id) = previous_response_id {
            debug!("Using previous_response_id: {}", prev_id);
        }

        // Build the request
        let mut request_body = json!({
            "model": model,
            "input": input,
        });

        if let Some(inst) = instructions {
            request_body["instructions"] = json!(inst);
        }

        if let Some(prev_id) = previous_response_id {
            request_body["previous_response_id"] = json!(prev_id);
        }

        if let Some(fmt) = response_format {
            request_body["response_format"] = fmt;
        }

        // Merge parameters into the request
        if let Some(params) = parameters {
            if let Some(obj) = params.as_object() {
                for (key, value) in obj {
                    request_body[key] = value.clone();
                }
            }
        }

        if let Some(tools_list) = tools {
            request_body["tools"] = json!(tools_list);
            request_body["tool_choice"] = json!("auto"); // Default to auto
        }

        info!("📤 Sending request to GPT-5 Responses API");
        debug!(
            "Request body: {}",
            serde_json::to_string_pretty(&request_body)?
        );

        // Send the request using the client's post_response method
        let response = self
            .client
            .post_response(request_body.clone())
            .await
            .context("Failed to call Responses API")?;

        // Parse the response
        let response_data: ResponsesResponse =
            serde_json::from_value(response).context("Failed to parse ResponsesResponse")?;

        // Update the previous_response_id for the session
        if let (Some(session_id), Some(thread_mgr)) = (session_id, &self.thread_manager) {
            thread_mgr
                .update_response_id(session_id, response_data.id.clone())
                .await?;
            info!(
                "✅ Updated session {} with response_id: {}",
                session_id, response_data.id
            );
        }

        // Extract the text content
        let mut output_text = String::new();
        let mut function_calls = Vec::new();

        for item in &response_data.output {
            match item.output_type.as_str() {
                "text" => {
                    if let Some(text) = &item.text {
                        output_text.push_str(text);
                    }
                }
                "function_call" | "tool_call" => {
                    function_calls.push(item.clone());
                }
                _ => {
                    debug!("Unknown output type: {}", item.output_type);
                }
            }
        }

        // Handle function calls if present
        if !function_calls.is_empty() {
            info!(
                "🔧 Response contains {} function calls",
                function_calls.len()
            );
            for call in &function_calls {
                debug!("Function call: {:?}", call);
            }
        }

        // Log usage if available
        if let Some(usage) = &response_data.usage {
            info!(
                "📊 Token usage - Prompt: {}, Completion: {}, Total: {}{}",
                usage.prompt_tokens,
                usage.completion_tokens,
                usage.total_tokens,
                usage
                    .reasoning_tokens
                    .map(|r| format!(", Reasoning: {r}"))
                    .unwrap_or_default()
            );
        }

        Ok(output_text)
    }

    /// Create a streaming response
    pub async fn create_streaming_response(
        &self,
        model: &str,
        input: Vec<Message>,
        instructions: Option<String>,
        session_id: Option<&str>,
        parameters: Option<Value>,
    ) -> Result<impl futures::Stream<Item = Result<Value>>> {
        let previous_response_id =
            if let (Some(session_id), Some(thread_mgr)) = (session_id, &self.thread_manager) {
                thread_mgr.get_previous_response_id(session_id).await
            } else {
                None
            };

        let mut request_body = json!({
            "model": model,
            "input": input,
            "stream": true,
        });

        if let Some(inst) = instructions {
            request_body["instructions"] = json!(inst);
        }

        if let Some(prev_id) = previous_response_id {
            request_body["previous_response_id"] = json!(prev_id);
        }

        if let Some(params) = parameters {
            if let Some(obj) = params.as_object() {
                for (key, value) in obj {
                    request_body[key] = value.clone();
                }
            }
        }

        self.client
            .post_response_stream(request_body)
            .await
            .context("Failed to create streaming response")
    }

    /// Helper to build standard GPT-5 parameters
    pub fn build_gpt5_parameters(
        verbosity: &str,
        reasoning_effort: &str,
        max_output_tokens: Option<i32>,
        temperature: Option<f64>, // <-- FIX: Changed from f32 to f64
    ) -> Value {
        let mut params = json!({
            "verbosity": verbosity,
            "reasoning_effort": reasoning_effort,
        });

        if let Some(max_tokens) = max_output_tokens {
            params["max_output_tokens"] = json!(max_tokens);
        }

        if let Some(temp) = temperature {
            params["temperature"] = json!(temp);
        }

        params
    }

    /// Helper to build common tools
    pub fn build_standard_tools(
        enable_web_search: bool,
        enable_code_interpreter: bool,
    ) -> Vec<Tool> {
        let mut tools = Vec::new();

        if enable_web_search {
            tools.push(Tool {
                tool_type: "web_search_preview".to_string(),
                function: None,
                web_search_preview: Some(json!({})),
                code_interpreter: None,
            });
        }

        if enable_code_interpreter {
            tools.push(Tool {
                tool_type: "code_interpreter".to_string(),
                function: None,
                web_search_preview: None,
                code_interpreter: Some(CodeInterpreterConfig {
                    container: ContainerConfig {
                        container_type: "auto".to_string(),
                    },
                }),
            });
        }

        tools
    }

    /// Build a custom function tool
    pub fn build_function_tool(
        name: &str,
        description: &str,
        parameters: Value,
    ) -> Tool {
        Tool {
            tool_type: "function".to_string(),
            function: Some(FunctionDefinition {
                name: name.to_string(),
                description: description.to_string(),
                parameters,
            }),
            web_search_preview: None,
            code_interpreter: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_gpt5_parameters() {
        let params = ResponsesManager::build_gpt5_parameters(
            "medium",
            "high",
            Some(4096),
            Some(0.7),
        );

        assert_eq!(params["verbosity"], "medium");
        assert_eq!(params["reasoning_effort"], "high");
        assert_eq!(params["max_output_tokens"], 4096);

        // THE FIX: Compare floating-point numbers for approximate equality.
        let temp = params["temperature"].as_f64().unwrap();
        assert!((temp - 0.7).abs() < f64::EPSILON);
    }

    #[test]
    fn test_build_standard_tools() {
        let tools = ResponsesManager::build_standard_tools(true, true);
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].tool_type, "web_search_preview");
        assert_eq!(tools[1].tool_type, "code_interpreter");
    }
}
