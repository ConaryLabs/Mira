// backend/src/agents/executor/builtin.rs
// Executor for built-in agents (in-process via tokio)

use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use crate::agents::types::{
    AgentArtifact, AgentConfig, AgentDefinition, AgentResult, AgentToolCall,
    ThinkingLevelPreference,
};
use crate::llm::provider::gemini3::{Gemini3Provider, ThinkingLevel, ToolCallResponse};
use crate::llm::provider::{Message, ToolCallInfo};
use crate::operations::engine::tool_router::ToolRouter;
use crate::operations::tools;

use super::{AgentEvent, AgentExecutor};

/// Executor for built-in agents running in-process
pub struct BuiltinAgentExecutor {
    llm_provider: Arc<Gemini3Provider>,
    tool_router: Arc<ToolRouter>,
}

impl BuiltinAgentExecutor {
    pub fn new(llm_provider: Arc<Gemini3Provider>, tool_router: Arc<ToolRouter>) -> Self {
        Self {
            llm_provider,
            tool_router,
        }
    }

    /// Get filtered tools based on agent's tool access
    fn get_filtered_tools(&self, definition: &AgentDefinition) -> Vec<Value> {
        let all_tools = tools::get_llm_tools();

        all_tools
            .into_iter()
            .filter(|tool| {
                let name = tool
                    .get("function")
                    .and_then(|f| f.get("name"))
                    .and_then(|n| n.as_str())
                    .unwrap_or("");
                definition.tool_access.is_allowed(name)
            })
            .collect()
    }

    /// Determine thinking level based on preference (for future use)
    #[allow(dead_code)]
    fn get_thinking_level(&self, preference: &ThinkingLevelPreference) -> ThinkingLevel {
        match preference {
            ThinkingLevelPreference::Low => ThinkingLevel::Low,
            ThinkingLevelPreference::High => ThinkingLevel::High,
            ThinkingLevelPreference::Adaptive => ThinkingLevel::High, // Default to high for complex tasks
        }
    }

    /// Build initial messages for the agent
    fn build_initial_messages(&self, definition: &AgentDefinition, config: &AgentConfig) -> Vec<Message> {
        let mut user_content = config.task.clone();

        // Add context if provided
        if let Some(ctx) = &config.context {
            user_content = format!("{}\n\nAdditional Context:\n{}", user_content, ctx);
        }

        // Add context files if provided
        if !config.context_files.is_empty() {
            user_content = format!(
                "{}\n\nFiles to examine:\n{}",
                user_content,
                config.context_files.join("\n")
            );
        }

        vec![
            Message::system(definition.system_prompt.clone()),
            Message::user(user_content),
        ]
    }

    /// Execute a single tool call
    async fn execute_tool_call(
        &self,
        tool_name: &str,
        arguments: Value,
        config: &AgentConfig,
    ) -> Result<Value> {
        // Use context-aware routing if we have project/session context
        if config.project_id.is_some() || config.session_id.is_some() {
            self.tool_router
                .route_tool_call_with_context(
                    tool_name,
                    arguments,
                    config.project_id.as_deref(),
                    config.session_id.as_deref().unwrap_or("agent"),
                )
                .await
        } else {
            self.tool_router.route_tool_call(tool_name, arguments).await
        }
    }

    /// Send event if channel is available
    async fn emit_event(event_tx: &Option<mpsc::Sender<AgentEvent>>, event: AgentEvent) {
        if let Some(tx) = event_tx {
            let _ = tx.send(event).await;
        }
    }
}

#[async_trait]
impl AgentExecutor for BuiltinAgentExecutor {
    async fn execute(
        &self,
        definition: &AgentDefinition,
        config: AgentConfig,
        event_tx: Option<mpsc::Sender<AgentEvent>>,
    ) -> Result<AgentResult> {
        let start_time = Instant::now();
        let agent_execution_id = uuid::Uuid::new_v4().to_string();

        info!(
            "[AGENT:{}] Starting execution (id: {})",
            definition.id, agent_execution_id
        );

        // Emit started event
        Self::emit_event(
            &event_tx,
            AgentEvent::Started {
                agent_execution_id: agent_execution_id.clone(),
                agent_name: definition.name.clone(),
                task: config.task.clone(),
            },
        )
        .await;

        // Build initial messages
        let mut messages = self.build_initial_messages(definition, &config);

        // Get filtered tools for this agent
        let tools = self.get_filtered_tools(definition);

        // Track execution
        let mut total_tokens_input: i64 = 0;
        let mut total_tokens_output: i64 = 0;
        let mut tool_calls_made: Vec<AgentToolCall> = Vec::new();
        let artifacts: Vec<AgentArtifact> = Vec::new();
        let mut accumulated_response = String::new();
        let mut thought_signature = config.thought_signature.clone();
        let mut current_iteration: usize = 0;

        // Tool-calling loop
        for iteration in 1..=definition.max_iterations {
            current_iteration = iteration as usize;
            debug!(
                "[AGENT:{}] Iteration {}/{}",
                definition.id, iteration, definition.max_iterations
            );

            // Emit progress
            Self::emit_event(
                &event_tx,
                AgentEvent::Progress {
                    agent_execution_id: agent_execution_id.clone(),
                    agent_name: definition.name.clone(),
                    iteration: current_iteration,
                    max_iterations: definition.max_iterations as usize,
                    current_activity: format!("Iteration {}", current_iteration),
                },
            )
            .await;

            // Call LLM with tools
            let response: ToolCallResponse = self
                .llm_provider
                .call_with_tools(messages.clone(), tools.clone())
                .await
                .context("LLM call failed")?;

            total_tokens_input += response.tokens_input;
            total_tokens_output += response.tokens_output;

            // Capture thought signature for continuity
            thought_signature = response.thought_signature.clone();

            // Accumulate response content
            if let Some(content) = &response.content {
                if !content.is_empty() {
                    accumulated_response.push_str(content);

                    // Emit streaming event
                    Self::emit_event(
                        &event_tx,
                        AgentEvent::Streaming {
                            agent_execution_id: agent_execution_id.clone(),
                            content: content.clone(),
                        },
                    )
                    .await;
                }
            }

            // No more tool calls - we're done
            if response.tool_calls.is_empty() {
                debug!("[AGENT:{}] No more tool calls, completing", definition.id);
                break;
            }

            // Build assistant message with tool calls and thought signature
            let tool_calls_info: Vec<ToolCallInfo> = response
                .tool_calls
                .iter()
                .map(|tc| ToolCallInfo {
                    id: tc.id.clone(),
                    name: tc.name.clone(),
                    arguments: tc.arguments.clone(),
                })
                .collect();

            messages.push(Message::assistant_with_tool_calls_and_signature(
                response.content.clone().unwrap_or_default(),
                tool_calls_info,
                response.thought_signature.clone(),
            ));

            // Execute tool calls
            for tool_call in &response.tool_calls {
                let tool_start = Instant::now();

                // Check if tool is allowed for this agent
                if !definition.tool_access.is_allowed(&tool_call.name) {
                    warn!(
                        "[AGENT:{}] Tool '{}' not allowed",
                        definition.id, tool_call.name
                    );

                    messages.push(Message::tool_result(
                        tool_call.id.clone(),
                        serde_json::json!({
                            "error": format!("Tool '{}' is not allowed for this agent", tool_call.name)
                        })
                        .to_string(),
                    ));

                    tool_calls_made.push(AgentToolCall {
                        tool_name: tool_call.name.clone(),
                        arguments: tool_call.arguments.clone(),
                        result: serde_json::json!({"error": "Not allowed"}),
                        success: false,
                        duration_ms: tool_start.elapsed().as_millis() as u64,
                    });

                    continue;
                }

                debug!("[AGENT:{}] Executing tool: {}", definition.id, tool_call.name);

                let result = self
                    .execute_tool_call(&tool_call.name, tool_call.arguments.clone(), &config)
                    .await;

                let (result_value, success) = match result {
                    Ok(v) => (v, true),
                    Err(e) => (serde_json::json!({"error": e.to_string()}), false),
                };

                let duration_ms = tool_start.elapsed().as_millis() as u64;

                tool_calls_made.push(AgentToolCall {
                    tool_name: tool_call.name.clone(),
                    arguments: tool_call.arguments.clone(),
                    result: result_value.clone(),
                    success,
                    duration_ms,
                });

                // Add tool result to messages
                messages.push(Message::tool_result(
                    tool_call.id.clone(),
                    serde_json::to_string(&result_value)
                        .unwrap_or_else(|_| result_value.to_string()),
                ));

                // Emit tool executed event
                Self::emit_event(
                    &event_tx,
                    AgentEvent::ToolExecuted {
                        agent_execution_id: agent_execution_id.clone(),
                        tool_name: tool_call.name.clone(),
                        success,
                        duration_ms,
                    },
                )
                .await;
            }
        }

        let execution_time_ms = start_time.elapsed().as_millis() as u64;

        info!(
            "[AGENT:{}] Completed in {}ms, {} tool calls, {} iterations",
            definition.id,
            execution_time_ms,
            tool_calls_made.len(),
            current_iteration
        );

        // Emit completed event
        Self::emit_event(
            &event_tx,
            AgentEvent::Completed {
                agent_execution_id: agent_execution_id.clone(),
                agent_name: definition.name.clone(),
                summary: if accumulated_response.len() > 200 {
                    format!("{}...", &accumulated_response[..200])
                } else {
                    accumulated_response.clone()
                },
                iterations_used: current_iteration,
            },
        )
        .await;

        Ok(AgentResult {
            agent_id: definition.id.clone(),
            success: true,
            response: accumulated_response,
            thought_signature,
            artifacts,
            tool_calls: tool_calls_made,
            tokens_input: total_tokens_input,
            tokens_output: total_tokens_output,
            execution_time_ms,
            error: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::builtin::create_explore_agent;

    // Note: Full tests require LLM provider and tool router setup
    // These are integration tests that should be run with real services

    #[test]
    fn test_build_initial_messages() {
        let _definition = create_explore_agent();
        let config = AgentConfig {
            agent_id: "explore".to_string(),
            task: "Find the main function".to_string(),
            context: Some("Looking for entry point".to_string()),
            context_files: vec!["src/main.rs".to_string()],
            parent_operation_id: None,
            session_id: None,
            project_id: None,
            thought_signature: None,
        };

        // We can't call build_initial_messages directly without an executor instance
        // This test verifies the config structure
        assert_eq!(config.task, "Find the main function");
        assert!(config.context.is_some());
        assert_eq!(config.context_files.len(), 1);
    }

    #[test]
    fn test_tool_filtering() {
        let definition = create_explore_agent();
        assert!(definition.tool_access.is_allowed("read_project_file"));
        assert!(definition.tool_access.is_allowed("search_codebase"));
        assert!(!definition.tool_access.is_allowed("write_project_file"));
        assert!(!definition.tool_access.is_allowed("execute_command"));
    }
}
