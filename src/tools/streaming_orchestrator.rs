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
        // Store references for context building in DeepSeek codegen
        let messages_ref = &messages;
        let context_ref = &context;
        let metadata_ref = metadata.as_ref();
        
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
                        
                        // Build rich context from conversation, memory, and previous tool results
                        let codegen_context = self.build_codegen_context(
                            messages_ref,
                            context_ref,
                            metadata_ref,
                            project_id,
                            &arguments,
                            &tool_results,
                        );
                        
                        debug!("Built DeepSeek context: {} chars", codegen_context.len());
                        
                        match self.deepseek_provider
                            .as_ref()
                            .unwrap()
                            .generate_code_artifact(&arguments, Some(&codegen_context))
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
    
    /// Build rich context for DeepSeek code generation
    /// Includes: project info, recent conversation, memory context, and previous tool results
    fn build_codegen_context(
        &self,
        messages: &[Message],
        context: &RecallContext,
        metadata: Option<&MessageMetadata>,
        project_id: Option<&str>,
        tool_arguments: &Value,
        previous_tool_results: &[(String, Value)],
    ) -> String {
        let mut ctx = String::new();
        
        // Generation intent - what the user is asking for
        ctx.push_str("=== GENERATION REQUEST ===\n");
        if let Some(desc) = tool_arguments.get("description").and_then(|v| v.as_str()) {
            ctx.push_str(&format!("Task: {}\n", desc));
        }
        if let Some(path) = tool_arguments.get("path").and_then(|v| v.as_str()) {
            ctx.push_str(&format!("Target file: {}\n", path));
        }
        if let Some(lang) = tool_arguments.get("language").and_then(|v| v.as_str()) {
            ctx.push_str(&format!("Language: {}\n", lang));
        }
        ctx.push_str("\n");
        
        // Project context from metadata
        if let Some(meta) = metadata {
            ctx.push_str("=== PROJECT INFO ===\n");
            if let Some(project_name) = &meta.project_name {
                ctx.push_str(&format!("Name: {}\n", project_name));
                
                if meta.has_repository == Some(true) {
                    ctx.push_str("Type: Git repository\n");
                    if let Some(branch) = &meta.branch {
                        ctx.push_str(&format!("Branch: {}\n", branch));
                    }
                    if let Some(root) = &meta.repo_root {
                        ctx.push_str(&format!("Root: {}\n", root));
                    }
                }
            }
            
            // Current file context (if editing existing file)
            if let Some(file_path) = &meta.file_path {
                ctx.push_str(&format!("\nCurrent file: {}\n", file_path));
                if let Some(content) = &meta.file_content {
                    let preview = if content.len() > 500 {
                        format!("{}...\n(truncated, {} total chars)", &content[..500], content.len())
                    } else {
                        content.clone()
                    };
                    ctx.push_str(&format!("Content:\n```\n{}\n```\n", preview));
                }
            }
            ctx.push_str("\n");
        } else if let Some(pid) = project_id {
            ctx.push_str(&format!("=== PROJECT INFO ===\nID: {}\n\n", pid));
        }
        
        // Previous tool results - critical for multi-step codegen
        // Example: user searches for function, then asks to implement similar pattern
        if !previous_tool_results.is_empty() {
            ctx.push_str("=== PREVIOUS TOOL RESULTS ===\n");
            for (tool_name, result) in previous_tool_results {
                ctx.push_str(&format!("Tool: {}\n", tool_name));
                
                // Format based on tool type for readability
                match tool_name.as_str() {
                    "read_file" => {
                        if let Some(content) = result.get("content").and_then(|c| c.as_str()) {
                            let preview = if content.len() > 1000 {
                                format!("{}...\n(truncated, {} total chars)", &content[..1000], content.len())
                            } else {
                                content.to_string()
                            };
                            ctx.push_str(&format!("Content:\n```\n{}\n```\n", preview));
                        }
                    },
                    "search_code" => {
                        if let Some(results) = result.get("results").and_then(|r| r.as_array()) {
                            ctx.push_str(&format!("Found {} matches:\n", results.len()));
                            
                            // Show up to 5 results with full details
                            for (i, r) in results.iter().take(5).enumerate() {
                                let element_type = r.get("element_type").and_then(|t| t.as_str()).unwrap_or("unknown");
                                let name = r.get("name").and_then(|n| n.as_str()).unwrap_or("unnamed");
                                let full_path = r.get("full_path").and_then(|p| p.as_str()).unwrap_or("unknown path");
                                
                                ctx.push_str(&format!("\n  {}. {} '{}' in {}\n", i + 1, element_type, name, full_path));
                                
                                // Show line range
                                if let (Some(start), Some(end)) = (
                                    r.get("start_line").and_then(|s| s.as_i64()),
                                    r.get("end_line").and_then(|e| e.as_i64())
                                ) {
                                    ctx.push_str(&format!("     Lines {}-{}\n", start, end));
                                }
                                
                                // Show visibility and flags
                                if let Some(visibility) = r.get("visibility").and_then(|v| v.as_str()) {
                                    let mut flags = vec![visibility];
                                    if r.get("is_async").and_then(|a| a.as_bool()).unwrap_or(false) {
                                        flags.push("async");
                                    }
                                    if r.get("is_test").and_then(|t| t.as_bool()).unwrap_or(false) {
                                        flags.push("test");
                                    }
                                    ctx.push_str(&format!("     Attributes: {}\n", flags.join(", ")));
                                }
                                
                                // Show complexity for functions
                                if element_type == "function" {
                                    if let Some(complexity) = r.get("complexity_score").and_then(|c| c.as_i64()) {
                                        if complexity > 0 {
                                            ctx.push_str(&format!("     Complexity: {}\n", complexity));
                                        }
                                    }
                                }
                                
                                // Show documentation (first 150 chars)
                                if let Some(doc) = r.get("documentation").and_then(|d| d.as_str()) {
                                    let doc_preview = if doc.len() > 150 {
                                        format!("{}...", &doc[..150])
                                    } else {
                                        doc.to_string()
                                    };
                                    ctx.push_str(&format!("     Doc: {}\n", doc_preview));
                                }
                                
                                // Show code snippet (first 400 chars)
                                if let Some(content) = r.get("content").and_then(|c| c.as_str()) {
                                    let snippet = if content.len() > 400 {
                                        format!("{}...", &content[..400])
                                    } else {
                                        content.to_string()
                                    };
                                    ctx.push_str(&format!("     Code:\n```\n{}\n```\n", snippet));
                                }
                            }
                            
                            if results.len() > 5 {
                                ctx.push_str(&format!("\n... and {} more matches (showing top 5)\n", results.len() - 5));
                            }
                            ctx.push_str("\n");
                        }
                    },
                    _ => {
                        // Generic result formatting
                        ctx.push_str(&format!("{}\n", serde_json::to_string_pretty(result).unwrap_or_else(|_| "{}".to_string())));
                    }
                }
                ctx.push_str("\n");
            }
        }
        
        // Recent conversation (last 5 messages, condensed)
        if messages.len() > 1 {
            ctx.push_str("=== RECENT CONVERSATION ===\n");
            for msg in messages.iter().rev().take(5).rev() {
                let role = match msg.role.as_str() {
                    "user" => "User",
                    "assistant" => "Assistant",
                    _ => "System",
                };
                let content_preview = if msg.content.len() > 200 {
                    format!("{}...", &msg.content[..200])
                } else {
                    msg.content.clone()
                };
                ctx.push_str(&format!("{}: {}\n", role, content_preview));
            }
            ctx.push_str("\n");
        }
        
        // Session summary from memory
        if let Some(summary) = &context.session_summary {
            ctx.push_str("=== SESSION SUMMARY ===\n");
            ctx.push_str(&format!("{}\n\n", summary));
        }
        
        // High-salience recent memories (top 3)
        // Helps maintain consistency with patterns user has established
        if !context.recent.is_empty() {
            let high_salience: Vec<_> = context.recent.iter()
                .filter(|m| m.salience.unwrap_or(0.0) > 0.7)
                .take(3)
                .collect();
                
            if !high_salience.is_empty() {
                ctx.push_str("=== HIGH-SALIENCE CONTEXT ===\n");
                for mem in high_salience {
                    if let Some(summary) = &mem.summary {
                        ctx.push_str(&format!("- {}\n", summary));
                    }
                }
                ctx.push_str("\n");
            }
        }
        
        ctx
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
