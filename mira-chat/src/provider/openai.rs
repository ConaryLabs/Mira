//! OpenAI provider implementation (Responses API)
//!
//! Wraps the existing responses.rs client with the Provider trait.

use anyhow::Result;
use async_trait::async_trait;
use tokio::sync::mpsc;

use super::{
    Capabilities, ChatRequest, ChatResponse, FinishReason, Provider, StreamEvent,
    ToolCall, ToolContinueRequest, Usage,
};
use crate::responses::{self, Client as ResponsesClient, Tool as ResponsesTool};

/// OpenAI provider using the Responses API
pub struct OpenAiProvider {
    client: ResponsesClient,
    capabilities: Capabilities,
}

impl OpenAiProvider {
    /// Create a new OpenAI provider
    pub fn new(api_key: String) -> Self {
        Self {
            client: ResponsesClient::new(api_key),
            capabilities: Capabilities::openai_responses(),
        }
    }

    /// Get the underlying client for direct access if needed
    pub fn client(&self) -> &ResponsesClient {
        &self.client
    }

    /// Convert our tool definitions to Responses API format
    fn convert_tools(tools: &[super::ToolDefinition]) -> Vec<ResponsesTool> {
        tools
            .iter()
            .map(|t| ResponsesTool {
                tool_type: "function".into(),
                name: t.name.clone(),
                description: Some(t.description.clone()),
                parameters: t.parameters.clone(),
            })
            .collect()
    }

    /// Convert Responses API stream events to our unified events
    fn convert_stream_event(event: responses::StreamEvent) -> Option<StreamEvent> {
        match event {
            responses::StreamEvent::TextDelta(text) => Some(StreamEvent::TextDelta(text)),
            responses::StreamEvent::FunctionCallStart { name, call_id } => {
                Some(StreamEvent::FunctionCallStart { call_id, name })
            }
            responses::StreamEvent::FunctionCallDelta {
                call_id,
                arguments_delta,
            } => Some(StreamEvent::FunctionCallDelta {
                call_id,
                arguments_delta,
            }),
            responses::StreamEvent::FunctionCallDone { call_id, .. } => {
                Some(StreamEvent::FunctionCallEnd { call_id })
            }
            responses::StreamEvent::Done(response) => {
                // Extract usage from the final response
                if let Some(u) = response.usage {
                    // We could emit usage here, but for simplicity just signal done
                    Some(StreamEvent::Done)
                } else {
                    Some(StreamEvent::Done)
                }
            }
            responses::StreamEvent::Error(e) => Some(StreamEvent::Error(e)),
        }
    }
}

#[async_trait]
impl Provider for OpenAiProvider {
    fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    fn name(&self) -> &'static str {
        "openai"
    }

    async fn create_stream(
        &self,
        request: ChatRequest,
    ) -> Result<mpsc::Receiver<StreamEvent>> {
        let tools = Self::convert_tools(&request.tools);
        let effort = request.reasoning_effort.as_deref().unwrap_or("medium");

        // Use the existing client's streaming method
        let rx = self
            .client
            .create_stream(
                &request.input,
                &request.system,
                request.previous_response_id.as_deref(),
                effort,
                &request.model,
                &tools,
            )
            .await?;

        // Convert stream events
        let (tx, out_rx) = mpsc::channel(100);
        tokio::spawn(async move {
            let mut rx = rx;
            while let Some(event) = rx.recv().await {
                if let Some(converted) = Self::convert_stream_event(event) {
                    if tx.send(converted).await.is_err() {
                        break;
                    }
                }
            }
        });

        Ok(out_rx)
    }

    async fn create(&self, request: ChatRequest) -> Result<ChatResponse> {
        let tools = Self::convert_tools(&request.tools);
        let effort = request.reasoning_effort.as_deref().unwrap_or("medium");

        // Note: responses.rs create() hardcodes gpt-5.2, doesn't take model param
        let response = self
            .client
            .create(
                &request.input,
                &request.system,
                request.previous_response_id.as_deref(),
                effort,
                &tools,
            )
            .await?;

        // Convert response
        let mut text = String::new();
        let mut tool_calls = Vec::new();

        for item in &response.output {
            if let Some(t) = item.text() {
                text = t;
            }
            if let Some((name, args, call_id)) = item.as_function_call() {
                tool_calls.push(ToolCall {
                    call_id: call_id.to_string(),
                    name: name.to_string(),
                    arguments: args.to_string(),
                });
            }
        }

        let finish_reason = if !tool_calls.is_empty() {
            FinishReason::ToolCalls
        } else {
            FinishReason::Stop
        };

        let usage = response.usage.map(|u| Usage {
            input_tokens: u.input_tokens,
            output_tokens: u.output_tokens,
            reasoning_tokens: u.reasoning_tokens(),
            cached_tokens: u.cached_tokens(),
        });

        Ok(ChatResponse {
            id: response.id,
            text,
            reasoning: None, // Responses API doesn't expose reasoning in non-streaming
            tool_calls,
            usage,
            finish_reason,
        })
    }

    async fn continue_with_tools_stream(
        &self,
        request: ToolContinueRequest,
    ) -> Result<mpsc::Receiver<StreamEvent>> {
        let tools = Self::convert_tools(&request.tools);
        let effort = request.reasoning_effort.as_deref().unwrap_or("low");

        // Convert tool results to (call_id, output) pairs
        let results: Vec<(String, String)> = request
            .tool_results
            .iter()
            .map(|r| (r.call_id.clone(), r.output.clone()))
            .collect();

        let prev_id = request
            .previous_response_id
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("OpenAI provider requires previous_response_id"))?;

        let rx = self
            .client
            .continue_with_tool_results_stream(
                prev_id,
                results,
                &request.system,
                effort,
                &request.model,
                &tools,
            )
            .await?;

        // Convert stream events
        let (tx, out_rx) = mpsc::channel(100);
        tokio::spawn(async move {
            let mut rx = rx;
            while let Some(event) = rx.recv().await {
                if let Some(converted) = Self::convert_stream_event(event) {
                    if tx.send(converted).await.is_err() {
                        break;
                    }
                }
            }
        });

        Ok(out_rx)
    }
}
