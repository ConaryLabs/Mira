// src/operations/tools/mcp.rs
// MCP tool schema generation for LLM tool calling

use serde_json::Value;
use std::sync::Arc;

use crate::mcp::McpManager;

/// Get all MCP tools from connected servers in OpenAI function format
///
/// Returns tools named as `mcp__{server}__{tool}` for routing
pub async fn get_mcp_tools(manager: &McpManager) -> Vec<Value> {
    let all_tools = manager.get_all_tools().await;

    all_tools
        .into_iter()
        .map(|(server_name, tool)| tool.to_openai_format(&server_name))
        .collect()
}

/// Get MCP tools synchronously from a cached snapshot
///
/// Use this when async isn't available. Returns empty if no snapshot.
#[allow(dead_code)]
pub fn get_mcp_tools_sync(_manager: &Arc<McpManager>) -> Vec<Value> {
    // For sync contexts, we can't call async methods
    // This is a limitation - callers should prefer async version
    // Returns empty for now, async callers will get real tools
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_get_mcp_tools_empty_manager() {
        let manager = McpManager::new();
        let tools = get_mcp_tools(&manager).await;
        assert!(tools.is_empty());
    }
}
