// src/synthesis/loader.rs
// Dynamic tool loading and registration

use anyhow::{Context, Result};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use super::storage::SynthesisStorage;
use super::types::*;

/// Trait for dynamically loaded tools
#[async_trait]
pub trait Tool: Send + Sync {
    /// Get the tool name
    fn name(&self) -> &str;

    /// Get the OpenAI-compatible tool definition
    fn definition(&self) -> ToolDefinition;

    /// Execute the tool with given arguments
    async fn execute(&self, args: ToolArgs) -> Result<ToolResult>;
}

/// Dynamic tool loader manages synthesized tools at runtime
pub struct DynamicToolLoader {
    /// Loaded tools by name
    tools: RwLock<HashMap<String, Arc<dyn Tool>>>,
    /// Storage for persistence
    storage: Arc<SynthesisStorage>,
}

impl DynamicToolLoader {
    pub fn new(storage: Arc<SynthesisStorage>) -> Self {
        Self {
            tools: RwLock::new(HashMap::new()),
            storage,
        }
    }

    /// Register a tool
    pub async fn register(&self, tool: Arc<dyn Tool>) -> Result<()> {
        let name = tool.name().to_string();
        let mut tools = self.tools.write().await;

        if tools.contains_key(&name) {
            warn!("Tool {} already registered, replacing", name);
        }

        tools.insert(name.clone(), tool);
        info!("Registered tool: {}", name);
        Ok(())
    }

    /// Unregister a tool by name
    pub async fn unregister(&self, name: &str) -> Result<()> {
        let mut tools = self.tools.write().await;

        if tools.remove(name).is_some() {
            info!("Unregistered tool: {}", name);
            Ok(())
        } else {
            Err(anyhow::anyhow!("Tool not found: {}", name))
        }
    }

    /// Get a tool by name
    pub async fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        let tools = self.tools.read().await;
        tools.get(name).cloned()
    }

    /// Check if a tool is loaded
    pub async fn is_loaded(&self, name: &str) -> bool {
        let tools = self.tools.read().await;
        tools.contains_key(name)
    }

    /// Get all tool definitions for LLM
    pub async fn get_definitions(&self) -> Vec<ToolDefinition> {
        let tools = self.tools.read().await;
        tools.values().map(|t| t.definition()).collect()
    }

    /// Get all loaded tool names
    pub async fn list_tools(&self) -> Vec<String> {
        let tools = self.tools.read().await;
        tools.keys().cloned().collect()
    }

    /// Execute a tool by name
    pub async fn execute(&self, name: &str, args: ToolArgs) -> Result<ToolResult> {
        let tools = self.tools.read().await;

        let tool = tools
            .get(name)
            .ok_or_else(|| anyhow::anyhow!("Tool not found: {}", name))?;

        let start = std::time::Instant::now();
        let result = tool.execute(args).await;
        let duration = start.elapsed();

        debug!(
            "Tool {} executed in {:?}: {:?}",
            name,
            duration,
            result.is_ok()
        );

        result
    }

    /// Load all enabled tools from storage for a project
    pub async fn load_project_tools(&self, project_id: &str) -> Result<usize> {
        let synthesized_tools = self
            .storage
            .list_tools(project_id, true)
            .await
            .context("Failed to list tools")?;

        let mut loaded = 0;

        for tool_data in synthesized_tools {
            // Create tool wrapper from stored data
            let wrapper = StoredToolWrapper::new(tool_data);

            if let Err(e) = self.register(Arc::new(wrapper)).await {
                warn!("Failed to load tool: {}", e);
            } else {
                loaded += 1;
            }
        }

        info!("Loaded {} tools for project {}", loaded, project_id);
        Ok(loaded)
    }

    /// Unload all tools
    pub async fn unload_all(&self) {
        let mut tools = self.tools.write().await;
        let count = tools.len();
        tools.clear();
        info!("Unloaded {} tools", count);
    }

    /// Reload a specific tool
    pub async fn reload(&self, name: &str) -> Result<()> {
        // Get current tool data from storage
        let tool_data = self
            .storage
            .get_tool(name)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Tool not found in storage: {}", name))?;

        // Unregister old version
        let _ = self.unregister(name).await;

        // Register new version
        let wrapper = StoredToolWrapper::new(tool_data);
        self.register(Arc::new(wrapper)).await?;

        info!("Reloaded tool: {}", name);
        Ok(())
    }
}

/// Wrapper for stored tool data that implements the Tool trait
struct StoredToolWrapper {
    data: SynthesizedTool,
}

impl StoredToolWrapper {
    fn new(data: SynthesizedTool) -> Self {
        Self { data }
    }
}

#[async_trait]
impl Tool for StoredToolWrapper {
    fn name(&self) -> &str {
        &self.data.name
    }

    fn definition(&self) -> ToolDefinition {
        // Parse tool definition from source code or generate default
        ToolDefinition {
            tool_type: "function".to_string(),
            function: FunctionDefinition {
                name: self.data.name.clone(),
                description: self.data.description.clone(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {},
                    "required": []
                }),
            },
        }
    }

    async fn execute(&self, _args: ToolArgs) -> Result<ToolResult> {
        // For now, return a placeholder
        // In a full implementation, this would:
        // 1. Load and execute the compiled binary
        // 2. Or interpret the source code dynamically
        Ok(ToolResult::failure(format!(
            "Tool {} is not yet compiled for execution",
            self.data.name
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    struct MockTool {
        name: String,
    }

    #[async_trait]
    impl Tool for MockTool {
        fn name(&self) -> &str {
            &self.name
        }

        fn definition(&self) -> ToolDefinition {
            ToolDefinition {
                tool_type: "function".to_string(),
                function: FunctionDefinition {
                    name: self.name.clone(),
                    description: "Mock tool".to_string(),
                    parameters: serde_json::json!({"type": "object", "properties": {}}),
                },
            }
        }

        async fn execute(&self, _args: ToolArgs) -> Result<ToolResult> {
            Ok(ToolResult::success("Mock result".to_string()))
        }
    }

    #[tokio::test]
    async fn test_tool_registration() {
        let pool = SqlitePoolOptions::new()
            .connect(":memory:")
            .await
            .unwrap();

        let storage = Arc::new(SynthesisStorage::new(Arc::new(pool)));
        let loader = DynamicToolLoader::new(storage);

        let tool = Arc::new(MockTool {
            name: "test_tool".to_string(),
        });

        loader.register(tool).await.unwrap();

        assert!(loader.is_loaded("test_tool").await);
        assert!(!loader.is_loaded("other_tool").await);

        let names = loader.list_tools().await;
        assert_eq!(names.len(), 1);
        assert_eq!(names[0], "test_tool");
    }
}
