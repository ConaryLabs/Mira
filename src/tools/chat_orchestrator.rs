// src/tools/chat_orchestrator.rs
// Chat orchestration with tool execution loop
// Owns prompt building - handler just passes raw context

use anyhow::Result;
use serde_json::Value;
use std::sync::Arc;
use tracing::{debug, info, warn};

use crate::llm::provider::{Message, ToolContext, TokenUsage};
use crate::llm::ReasoningConfig;
use crate::tools::ToolExecutor;
use crate::state::AppState;
use crate::memory::features::recall_engine::RecallContext;
use crate::persona::PersonaOverlay;
use crate::api::ws::message::MessageMetadata;
use crate::prompt::unified_builder::UnifiedPromptBuilder;
use crate::config::CONFIG;

pub struct ChatOrchestrator {
    state: Arc<AppState>,
    executor: ToolExecutor,
}

impl ChatOrchestrator {
    pub fn new(state: Arc<AppState>) -> Self {
        let executor = ToolExecutor::new(
            state.code_intelligence.clone(),
            state.sqlite_pool.clone(),
        );
        
        Self { state, executor }
    }
    
    /// Execute a non-streaming chat with tool support
    /// Handles multi-turn tool execution loop with automatic synthesis
    pub async fn execute_with_tools(
        &self,
        messages: Vec<Message>,
        persona: PersonaOverlay,
        context: RecallContext,
        tools: Vec<Value>,
        metadata: Option<MessageMetadata>,
        project_id: Option<&str>,
    ) -> Result<ChatResult> {
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
        
        loop {
            iteration += 1;

            // Safety valve: force final synthesis if we exceed max iterations
            if iteration > max_iterations {
                warn!("Hit max iterations ({}) - forcing final synthesis without tools", max_iterations);

                // Use provider directly - no router, no downcast
                let raw_response = self.state.gpt5_provider.chat_with_tools_internal(
                    vec![],
                    system_prompt.clone(),
                    vec![],                       // no tools - force synthesis
                    context_obj.clone(),
                    Some("high"),
                    Some("high"),
                ).await?;

                info!(
                    "GPT-5 final synthesis | input={} output={} reasoning={} latency={}ms",
                    raw_response.tokens.input,
                    raw_response.tokens.output,
                    raw_response.tokens.reasoning,
                    raw_response.latency_ms
                );

                return Ok(ChatResult {
                    content: raw_response.text_output,
                    artifacts: collected_artifacts,
                    tokens: raw_response.tokens,
                    latency_ms: raw_response.latency_ms,
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
            
            info!("Orchestrator call {}: reasoning={}, verbosity={}", iteration, reasoning, verbosity);
            
            // Use provider directly - no router, no downcast bullshit
            let raw_response = self.state.gpt5_provider.chat_with_tools_internal(
                if context_obj.is_some() { vec![] } else { messages.clone() },
                system_prompt.clone(),
                tools.clone(),
                context_obj.clone(),
                Some(reasoning),
                Some(verbosity),
            ).await?;
            
            info!(
                "GPT-5 response | input={} output={} reasoning={} latency={}ms",
                raw_response.tokens.input,
                raw_response.tokens.output,
                raw_response.tokens.reasoning,
                raw_response.latency_ms
            );
            
            context_obj = Some(ToolContext::Gpt5 {
                previous_response_id: raw_response.id.clone(),
                tool_outputs: vec![], // Non-streaming path doesn't collect tool outputs yet
            });
            
            // Execute any pending tool calls and continue loop
            if !raw_response.function_calls.is_empty() {
                info!("Executing {} tools", raw_response.function_calls.len());
                
                tools_called.clear();
                
                for func_call in &raw_response.function_calls {
                    let tool_name = &func_call.name;
                    tools_called.push(tool_name.clone());
                    
                    debug!("Executing tool: {}", tool_name);
                    
                    let result = self.executor.execute_tool(
                        tool_name,
                        &func_call.arguments,
                        project_id.unwrap_or(""),
                    ).await?;
                    
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
                
                continue;
            }
            
            // No tool calls - this is the final response
            debug!("Non-streaming complete - returning {} artifacts", collected_artifacts.len());
            
            return Ok(ChatResult {
                content: raw_response.text_output,
                artifacts: collected_artifacts,
                tokens: raw_response.tokens,
                latency_ms: raw_response.latency_ms,
            });
        }
    }
}

#[derive(Debug)]
pub struct ChatResult {
    pub content: String,
    pub artifacts: Vec<Value>,
    pub tokens: TokenUsage,
    pub latency_ms: i64,
}
