// src/tools/streaming_orchestrator.rs
// Streaming chat orchestration with real-time event callbacks
// NOW OWNS PROMPT BUILDING - handler just passes raw context

use anyhow::Result;
use serde_json::Value;
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
        
        // Initialize DeepSeek provider if configured
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
        // Store references for context building
        let messages_ref = &messages;
        let context_ref = &context;
        let metadata_ref = metadata.as_ref();
        
        // Build system prompt HERE, not in handler
        let system_prompt = UnifiedPromptBuilder::build_system_prompt(
            &persona,
            &context,
            None, // tools are passed separately to LLM
            metadata.as_ref(),
            project_id,
        );
        
        debug!("System prompt built: {} chars", system_prompt.len());
        
        let mut iteration = 0;
        let max_iterations = 5;
        let mut context_obj: Option<ToolContext> = None;
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
                        });
                        
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
            
            context_obj = Some(ToolContext::Gpt5 {
                previous_response_id: response_id.clone(),
            });
            
            if !tool_calls.is_empty() {
                info!("Executing {} tools", tool_calls.len());
                
                tools_called.clear();
                let mut tool_results: Vec<(String, Value)> = Vec::new();
                
                for (tool_id, tool_call) in tool_calls.iter() {
                    let tool_name = &tool_call.name;
                    tools_called.push(tool_name.clone());
                    
                    debug!("Executing tool: {} ({})", tool_name, tool_id);
                    
                    let arguments: Value = serde_json::from_str(&tool_call.arguments)
                        .unwrap_or_else(|e| {
                            warn!("Failed to parse tool arguments: {}", e);
                            Value::Object(Default::default())
                        });
                    
                    // INTERCEPT: Route create_artifact to DeepSeek if available
                    if tool_name == "create_artifact" && self.deepseek_provider.is_some() {
                        info!("Routing create_artifact to DeepSeek for cheap generation");
                        
                        // Build rich context from conversation, memory, AND previous tool results
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
                                collected_artifacts.push(artifact_json);
                                continue; // Skip normal tool execution
                            }
                            Err(e) => {
                                warn!("DeepSeek generation failed, falling back to GPT-5: {}", e);
                                // Fall through to normal execution
                            }
                        }
                    }
                    
                    // Normal tool execution (GPT-5 or other tools)
                    let result = self.executor.execute_tool(
                        tool_name,
                        &arguments,
                        project_id.unwrap_or(""),
                    ).await?;
                    
                    // TRACK the result for context building
                    tool_results.push((tool_name.clone(), result.clone()));
                    
                    if tool_name == "create_artifact" {
                        if let Some(artifact) = result.get("artifact") {
                            collected_artifacts.push(artifact.clone());
                        }
                    }
                }
                
                continue;
            }
            
            // Final result - return accumulated JSON text
            debug!("Streaming complete - returning {} bytes of JSON", structured_response_output.len());
            
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
    /// Includes: project info, recent conversation, memory context, tool results
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
        
        // GENERATION INTENT - Make this prominent
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
        
        // Project context
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
            
            // Current file context
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
        
        // PREVIOUS TOOL RESULTS - This is the gold
        if !previous_tool_results.is_empty() {
            ctx.push_str("=== PREVIOUS TOOL RESULTS ===\n");
            for (tool_name, result) in previous_tool_results {
                ctx.push_str(&format!("Tool: {}\n", tool_name));
                
                // Format based on tool type
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
                            for (i, r) in results.iter().take(3).enumerate() {
                                if let Some(file) = r.get("file").and_then(|f| f.as_str()) {
                                    ctx.push_str(&format!("  {}. {}\n", i + 1, file));
                                    if let Some(snippet) = r.get("snippet").and_then(|s| s.as_str()) {
                                        ctx.push_str(&format!("     {}\n", snippet));
                                    }
                                }
                            }
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
        
        // Session summary
        if let Some(summary) = &context.session_summary {
            ctx.push_str("=== SESSION SUMMARY ===\n");
            ctx.push_str(&format!("{}\n\n", summary));
        }
        
        // High-salience recent memories (top 3 from recent context)
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
