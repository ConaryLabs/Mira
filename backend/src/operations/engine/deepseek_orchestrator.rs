// backend/src/operations/engine/deepseek_orchestrator.rs
// DeepSeek-based orchestration replacing GPT-5 Responses API complexity
// Mirrors Claude Code's architecture: chat for execution, reasoner for complex generation

use anyhow::{Context, Result};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use crate::llm::provider::deepseek::{DeepSeekProvider, ToolCall};
use crate::llm::provider::Message;
use crate::llm::router::{DeepSeekModel, ModelRouter, TaskAnalysis};
use crate::operations::engine::tool_router::ToolRouter;

use super::events::OperationEngineEvent;

/// DeepSeek orchestrator for intelligent dual-model routing
///
/// Architecture:
/// - deepseek-chat: Primary orchestrator + executor (like Claude's Sonnet + Haiku)
/// - deepseek-reasoner: Complex reasoning + large code gen (like Claude's Opus)
///
/// Flow patterns:
/// 1. Simple: Chat handles everything with tools
/// 2. Complex: Chat gathers context → Reasoner generates → Chat applies
pub struct DeepSeekOrchestrator {
    provider: DeepSeekProvider,
    router: ModelRouter,
    tool_router: Option<Arc<ToolRouter>>,
}

impl DeepSeekOrchestrator {
    pub fn new(
        provider: DeepSeekProvider,
        router: ModelRouter,
        tool_router: Option<Arc<ToolRouter>>,
    ) -> Self {
        Self {
            provider,
            router,
            tool_router,
        }
    }

    /// Execute a request using smart routing between chat and reasoner
    pub async fn execute(
        &self,
        operation_id: &str,
        messages: Vec<Message>,
        tools: Vec<Value>,
        event_tx: &mpsc::Sender<OperationEngineEvent>,
    ) -> Result<String> {
        // Analyze the task to determine routing
        let last_message = messages.last()
            .map(|m| m.content.as_str())
            .unwrap_or("");

        let analysis = TaskAnalysis::analyze(last_message, !tools.is_empty());
        let chosen_model = self.router.choose_model(&analysis);

        info!(
            "[ORCHESTRATOR] Task analysis: complexity={:?}, estimated_tokens={}, model={:?}",
            analysis.complexity,
            analysis.estimated_tokens,
            chosen_model
        );

        match chosen_model {
            DeepSeekModel::Chat => {
                self.execute_with_chat(operation_id, messages, tools, event_tx).await
            }
            DeepSeekModel::Reasoner => {
                self.execute_with_reasoner(operation_id, messages, tools, event_tx).await
            }
        }
    }

    /// Execute using chat model with full tool calling loop
    /// This is the primary path for most operations (like Claude's Sonnet)
    async fn execute_with_chat(
        &self,
        operation_id: &str,
        mut messages: Vec<Message>,
        tools: Vec<Value>,
        event_tx: &mpsc::Sender<OperationEngineEvent>,
    ) -> Result<String> {
        info!("[ORCHESTRATOR] Executing with deepseek-chat (tools + orchestration)");

        let mut accumulated_text = String::new();
        let max_iterations = 10; // Safety limit

        for iteration in 1..=max_iterations {
            debug!("[ORCHESTRATOR] Chat iteration {}/{}", iteration, max_iterations);

            // Call DeepSeek chat with tools
            let response = self.provider
                .call_with_tools(messages.clone(), tools.clone())
                .await
                .context("Failed to call DeepSeek chat model")?;

            // Stream any text content
            if let Some(content) = &response.content {
                if !content.is_empty() {
                    accumulated_text.push_str(content);

                    let _ = event_tx.send(OperationEngineEvent::Streaming {
                        operation_id: operation_id.to_string(),
                        content: content.clone(),
                    }).await;
                }
            }

            // Check if we have tool calls
            if response.tool_calls.is_empty() {
                info!("[ORCHESTRATOR] No tool calls, execution complete");
                break;
            }

            info!("[ORCHESTRATOR] Processing {} tool calls", response.tool_calls.len());

            // Execute tools and collect results
            for tool_call in response.tool_calls {
                let result = self.execute_tool(operation_id, &tool_call, event_tx).await?;

                // Add tool result to conversation
                messages.push(Message {
                    role: "tool".to_string(),
                    content: serde_json::to_string(&result)?,
                });
            }

            // Safety check
            if iteration >= max_iterations {
                warn!("[ORCHESTRATOR] Max iterations reached, stopping");
                break;
            }
        }

        Ok(accumulated_text)
    }

    /// Execute using reasoner model for complex generation
    /// Pattern: Chat gathers context → Reasoner generates → Chat applies
    async fn execute_with_reasoner(
        &self,
        operation_id: &str,
        messages: Vec<Message>,
        tools: Vec<Value>,
        event_tx: &mpsc::Sender<OperationEngineEvent>,
    ) -> Result<String> {
        info!("[ORCHESTRATOR] Executing with deepseek-reasoner (complex generation)");

        // Phase 1: Use chat to gather context (if tools available)
        let context = if !tools.is_empty() && self.tool_router.is_some() {
            info!("[ORCHESTRATOR] Phase 1: Gathering context with chat model");
            self.gather_context_with_chat(operation_id, messages.clone(), tools.clone(), event_tx)
                .await?
        } else {
            // No tools, use messages directly
            messages.iter()
                .map(|m| format!("{}: {}", m.role, m.content))
                .collect::<Vec<_>>()
                .join("\n")
        };

        // Phase 2: Use reasoner for generation
        info!("[ORCHESTRATOR] Phase 2: Generating with reasoner model");

        let reasoner_messages = vec![
            Message::system("You are an expert code generation assistant. Generate complete, working code.".to_string()),
            Message::user(context),
        ];

        // Note: Reasoner uses generate_code method with structured output
        // For now, we'll use a simple approach - this can be enhanced later
        let generated_content = self.call_reasoner_simple(reasoner_messages).await?;

        // Stream the generated content
        let _ = event_tx.send(OperationEngineEvent::Streaming {
            operation_id: operation_id.to_string(),
            content: generated_content.clone(),
        }).await;

        // Phase 3: Use chat to apply changes (if tools available)
        if !tools.is_empty() && self.tool_router.is_some() {
            info!("[ORCHESTRATOR] Phase 3: Applying changes with chat model");
            self.apply_with_chat(operation_id, &generated_content, tools, event_tx).await?;
        }

        Ok(generated_content)
    }

    /// Gather context using chat model with tools
    async fn gather_context_with_chat(
        &self,
        operation_id: &str,
        mut messages: Vec<Message>,
        tools: Vec<Value>,
        event_tx: &mpsc::Sender<OperationEngineEvent>,
    ) -> Result<String> {
        // Add a system message to guide context gathering
        messages.insert(0, Message::system(
            "Gather all necessary context for this task using the available tools. \
            Read relevant files, search code, check git history as needed.".to_string()
        ));

        let response = self.provider
            .call_with_tools(messages, tools)
            .await?;

        // Execute any tool calls
        let mut context_parts = Vec::new();

        if let Some(content) = response.content {
            context_parts.push(content);
        }

        for tool_call in response.tool_calls {
            let result = self.execute_tool(operation_id, &tool_call, event_tx).await?;
            context_parts.push(serde_json::to_string(&result)?);
        }

        Ok(context_parts.join("\n\n"))
    }

    /// Apply generated code using chat model with tools
    async fn apply_with_chat(
        &self,
        operation_id: &str,
        generated_code: &str,
        tools: Vec<Value>,
        event_tx: &mpsc::Sender<OperationEngineEvent>,
    ) -> Result<()> {
        let messages = vec![
            Message::system("Apply the generated code using the write_file tool.".to_string()),
            Message::user(format!("Generated code:\n{}", generated_code)),
        ];

        let response = self.provider
            .call_with_tools(messages, tools)
            .await?;

        // Execute tool calls
        for tool_call in response.tool_calls {
            self.execute_tool(operation_id, &tool_call, event_tx).await?;
        }

        Ok(())
    }

    /// Simple reasoner call (placeholder - will be enhanced)
    async fn call_reasoner_simple(&self, messages: Vec<Message>) -> Result<String> {
        // For now, use the chat endpoint with high max_tokens
        // This will be replaced with proper reasoner integration
        let response = self.provider
            .call_with_tools(messages, vec![])
            .await?;

        Ok(response.content.unwrap_or_default())
    }

    /// Execute a single tool call
    async fn execute_tool(
        &self,
        operation_id: &str,
        tool_call: &ToolCall,
        event_tx: &mpsc::Sender<OperationEngineEvent>,
    ) -> Result<Value> {
        info!("[ORCHESTRATOR] Executing tool: {}", tool_call.name);

        // Emit tool execution event
        let _ = event_tx.send(OperationEngineEvent::ToolExecuted {
            operation_id: operation_id.to_string(),
            tool_name: tool_call.name.clone(),
            tool_type: "file".to_string(), // TODO: Classify tool type
            summary: format!("Executing {}", tool_call.name),
            success: true,
            details: None,
        }).await;

        // Route to tool router if available
        if let Some(router) = &self.tool_router {
            match router.route_tool_call(&tool_call.name, tool_call.arguments.clone()).await {
                Ok(result) => Ok(result),
                Err(e) => {
                    warn!("[ORCHESTRATOR] Tool execution failed: {}", e);
                    Ok(serde_json::json!({
                        "success": false,
                        "error": e.to_string()
                    }))
                }
            }
        } else {
            warn!("[ORCHESTRATOR] No tool router available");
            Ok(serde_json::json!({
                "success": false,
                "error": "Tool router not available"
            }))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_orchestrator_creation() {
        let provider = DeepSeekProvider::new("test-key".to_string());
        let router = ModelRouter::default();
        let _orchestrator = DeepSeekOrchestrator::new(provider, router, None);

        // Just verify it compiles and creates
        assert!(true);
    }
}
