// src/services/chat_with_tools.rs
// Simplified tool management - directly returns Tool types for the Responses API
// No abstraction layer needed since we're fully committed to GPT-5's format

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tracing::info;

use crate::llm::responses::types::{Tool, FunctionDefinition, CodeInterpreterConfig, ContainerConfig};
use crate::config::CONFIG;
use crate::services::chat::{ChatService, ChatResponse};

/// Get enabled tools from configuration
/// Returns tools in the exact format expected by the Responses API
pub fn get_enabled_tools() -> Vec<Tool> {
    let mut tools = vec![];
    
    if CONFIG.enable_web_search {
        info!("🔍 Web search tool enabled");
        tools.push(Tool {
            tool_type: "web_search_preview".to_string(),
            function: None,
            web_search_preview: Some(json!({})),
            code_interpreter: None,
        });
    }
    
    if CONFIG.enable_code_interpreter {
        info!("💻 Code interpreter tool enabled");
        tools.push(Tool {
            tool_type: "code_interpreter".to_string(),
            function: None,
            web_search_preview: None,
            code_interpreter: Some(CodeInterpreterConfig {
                container: ContainerConfig {
                    container_type: "auto".to_string(),
                },
            }),
        });
    }
    
    if CONFIG.enable_file_search {
        info!("📁 File search tool enabled");
        tools.push(Tool {
            tool_type: "function".to_string(),
            function: Some(FunctionDefinition {
                name: "file_search".to_string(),
                description: "Search through uploaded files".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "Search query"
                        }
                    },
                    "required": ["query"]
                }),
            }),
            web_search_preview: None,
            code_interpreter: None,
        });
    }
    
    if CONFIG.enable_image_generation {
        info!("🎨 Image generation tool enabled");
        tools.push(Tool {
            tool_type: "function".to_string(),
            function: Some(FunctionDefinition {
                name: "image_generation".to_string(),
                description: "Generate images from text descriptions".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "prompt": {
                            "type": "string",
                            "description": "Text description of the image to generate"
                        },
                        "size": {
                            "type": "string",
                            "enum": ["1024x1024", "1024x1792", "1792x1024"],
                            "description": "Size of the image",
                            "default": "1024x1024"
                        }
                    },
                    "required": ["prompt"]
                }),
            }),
            web_search_preview: None,
            code_interpreter: None,
        });
    }
    
    info!("📦 Total tools enabled: {}", tools.len());
    tools
}

/// Extended chat response with tool results
#[derive(Debug, Serialize, Deserialize)]
pub struct ChatResponseWithTools {
    #[serde(flatten)]
    pub base: ChatResponse,
    pub tool_results: Option<Vec<Value>>,
    pub citations: Option<Vec<Value>>,
    pub previous_response_id: Option<String>,
}

/// Extension trait for ChatService to add tool support
#[async_trait::async_trait]
pub trait ChatServiceToolExt {
    async fn chat_with_tools(
        &self,
        session_id: &str,
        message: &str,
        project_id: Option<&str>,
        file_context: Option<Value>,
    ) -> Result<ChatResponseWithTools>;
}

#[async_trait::async_trait]
impl ChatServiceToolExt for ChatService {
    /// Chat with GPT-5 tool support
    /// The actual tool execution happens in the ResponsesManager during streaming
    async fn chat_with_tools(
        &self,
        session_id: &str,
        message: &str,
        project_id: Option<&str>,
        file_context: Option<Value>,
    ) -> Result<ChatResponseWithTools> {
        info!("🚀 Starting chat_with_tools for session: {}", session_id);
        
        // Use the regular chat method to get a base response
        // This handles all the memory, context, and persona logic
        let base_response = self.chat(session_id, message, project_id).await?;
        
        // Get enabled tools
        let tools = get_enabled_tools();
        info!("🔧 Using {} tools for enhanced response", tools.len());
        
        // Note: In the actual implementation, tool execution happens during
        // streaming in chat_tools.rs via the ResponsesManager.
        // This method is primarily for non-streaming contexts or testing.
        
        let mut citations = vec![];
        
        // Add file context as a citation if provided
        if let Some(ctx) = file_context {
            if let Some(path) = ctx.get("file_path").and_then(|p| p.as_str()) {
                citations.push(json!({
                    "file_id": "file_001",
                    "filename": path,
                    "url": None::<String>,
                    "snippet": format!("File context from: {}", path)
                }));
            }
        }
        
        // Build final response
        Ok(ChatResponseWithTools {
            base: base_response,
            tool_results: None, // Tool results come from ResponsesManager during streaming
            citations: if citations.is_empty() { None } else { Some(citations) },
            previous_response_id: None, // Would come from ResponsesManager
        })
    }
}

/// Alternative implementation that creates a wrapper service
pub struct ChatServiceWithTools {
    inner: ChatService,
}

impl ChatServiceWithTools {
    pub fn new(chat_service: ChatService) -> Self {
        Self {
            inner: chat_service,
        }
    }
    
    pub async fn chat_with_tools(
        &self,
        session_id: &str,
        message: &str,
        project_id: Option<&str>,
        file_context: Option<Value>,
    ) -> Result<ChatResponseWithTools> {
        self.inner.chat_with_tools(session_id, message, project_id, file_context).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_enabled_tools_count() {
        let tools = get_enabled_tools();
        // Tools returned depend on CONFIG settings
        assert!(tools.len() <= 4);
    }
}
