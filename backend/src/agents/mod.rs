// backend/src/agents/mod.rs
// Agent system - Claude Code-style specialized agents for Mira
//
// This module provides:
// - Built-in agents: explore (read-only), plan (research), general (full)
// - Custom agents: loaded from ~/.mira/agents.json and .mira/agents.json
// - Hybrid execution: built-in run in-process, custom run as subprocesses
// - Tool filtering: agents can only use their allowed tools
// - Thought signature support: Gemini 3 reasoning continuity

pub mod builtin;
pub mod executor;
pub mod protocol;
pub mod registry;
pub mod tool_schema;
pub mod types;

use anyhow::Result;
use serde_json::Value;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::info;

use crate::llm::provider::LlmProvider;
use crate::operations::engine::tool_router::ToolRouter;

pub use executor::{AgentDispatcher, AgentEvent};
pub use registry::AgentRegistry;
pub use types::{AgentConfig, AgentDefinition, AgentResult, AgentType, ToolAccess};

/// Main agent manager - coordinates registry and execution
pub struct AgentManager {
    registry: Arc<AgentRegistry>,
    dispatcher: AgentDispatcher,
}

impl AgentManager {
    /// Create a new agent manager
    pub fn new(llm_provider: Arc<dyn LlmProvider>, tool_router: Arc<ToolRouter>) -> Self {
        let builtin_executor = Arc::new(executor::builtin::BuiltinAgentExecutor::new(
            Arc::clone(&llm_provider),
            Arc::clone(&tool_router),
        ));
        let subprocess_executor =
            Arc::new(executor::subprocess::SubprocessAgentExecutor::new(Arc::clone(
                &tool_router,
            )));

        Self {
            registry: Arc::new(AgentRegistry::new()),
            dispatcher: AgentDispatcher::new(builtin_executor, subprocess_executor),
        }
    }

    /// Load agents from config files
    pub async fn load(&self, project_root: Option<&Path>) -> Result<()> {
        self.registry.load(project_root).await
    }

    /// Get an agent definition by ID
    pub fn get_agent(&self, id: &str) -> Option<AgentDefinition> {
        self.registry.get(id)
    }

    /// Check if an agent exists
    pub fn has_agent(&self, id: &str) -> bool {
        self.registry.has_agent(id)
    }

    /// List all available agents
    pub fn list_agents(&self) -> Vec<AgentDefinition> {
        self.registry.list()
    }

    /// List only built-in agents
    pub fn list_builtin_agents(&self) -> Vec<AgentDefinition> {
        self.registry.list_builtin()
    }

    /// Execute an agent
    ///
    /// # Arguments
    /// * `agent_id` - ID of the agent to execute
    /// * `config` - Agent configuration including task and context
    /// * `event_tx` - Optional channel for receiving agent events
    ///
    /// # Returns
    /// Result containing the agent's final response and metadata
    pub async fn execute(
        &self,
        agent_id: &str,
        config: AgentConfig,
        event_tx: Option<mpsc::Sender<AgentEvent>>,
    ) -> Result<AgentResult> {
        let definition = self
            .registry
            .get(agent_id)
            .ok_or_else(|| anyhow::anyhow!("Agent not found: {}", agent_id))?;

        info!(
            "[AGENT_MANAGER] Executing agent '{}' for task: {}",
            agent_id,
            if config.task.len() > 100 {
                format!("{}...", &config.task[..100])
            } else {
                config.task.clone()
            }
        );

        self.dispatcher.execute(&definition, config, event_tx).await
    }

    /// Execute multiple agents in parallel
    ///
    /// Returns a vector of results, one per agent (in same order as input)
    pub async fn execute_parallel(
        &self,
        configs: Vec<(String, AgentConfig)>,
        event_tx: Option<mpsc::Sender<AgentEvent>>,
    ) -> Vec<Result<AgentResult>> {
        use futures::future::join_all;

        let futures: Vec<_> = configs
            .into_iter()
            .map(|(agent_id, config)| {
                let event_tx = event_tx.clone();
                async move { self.execute(&agent_id, config, event_tx).await }
            })
            .collect();

        join_all(futures).await
    }

    /// Get the spawn_agent tool schema with current agent list
    pub fn get_spawn_agent_tool(&self) -> Value {
        let agents = self.registry.get_agent_info_for_llm();
        tool_schema::build_spawn_agent_tool(&agents)
    }

    /// Get the spawn_agents_parallel tool schema
    pub fn get_spawn_agents_parallel_tool(&self) -> Value {
        let agents = self.registry.get_agent_info_for_llm();
        tool_schema::build_spawn_agents_parallel_tool(&agents)
    }

    /// Get both agent tool schemas
    pub fn get_agent_tools(&self) -> Vec<Value> {
        vec![
            self.get_spawn_agent_tool(),
            self.get_spawn_agents_parallel_tool(),
        ]
    }

    /// Reload the registry
    pub async fn reload(&self, project_root: Option<&Path>) -> Result<()> {
        self.registry.reload(project_root).await
    }

    /// Get count of registered agents
    pub fn agent_count(&self) -> usize {
        self.registry.count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: Full tests require LLM provider and tool router setup
    // These are basic structural tests

    #[test]
    fn test_agent_manager_structure() {
        // Verify the module structure compiles correctly
        // Full integration tests need actual services
    }

    #[tokio::test]
    async fn test_registry_loads_builtin() {
        let registry = AgentRegistry::new();
        registry.load(None).await.unwrap();

        assert!(registry.has_agent("explore"));
        assert!(registry.has_agent("plan"));
        assert!(registry.has_agent("general"));
        assert_eq!(registry.count(), 3);
    }
}
