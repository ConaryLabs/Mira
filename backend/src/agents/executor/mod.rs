// backend/src/agents/executor/mod.rs
// Agent execution framework

pub mod builtin;
pub mod subprocess;

use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::mpsc;

use crate::agents::types::{AgentConfig, AgentDefinition, AgentResult, AgentType};

/// Events emitted during agent execution
#[derive(Debug, Clone)]
pub enum AgentEvent {
    /// Agent execution started
    Started {
        agent_execution_id: String,
        agent_name: String,
        task: String,
    },
    /// Agent is making progress
    Progress {
        agent_execution_id: String,
        agent_name: String,
        iteration: usize,
        max_iterations: usize,
        current_activity: String,
    },
    /// Agent is streaming content
    Streaming {
        agent_execution_id: String,
        content: String,
    },
    /// Tool was executed by the agent
    ToolExecuted {
        agent_execution_id: String,
        tool_name: String,
        success: bool,
        duration_ms: u64,
    },
    /// Agent completed successfully
    Completed {
        agent_execution_id: String,
        agent_name: String,
        summary: String,
        iterations_used: usize,
    },
    /// Agent failed
    Failed {
        agent_execution_id: String,
        agent_name: String,
        error: String,
    },
}

/// Trait for agent execution
#[async_trait]
pub trait AgentExecutor: Send + Sync {
    /// Execute an agent with the given configuration
    async fn execute(
        &self,
        definition: &AgentDefinition,
        config: AgentConfig,
        event_tx: Option<mpsc::Sender<AgentEvent>>,
    ) -> Result<AgentResult>;
}

/// Dispatcher that routes to appropriate executor based on agent type
pub struct AgentDispatcher {
    builtin_executor: Arc<builtin::BuiltinAgentExecutor>,
    subprocess_executor: Arc<subprocess::SubprocessAgentExecutor>,
}

impl AgentDispatcher {
    pub fn new(
        builtin_executor: Arc<builtin::BuiltinAgentExecutor>,
        subprocess_executor: Arc<subprocess::SubprocessAgentExecutor>,
    ) -> Self {
        Self {
            builtin_executor,
            subprocess_executor,
        }
    }

    /// Execute an agent
    pub async fn execute(
        &self,
        definition: &AgentDefinition,
        config: AgentConfig,
        event_tx: Option<mpsc::Sender<AgentEvent>>,
    ) -> Result<AgentResult> {
        match definition.agent_type {
            AgentType::Builtin => {
                self.builtin_executor
                    .execute(definition, config, event_tx)
                    .await
            }
            AgentType::Subprocess => {
                self.subprocess_executor
                    .execute(definition, config, event_tx)
                    .await
            }
        }
    }
}
