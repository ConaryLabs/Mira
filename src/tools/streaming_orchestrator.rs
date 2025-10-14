// src/tools/streaming_orchestrator.rs
// Streaming chat orchestration with real-time event callbacks
// Owns prompt building - handler just passes raw context

use anyhow::Result;
use serde_json::{json, Value};
use std::sync::Arc;
use std::collections::HashMap;
use futures::StreamExt;
use tracing::{debug, info, warn};

use crate::llm::provider::{Message, ToolContext, TokenUsage, StreamEvent, DeepSeekProvider};
use crate::llm::provider::gpt5::Gpt5Provider;
use crate::llm::ReasoningConfig;
use crate::tools::ToolExecutor;
use crate::state::AppState;
use crate::memory::features::recall_engine::RecallContext;
use crate::persona::PersonaOverlay;
use crate::api::ws::message::MessageMetadata;
use crate::prompt::unified_builder::UnifiedPromptBuilder;
use crate::config::CONFIG;

pub struct StreamingOrchestrator {
    state: Arc<AppState>,
    executor: ToolExecutor,
    deepseek_provider: Option<DeepSeekProvider>,
}

impl StreamingOrchestrator {
    pub fn new(state: Arc<AppState>) -> Self {
        let executor = ToolExecutor::new(
            state.code_intelligence.clone(),
            state.sqlite_pool.clone(),
        );
        
        // Initialize DeepSeek provider if API key is configured
        let deepseek_provider = if DeepSeekProvider::is_available() {
            info!("DeepSeek provider enabled for code generation");
            Some(DeepSeekProvider::new())
        } else {
            info!("DeepSeek provider disabled (no API key or flag is false)");
            None
        };
        
        Self { 
            state, 
            executor,
            deepseek_provider,
        }
    }
    
    /// Execute a streaming chat with tool support
    /// Handles multi-turn tool execution loop with automatic synthesis
    pub async fn execute_with_tools_streaming<F>(
        &self,
        messages: Vec<Message>,
        persona: PersonaOverlay,
        context: RecallContext,
        tools: Vec<Value>,
        metadata: Option<MessageMetadata>,
        project_id: Option<&str>,
        mut on_event: F,
    ) -> Result<StreamingResult>
    where
        F: FnMut(StreamEvent) -> Result<()> + Send,
    {
        // Build system prompt with persona, memory, and project context
        let system_prompt = UnifiedPromptBuilder::build_system_prompt(
            &persona,
            &context,
            None, // tools are passed separately to LLM
            metadata.as_ref(),
            project_id,
        );
        
        debug!("System prompt built: {} chars", system_prompt.len());
        
        let mut iteration = 0;
        let max_iterations = CONFIG.tool_max_iterations;
        let mut context_obj: Option<ToolContext> = None;
        let mut collected_artifacts = Vec::new();
        let mut tools_called: Vec<String> = Vec::new();
        
        let mut total_input_tokens = 0;
        let mut total_output_tokens = 0;
        let mut total_reasoning_tokens = 0;
        
        loop {
            iteration += 1;

            // Safety valve: force final synthesis if we exceed max iterations
            // This prevents infinite tool-calling loops
            if iteration > max_iterations {
                warn!("Hit max iterations ({}) - forcing final synthesis without tools", max_iterations);
                
                let provider = self.state.llm_router.get_provider();
                let gpt5_provider = provider.as_any()
                    .downcast_ref::<Gpt5Provider>()
                    .ok_or_else(|| anyhow::anyhow!("Expected Gpt5Provider"))?;

                // Final pass: disable tools to prevent any more calls
                let mut stream = gpt5_provider.chat_with_tools_streaming(
                    vec![],                       // no new user/assistant messages
                    system_prompt.clone(),
                    vec![],                       // no tools - force synthesis
                    context_obj.clone(),          // include all tool outputs
                    Some("high"),
                    Some("high"),
                ).await?;

                let mut structured_response_output = String::new();
                
                while let Some(event_result) = stream.next().await {
                    let event = event_result?;
                    match &event {
                        StreamEvent::TextDelta { delta } => {
                            structured_response_output.push_str(delta);
                            on_event(event.clone())?;
                        }
                        StreamEvent::ReasoningDelta { .. } => {
                            on_event(event.clone())?;
                        }
                        StreamEvent::Done { input_tokens, output_tokens, reasoning_tokens, final_text, .. } => {
                            total_input_tokens += input_tokens;
                            total_output_tokens += output_tokens;
                            total_reasoning_tokens += reasoning_tokens;

                            // Use final_text as fallback if we didn't accumulate anything
                            if structured_response_output.is_empty() {
                                if let Some(text) = final_text {
                                    debug!("Using final_text from Done event: {} bytes", text.len());
                                    structured_response_output = text.clone();
                                }
                            }

                            info!(
                                "Final synthesis done | input={} output={} reasoning={} | text_length={}",
                                input_tokens, output_tokens, reasoning_tokens, structured_response_output.len()
                            );
                            on_event(event.clone())?;
                        }
                        StreamEvent::ToolCallArgumentsDelta { .. }
                        | StreamEvent::ToolCallStart { .. }
                        | StreamEvent::ToolCallComplete { .. } => {
                            // Tools are disabled in final synthesis; ignore any calls
                        }
                        StreamEvent::Error { message } => {
                            warn!("Stream error during final synthesis: {}", message);
                            let msg = message.clone();
                            on_event(event.clone())?;
                            return Err(anyhow::anyhow!("Stream error: {}", msg));
                        }
                    }
                }

                debug!("Final forced synthesis - returning {} bytes of JSON", structured_response_output.len());
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
            
            // Adaptive reasoning: adjust thinking depth based on context
            // First turn: evaluate which tools to use
            // After tools: synthesize results into coherent response
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
                if context_obj.is_some() { vec![] } else { messages.clone() },
                system_prompt.clone(),
                tools.clone(),
                context_obj.clone(),
                Some(reasoning),
                Some(verbosity),
            ).await?;
            
            let mut response_id = String::new();
            let mut tool_calls: HashMap<String, ToolCallBuilder> = HashMap::new();
            let mut structured_response_output = String::new();
            let mut event_count = 0;
            
            // Process streaming events from LLM
            while let Some(event_result) = stream.next().await {
                event_count += 1;
                let event = event_result?;
                
                match &event {
                    StreamEvent::TextDelta { delta } => {
                        // Accumulate text for structured response (json_schema format)
                        structured_response_output.push_str(delta);
                        on_event(event.clone())?;
                    }
                    StreamEvent::ReasoningDelta { delta: _ } => {
                        on_event(event.clone())?;
                    }
                    StreamEvent::ToolCallArgumentsDelta { id, delta } => {
                        tool_calls.entry(id.clone())
                            .or_insert_with(|| ToolCallBuilder {
                                name: String::new(),
                                arguments: String::new(),
                                call_id: id.clone(),
                            })
                            .arguments
                            .push_str(delta);
                        
                        on_event(event.clone())?;
                    }
                    StreamEvent::ToolCallStart { id, name } => {
                        debug!("Tool call started: {} ({})", name, id);
                        
                        tool_calls.insert(id.clone(), ToolCallBuilder {
                            name: name.clone(),
                            arguments: String::new(),
                            call_id: id.clone(),
                        });
                        
                        on_event(event.clone())?;
                    }
                    StreamEvent::ToolCallComplete { id, name, arguments } => {
                        debug!("Tool call complete: {} ({})", name, id);
                        
                        tool_calls.insert(id.clone(), ToolCallBuilder {
                            name: name.clone(),
                            arguments: arguments.to_string(),
                            call_id: id.clone(),
                        });
                        
                        on_event(event.clone())?;
                    }
                    StreamEvent::Done { response_id: rid, input_tokens, output_tokens, reasoning_tokens, final_text } => {
                        response_id = rid.clone();
                        total_input_tokens += input_tokens;
                        total_output_tokens += output_tokens;
                        total_reasoning_tokens += reasoning_tokens;
                        
                        // Use final_text if available (fallback if accumulated text is empty)
                        if structured_response_output.is_empty() {
                            if let Some(text) = final_text {
                                debug!("Using final_text from Done event: {} bytes", text.len());
                                structured_response_output = text.clone();
                            }
                        }
                        
                        info!(
                            "Stream done | input={} output={} reasoning={} | text_length={}",
                            input_tokens, output_tokens, reasoning_tokens, structured_response_output.len()
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
            
            debug!("========== STREAM COMPLETE ==========");
            debug!("Events received: {}", event_count);
            debug!("Text accumulated: {} bytes", structured_response_output.len());
            debug!("Tool calls: {}", tool_calls.len());
            debug!("First 200 chars: {:?}", &structured_response_output.chars().take(200).collect::<String>());
            debug!("=====================================");
            
            // Execute any pending tool calls and continue loop
            if !tool_calls.is_empty() {
                info!("Executing {} tools", tool_calls.len());
                
                tools_called.clear();
                let mut tool_results: Vec<(String, Value)> = Vec::new();
                let mut tool_outputs: Vec<Value> = Vec::new();
                
                for (_tool_id, tool_call) in tool_calls.iter() {
                    let tool_name = &tool_call.name;
                    tools_called.push(tool_name.clone());
                    
                    debug!("Executing tool: {} ({})", tool_name, tool_call.call_id);
                    
                    let arguments: Value = serde_json::from_str(&tool_call.arguments)
                        .unwrap_or_else(|e| {
                            warn!("Failed to parse tool arguments: {}", e);
                            Value::Object(Default::default())
                        });
                    
                    // Route create_artifact to DeepSeek for cheaper token usage
                    if tool_name == "create_artifact" && self.deepseek_provider.is_some() {
                        info!("Routing create_artifact to DeepSeek for cheap generation");
                        
                        // DeepSeek builds its own context from raw materials
                        match self.deepseek_provider
                            .as_ref()
                            .unwrap()
                            .generate_code_artifact(
                                &arguments,
                                &messages,
                                &context,
                                metadata.as_ref(),
                                project_id,
                                &tool_results,
                            )
                            .await
                        {
                            Ok(artifact_json) => {
                                info!("DeepSeek generation successful with full context");
                                collected_artifacts.push(artifact_json.clone());
                                
                                // Add tool output for GPT-5 continuation
                                tool_outputs.push(json!({
                                    "type": "function_call_output",
                                    "call_id": tool_call.call_id,
                                    "output": serde_json::to_string(&artifact_json)?
                                }));
                                
                                continue; // Skip normal tool execution
                            }
                            Err(e) => {
                                warn!("DeepSeek generation failed, falling back to GPT-5: {}", e);
                                // Fall through to normal execution
                            }
                        }
                    }
                    
                    // Normal tool execution for non-codegen tools or DeepSeek fallback
                    let result = self.executor.execute_tool(
                        tool_name,
                        &arguments,
                        project_id.unwrap_or(""),
                    ).await?;
                    
                    // Track result for context building in next DeepSeek call
                    tool_results.push((tool_name.clone(), result.clone()));
                    
                    // Format tool output for GPT-5 continuation
                    tool_outputs.push(json!({
                        "type": "function_call_output",
                        "call_id": tool_call.call_id,
                        "output": serde_json::to_string(&result)?
                    }));
                    
                    // Collect artifacts from create_artifact tool
                    // Handles both singular "artifact" and plural "artifacts" array formats
                    if tool_name == "create_artifact" {
                        let before_count = collected_artifacts.len();
                        
                        if let Some(artifact) = result.get("artifact") {
                            collected_artifacts.push(artifact.clone());
                            debug!("Collected artifact from 'artifact' field");
                        } else if let Some(artifacts_array) = result.get("artifacts") {
                            if let Some(arr) = artifacts_array.as_array() {
                                for artifact in arr {
                                    collected_artifacts.push(artifact.clone());
                                }
                                debug!("Collected {} artifacts from 'artifacts' array", arr.len());
                            }
                        }
                        
                        let after_count = collected_artifacts.len();
                        let new_artifacts = after_count - before_count;
                        
                        if new_artifacts == 0 {
                            warn!("create_artifact executed but NO artifacts collected - check executor return format!");
                            warn!("Result keys: {:?}", result.as_object().map(|o| o.keys().collect::<Vec<_>>()));
                        } else {
                            info!("Successfully collected {} new artifact(s), total: {}", new_artifacts, after_count);
                        }
                    }
                }
                
                // Build context for next iteration with tool outputs
                context_obj = Some(ToolContext::Gpt5 {
                    previous_response_id: response_id.clone(),
                    tool_outputs,
                });
                
                continue;
            }
            
            // No tool calls - this is the final response
            debug!("Streaming complete - returning {} bytes of JSON", structured_response_output.len());
            debug!("Final artifacts count: {}", collected_artifacts.len());
            
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

/// Accumulates tool call information as it streams in
#[derive(Debug)]
struct ToolCallBuilder {
    name: String,
    arguments: String,
    call_id: String,
}

#[derive(Debug)]
pub struct StreamingResult {
    pub content: String,
    pub artifacts: Vec<Value>,
    pub tokens: TokenUsage,
}
