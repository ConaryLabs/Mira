// src/tools/streaming_orchestrator.rs
// Streaming chat orchestration with real-time event callbacks
//
// Usage:
//   let orchestrator = StreamingOrchestrator::new(app_state);
//   let result = orchestrator.execute_with_tools_streaming(
//       messages,
//       system_prompt,
//       tools,
//       Some("project-id"),
//       |event| {
//           // Forward event to WebSocket
//           match event {
//               StreamEvent::TextDelta { delta } => { /* send to client */ }
//               StreamEvent::Done { .. } => { /* stream complete */ }
//               _ => {}
//           }
//           Ok(())
//       },
//   ).await?;

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
    
    /// Execute chat with streaming + tool loop
    /// The callback is called for each stream event (text deltas, tool calls, etc)
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
        let mut accumulated_text = String::new();
        let mut collected_artifacts = Vec::new();
        let mut tools_called: Vec<String> = Vec::new();
        
        // Track total tokens across iterations
        let mut total_input_tokens = 0;
        let mut total_output_tokens = 0;
        let mut total_reasoning_tokens = 0;
        
        loop {
            iteration += 1;
            if iteration > max_iterations {
                return Err(anyhow::anyhow!("Max iterations reached"));
            }
            
            // Determine reasoning/verbosity based on iteration
            let (reasoning, verbosity) = if iteration == 1 {
                ReasoningConfig::for_tool_selection()
            } else if !tools_called.is_empty() {
                let tool_refs: Vec<&str> = tools_called.iter().map(|s| s.as_str()).collect();
                ReasoningConfig::for_synthesis_after_tools(&tool_refs)
            } else {
                ReasoningConfig::for_direct_response()
            };
            
            info!("Streaming call {}: reasoning={}, verbosity={}", iteration, reasoning, verbosity);
            
            // Get provider and start streaming
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
            
            // Process stream events
            let mut stream_text = String::new();
            let mut tool_calls: HashMap<String, ToolCallBuilder> = HashMap::new();
            let mut response_id = String::new();
            
            while let Some(event_result) = stream.next().await {
                let event = event_result?;
                
                match &event {
                    StreamEvent::TextDelta { delta } => {
                        stream_text.push_str(delta);
                        // Forward to callback for real-time display
                        on_event(event.clone())?;
                    }
                    StreamEvent::ReasoningDelta { delta: _ } => {
                        // Forward reasoning deltas (optional display)
                        on_event(event.clone())?;
                    }
                    StreamEvent::ToolCallStart { id, name } => {
                        debug!("Tool call started: {} ({})", name, id);
                        tool_calls.insert(id.clone(), ToolCallBuilder {
                            name: name.clone(),
                            arguments: String::new(),
                        });
                        on_event(event.clone())?;
                    }
                    StreamEvent::ToolCallArgumentsDelta { id, delta } => {
                        if let Some(builder) = tool_calls.get_mut(id) {
                            builder.arguments.push_str(delta);
                        }
                        on_event(event.clone())?;
                    }
                    StreamEvent::ToolCallComplete { id, name, arguments } => {
                        debug!("Tool call complete: {} ({})", name, id);
                        tool_calls.insert(id.clone(), ToolCallBuilder {
                            name: name.clone(),
                            arguments: arguments.to_string(),
                        });
                        on_event(event.clone())?;
                    }
                    StreamEvent::Done { response_id: rid, input_tokens, output_tokens, reasoning_tokens } => {
                        response_id = rid.clone();
                        total_input_tokens += input_tokens;
                        total_output_tokens += output_tokens;
                        total_reasoning_tokens += reasoning_tokens;
                        
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
            
            // Save response_id for next iteration
            context = Some(ToolContext::Gpt5 {
                previous_response_id: response_id.clone(),
            });
            
            accumulated_text = stream_text.clone();
            
            // Execute tool calls if any
            if !tool_calls.is_empty() {
                info!("Executing {} tools", tool_calls.len());
                
                tools_called.clear();
                
                for (tool_id, tool_call) in tool_calls.iter() {
                    let tool_name = &tool_call.name;
                    tools_called.push(tool_name.clone());
                    
                    debug!("Executing tool: {} ({})", tool_name, tool_id);
                    
                    // Parse arguments
                    let arguments: Value = serde_json::from_str(&tool_call.arguments)
                        .unwrap_or_else(|e| {
                            warn!("Failed to parse tool arguments: {}", e);
                            Value::Object(Default::default())
                        });
                    
                    // Execute tool
                    let result = self.executor.execute_tool(
                        tool_name,
                        &arguments,
                        project_id.unwrap_or(""),
                    ).await?;
                    
                    // Collect artifacts
                    if tool_name == "create_artifact" {
                        if let Some(artifact) = result.get("artifact") {
                            collected_artifacts.push(artifact.clone());
                        }
                    }
                }
                
                // Continue loop for synthesis
                continue;
            }
            
            // No tools - done
            return Ok(StreamingResult {
                content: accumulated_text,
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

/// Builder for accumulating tool call arguments from stream
#[derive(Debug)]
struct ToolCallBuilder {
    name: String,
    arguments: String,
}

/// Result of streaming orchestration
#[derive(Debug)]
pub struct StreamingResult {
    pub content: String,
    pub artifacts: Vec<Value>,
    pub tokens: TokenUsage,
}
