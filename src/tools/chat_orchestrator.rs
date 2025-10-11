// src/tools/chat_orchestrator.rs
// Chat orchestration with tool execution loop
// NOW OWNS PROMPT BUILDING - handler just passes raw context

use anyhow::Result;
use serde_json::Value;
use std::sync::Arc;
use tracing::{debug, info};

use crate::llm::provider::{Message, ToolContext, TokenUsage};
use crate::llm::provider::gpt5::Gpt5Provider;
use crate::llm::ReasoningConfig;
use crate::tools::ToolExecutor;
use crate::state::AppState;
use crate::memory::features::recall_engine::RecallContext;
use crate::persona::PersonaOverlay;
use crate::api::ws::message::MessageMetadata;
use crate::prompt::unified_builder::UnifiedPromptBuilder;

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
    
    pub async fn execute_with_tools(
        &self,
        messages: Vec<Message>,
        persona: PersonaOverlay,
        context: RecallContext,
        tools: Vec<Value>,
        metadata: Option<MessageMetadata>,
        project_id: Option<&str>,
    ) -> Result<ChatResult> {
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
            
            info!("Orchestrator call {}: reasoning={}, verbosity={}", iteration, reasoning, verbosity);
            
            let provider = self.state.llm_router.get_provider();
            let gpt5_provider = provider.as_any()
                .downcast_ref::<Gpt5Provider>()
                .ok_or_else(|| anyhow::anyhow!("Expected Gpt5Provider"))?;
            
            let raw_response = gpt5_provider.chat_with_tools_internal(
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
            });
            
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
                    
                    if tool_name == "create_artifact" {
                        if let Some(artifact) = result.get("artifact") {
                            collected_artifacts.push(artifact.clone());
                        }
                    }
                }
                
                continue;
            }
            
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
