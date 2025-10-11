// src/tools/streaming_orchestrator.rs
// Streaming chat orchestration with real-time event callbacks

use anyhow::Result;
use serde_json::Value;
use std::sync::Arc;
use std::collections::HashMap;
use futures::StreamExt;
use tracing::{debug, info, warn};

use crate::llm::provider::{Message, ToolContext, TokenUsage, StreamEvent};
use crate::llm::provider::gpt5::Gpt5Provider;
use crate::llm::ReasoningConfig;
use crate::tools::ToolExecutor;
use crate::state::AppState;

pub struct StreamingOrchestrator {
    state: Arc<AppState>,
    executor: ToolExecutor,
}

impl StreamingOrchestrator {
    pub fn new(state: Arc<AppState>) -> Self {
        let executor = ToolExecutor::new(
            state.code_intelligence.clone(),
            state.sqlite_pool.clone(),
        );
        
        Self { state, executor }
    }
    
    pub async fn execute_with_tools_streaming<F>(
        &self,
        messages: Vec<Message>,
        system_prompt: String,
        tools: Vec<Value>,
        project_id: Option<&str>,
        mut on_event: F,
    ) -> Result<StreamingResult>
    where
        F: FnMut(StreamEvent) -> Result<()> + Send,
    {
        let mut iteration = 0;
        let max_iterations = 5;
        let mut context: Option<ToolContext> = None;
        let mut collected_artifacts = Vec::new();
        let mut tools_called: Vec<String> = Vec::new();
        
        let mut total_input_tokens = 0;
        let mut total_output_tokens = 0;
        let mut total_reasoning_tokens = 0;
        
        loop {
            iteration += 1;
            if iteration > max_iterations {
                return Err(anyhow::anyhow!("Max iterations reached"));
            }
            
            let (reasoning, verbosity) = if iteration == 1 {
                ReasoningConfig::for_tool_selection()
            } else if !tools_called.is_empty() {
                let tool_refs: Vec<&str> = tools_called.iter().map(|s| s.as_str()).collect();
                ReasoningConfig::for_synthesis_after_tools(&tool_refs)
            } else {
                ReasoningConfig::for_direct_response()
            };
            
            info!("Streaming call {}: reasoning={}, verbosity={}", iteration, reasoning, verbosity);
            
            let provider = self.state.llm_router.get_provider();
            let gpt5_provider = provider.as_any()
                .downcast_ref::<Gpt5Provider>()
                .ok_or_else(|| anyhow::anyhow!("Expected Gpt5Provider"))?;
            
            let mut stream = gpt5_provider.chat_with_tools_streaming(
                if context.is_some() { vec![] } else { messages.clone() },
                system_prompt.clone(),
                tools.clone(),
                context.clone(),
                Some(reasoning),
                Some(verbosity),
            ).await?;
            
            debug!("Stream created successfully, starting to process events");
            
            let mut structured_response_output = String::new();
            let mut tool_calls: HashMap<String, ToolCallBuilder> = HashMap::new();
            let mut response_id = String::new();
            let mut event_count = 0;
            
            while let Some(event_result) = stream.next().await {
                event_count += 1;
                let event = event_result?;
                
                match &event {
                    StreamEvent::TextDelta { delta } => {
                        debug!("TextDelta received (shouldn't happen with custom tools): {}", delta);
                        on_event(event.clone())?;
                    }
                    StreamEvent::ReasoningDelta { delta: _ } => {
                        on_event(event.clone())?;
                    }
                    StreamEvent::ToolCallStart { id, name } => {
                        debug!("Tool call started: {} ({})", name, id);
                        
                        if name == "structured_response" {
                            debug!("Starting structured_response accumulation");
                        } else {
                            tool_calls.insert(id.clone(), ToolCallBuilder {
                                name: name.clone(),
                                arguments: String::new(),
                            });
                        }
                        
                        on_event(event.clone())?;
                    }
                    StreamEvent::ToolCallArgumentsDelta { id, delta } => {
                        if let Some(builder) = tool_calls.get_mut(id) {
                            builder.arguments.push_str(delta);
                        } else if id.contains("structured_response") || delta.contains("OUTPUT:") {
                            structured_response_output.push_str(delta);
                        }
                        
                        on_event(event.clone())?;
                    }
                    StreamEvent::ToolCallComplete { id, name, arguments } => {
                        debug!("Tool call complete: {} ({})", name, id);
                        
                        if name == "structured_response" {
                            structured_response_output = arguments.to_string();
                            debug!("Structured response complete: {} bytes", structured_response_output.len());
                        } else {
                            tool_calls.insert(id.clone(), ToolCallBuilder {
                                name: name.clone(),
                                arguments: arguments.to_string(),
                            });
                        }
                        
                        on_event(event.clone())?;
                    }
                    StreamEvent::Done { response_id: rid, input_tokens, output_tokens, reasoning_tokens, final_text } => {
                        response_id = rid.clone();
                        total_input_tokens += input_tokens;
                        total_output_tokens += output_tokens;
                        total_reasoning_tokens += reasoning_tokens;
                        
                        if let Some(text) = final_text {
                            debug!("Using final_text from Done event: {} bytes", text.len());
                            structured_response_output = text.clone();
                        }
                        
                        info!(
                            "Stream done | input={} output={} reasoning={}",
                            input_tokens, output_tokens, reasoning_tokens
                        );
                        
                        on_event(event.clone())?;
                    }
                    StreamEvent::Error { message } => {
                        warn!("Stream error: {}", message);
                        let msg = message.clone();
                        on_event(event.clone())?;
                        return Err(anyhow::anyhow!("Stream error: {}", msg));
                    }
                }
            }
            
            debug!("Stream finished. Events: {}, structured output: {} bytes", event_count, structured_response_output.len());
            
            context = Some(ToolContext::Gpt5 {
                previous_response_id: response_id.clone(),
            });
            
            if !tool_calls.is_empty() {
                info!("Executing {} tools", tool_calls.len());
                
                tools_called.clear();
                
                for (tool_id, tool_call) in tool_calls.iter() {
                    let tool_name = &tool_call.name;
                    tools_called.push(tool_name.clone());
                    
                    debug!("Executing tool: {} ({})", tool_name, tool_id);
                    
                    let arguments: Value = serde_json::from_str(&tool_call.arguments)
                        .unwrap_or_else(|e| {
                            warn!("Failed to parse tool arguments: {}", e);
                            Value::Object(Default::default())
                        });
                    
                    let result = self.executor.execute_tool(
                        tool_name,
                        &arguments,
                        project_id.unwrap_or(""),
                    ).await?;
                    
                    if tool_name == "create_artifact" {
                        if let Some(artifact) = result.get("artifact") {
                            collected_artifacts.push(artifact.clone());
                        }
                    }
                }
                
                continue;
            }
            
            return Ok(StreamingResult {
                content: structured_response_output,
                artifacts: collected_artifacts,
                tokens: TokenUsage {
                    input: total_input_tokens,
                    output: total_output_tokens,
                    reasoning: total_reasoning_tokens,
                    cached: 0,
                },
            });
        }
    }
}

#[derive(Debug)]
struct ToolCallBuilder {
    name: String,
    arguments: String,
}

#[derive(Debug)]
pub struct StreamingResult {
    pub content: String,
    pub artifacts: Vec<Value>,
    pub tokens: TokenUsage,
}
