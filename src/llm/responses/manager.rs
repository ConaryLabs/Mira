// src/llm/responses/manager.rs
// GPT-5 Responses API manager with proper nested parameter structure

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
    thread_manager: Option<Arc<ThreadManager>>,
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
        self.create_response_with_context(
            model,
            input,
            instructions,
            None,
            response_format,
            parameters,
            None,
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
        let previous_response_id =
            if let (Some(session_id), Some(thread_mgr)) = (session_id, &self.thread_manager) {
                thread_mgr.get_previous_response_id(session_id).await
            } else {
                None
            };

        if let Some(ref prev_id) = previous_response_id {
            debug!("Using previous_response_id: {}", prev_id);
        }

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

        // Merge parameters into the request at top level
        if let Some(params) = parameters {
            if let Some(obj) = params.as_object() {
                for (key, value) in obj {
                    request_body[key] = value.clone();
                }
            }
        }

        if let Some(tools_list) = tools {
            request_body["tools"] = json!(tools_list);
            request_body["tool_choice"] = json!("auto");
        }

        info!("Sending request to GPT-5 Responses API");
        debug!(
            "Request body: {}",
            serde_json::to_string_pretty(&request_body)?
        );

        let response = self
            .client
            .post_response(request_body.clone())
            .await
            .context("Failed to call Responses API")?;

        let response_data: ResponsesResponse =
            serde_json::from_value(response).context("Failed to parse ResponsesResponse")?;

        if let (Some(session_id), Some(thread_mgr)) = (session_id, &self.thread_manager) {
            thread_mgr
                .update_response_id(session_id, response_data.id.clone())
                .await?;
            info!(
                "Updated session {} with response_id: {}",
                session_id, response_data.id
            );
        }

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

        if !function_calls.is_empty() {
            info!(
                "Response contains {} function calls",
                function_calls.len()
            );
            for call in &function_calls {
                debug!("Function call: {:?}", call);
            }
        }

        if let Some(usage) = &response_data.usage {
            info!(
                "Token usage - Prompt: {}, Completion: {}, Total: {}{}",
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

        // Merge parameters at top level
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

    /// Build GPT-5 parameters with CORRECT nested structure for Sept 2025 API
    pub fn build_gpt5_parameters(
        verbosity: &str,
        reasoning_effort: &str,
        max_output_tokens: Option<i32>,
        temperature: Option<f64>,
    ) -> Value {
        let mut params = json!({});

        // Add text configuration with verbosity
        params["text"] = json!({
            "verbosity": verbosity
        });

        // Add reasoning configuration
        params["reasoning"] = json!({
            "effort": reasoning_effort
        });

        // Add max_output_tokens at top level
        if let Some(max_tokens) = max_output_tokens {
            params["max_output_tokens"] = json!(max_tokens);
        }

        // Add temperature at top level
        if let Some(temp) = temperature {
            params["temperature"] = json!(temp);
        }

        params
    }

    /// Build common tools
    pub fn build_standard_tools(
        enable_web_search: bool,
        enable_code_interpreter: bool,
    ) -> Vec<Tool> {
        let mut tools = Vec::new();

        if enable_web_search {
            tools.push(Tool {
                tool_type: "web_search".to_string(),
                function: None,
                web_search: Some(json!({})),
                code_interpreter: None,
            });
        }

        if enable_code_interpreter {
            tools.push(Tool {
                tool_type: "code_interpreter".to_string(),
                function: None,
                web_search: None,
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
            web_search: None,
            code_interpreter: None,
        }
    }
}
