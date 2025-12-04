// backend/src/agents/registry.rs
// Agent registry for loading and managing agents

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::Path;
use std::sync::RwLock;
use tokio::fs;
use tracing::{debug, info, warn};

use super::builtin::{create_explore_agent, create_general_agent, create_plan_agent};
use super::types::{AgentDefinition, AgentScope, AgentType, AgentsConfig, CustomAgentConfig};

/// Registry for managing agents
pub struct AgentRegistry {
    agents: RwLock<HashMap<String, AgentDefinition>>,
}

impl AgentRegistry {
    pub fn new() -> Self {
        Self {
            agents: RwLock::new(HashMap::new()),
        }
    }

    /// Load built-in agents and custom agents from config files
    pub async fn load(&self, project_root: Option<&Path>) -> Result<()> {
        let mut agents = HashMap::new();

        // 1. Register built-in agents
        let builtin_agents = vec![
            create_explore_agent(),
            create_plan_agent(),
            create_general_agent(),
        ];

        for agent in builtin_agents {
            info!("Registered built-in agent: {}", agent.id);
            agents.insert(agent.id.clone(), agent);
        }

        // 2. Load user-global agents from ~/.mira/agents.json
        if let Some(home) = dirs::home_dir() {
            let user_config = home.join(".mira").join("agents.json");
            if user_config.exists() {
                match self
                    .load_config_file(&user_config, AgentScope::User, &mut agents)
                    .await
                {
                    Ok(count) => {
                        info!("Loaded {} user agents from {:?}", count, user_config);
                    }
                    Err(e) => {
                        warn!("Failed to load user agents: {}", e);
                    }
                }
            }
        }

        // 3. Load project-specific agents from .mira/agents.json
        if let Some(root) = project_root {
            let project_config = root.join(".mira").join("agents.json");
            if project_config.exists() {
                match self
                    .load_config_file(&project_config, AgentScope::Project, &mut agents)
                    .await
                {
                    Ok(count) => {
                        info!("Loaded {} project agents from {:?}", count, project_config);
                    }
                    Err(e) => {
                        warn!("Failed to load project agents: {}", e);
                    }
                }
            }
        }

        let mut registry = self.agents.write().unwrap();
        *registry = agents;

        info!("Agent registry loaded {} agents total", registry.len());
        Ok(())
    }

    /// Load agents from a config file
    async fn load_config_file(
        &self,
        path: &Path,
        scope: AgentScope,
        agents: &mut HashMap<String, AgentDefinition>,
    ) -> Result<usize> {
        let content = fs::read_to_string(path)
            .await
            .with_context(|| format!("Failed to read agents config: {}", path.display()))?;

        let config: AgentsConfig = serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse agents config: {}", path.display()))?;

        let count = config.agents.len();
        for custom in config.agents {
            let definition = self.custom_to_definition(custom, scope.clone());
            debug!(
                "Loaded custom agent: {} ({:?})",
                definition.id, definition.scope
            );
            agents.insert(definition.id.clone(), definition);
        }

        Ok(count)
    }

    /// Convert CustomAgentConfig to AgentDefinition
    fn custom_to_definition(&self, config: CustomAgentConfig, scope: AgentScope) -> AgentDefinition {
        AgentDefinition {
            id: config.id,
            name: config.name,
            description: config.description,
            agent_type: AgentType::Subprocess,
            scope,
            tool_access: config.tool_access.into(),
            system_prompt: String::new(), // Custom agents provide their own
            command: Some(config.command),
            args: config.args,
            env: config.env,
            timeout_ms: config.timeout_ms,
            max_iterations: config.max_iterations,
            can_spawn_agents: false, // Custom agents cannot spawn sub-agents
            thinking_level: config.thinking_level,
        }
    }

    /// Check if an agent exists
    pub fn has_agent(&self, id: &str) -> bool {
        let agents = self.agents.read().unwrap();
        agents.contains_key(id)
    }

    /// Get an agent by ID
    pub fn get(&self, id: &str) -> Option<AgentDefinition> {
        let agents = self.agents.read().unwrap();
        agents.get(id).cloned()
    }

    /// List all available agents
    pub fn list(&self) -> Vec<AgentDefinition> {
        let agents = self.agents.read().unwrap();
        agents.values().cloned().collect()
    }

    /// List only built-in agents
    pub fn list_builtin(&self) -> Vec<AgentDefinition> {
        let agents = self.agents.read().unwrap();
        agents
            .values()
            .filter(|a| a.scope == AgentScope::Builtin)
            .cloned()
            .collect()
    }

    /// Get agent IDs and descriptions for LLM tool schema
    pub fn get_agent_info_for_llm(&self) -> Vec<(String, String)> {
        let agents = self.agents.read().unwrap();
        agents
            .values()
            .map(|a| (a.id.clone(), a.description.clone()))
            .collect()
    }

    /// Reload the registry
    pub async fn reload(&self, project_root: Option<&Path>) -> Result<()> {
        info!("Reloading agent registry");
        self.load(project_root).await
    }

    /// Get count of registered agents
    pub fn count(&self) -> usize {
        let agents = self.agents.read().unwrap();
        agents.len()
    }
}

impl Default for AgentRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::types::ToolAccess;
    use std::io::Write;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_load_builtin_agents() {
        let registry = AgentRegistry::new();
        registry.load(None).await.unwrap();

        assert!(registry.has_agent("explore"));
        assert!(registry.has_agent("plan"));
        assert!(registry.has_agent("general"));
        assert_eq!(registry.count(), 3);
    }

    #[tokio::test]
    async fn test_get_agent() {
        let registry = AgentRegistry::new();
        registry.load(None).await.unwrap();

        let explore = registry.get("explore").unwrap();
        assert_eq!(explore.id, "explore");
        assert_eq!(explore.scope, AgentScope::Builtin);
        assert_eq!(explore.tool_access, ToolAccess::ReadOnly);

        let general = registry.get("general").unwrap();
        assert_eq!(general.tool_access, ToolAccess::Full);
    }

    #[tokio::test]
    async fn test_load_custom_agents() {
        let temp_dir = TempDir::new().unwrap();
        let mira_dir = temp_dir.path().join(".mira");
        std::fs::create_dir_all(&mira_dir).unwrap();

        let config = r#"{
            "agents": [
                {
                    "id": "custom-test",
                    "name": "Custom Test Agent",
                    "description": "A custom test agent",
                    "command": "python",
                    "args": ["-m", "test"],
                    "tool_access": "read_only"
                }
            ]
        }"#;

        let config_path = mira_dir.join("agents.json");
        let mut file = std::fs::File::create(&config_path).unwrap();
        file.write_all(config.as_bytes()).unwrap();

        let registry = AgentRegistry::new();
        registry.load(Some(temp_dir.path())).await.unwrap();

        // Should have 3 built-in + 1 custom
        assert_eq!(registry.count(), 4);
        assert!(registry.has_agent("custom-test"));

        let custom = registry.get("custom-test").unwrap();
        assert_eq!(custom.scope, AgentScope::Project);
        assert_eq!(custom.agent_type, AgentType::Subprocess);
    }

    #[tokio::test]
    async fn test_agent_info_for_llm() {
        let registry = AgentRegistry::new();
        registry.load(None).await.unwrap();

        let info = registry.get_agent_info_for_llm();
        assert_eq!(info.len(), 3);

        // Check that we have all agent IDs
        let ids: Vec<&str> = info.iter().map(|(id, _)| id.as_str()).collect();
        assert!(ids.contains(&"explore"));
        assert!(ids.contains(&"plan"));
        assert!(ids.contains(&"general"));
    }
}
