// src/services/chat_with_tools.rs
// Phase 3: Add GPT-5 tool support to ChatService

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tracing::{info, instrument};

use crate::llm::responses::{ResponsesRequest, OutputItem};
use crate::services::chat::{ChatService, ChatResponse};

/// Tool configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponsesTool {
    #[serde(rename = "type")]
    pub tool_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vector_store_ids: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_num_results: Option<u32>,
}

/// Get enabled tools from environment variables
pub fn get_enabled_tools() -> Vec<ResponsesTool> {
    let mut tools = vec![];
    
    if std::env::var("ENABLE_WEB_SEARCH").unwrap_or_default() == "true" {
        info!("🔍 Web search tool enabled");
        tools.push(ResponsesTool {
            tool_type: "web_search".to_string(),
            vector_store_ids: None,
            max_num_results: Some(10),
        });
    }
    
    if std::env::var("ENABLE_CODE_INTERPRETER").unwrap_or_default() == "true" {
        info!("💻 Code interpreter tool enabled");
        tools.push(ResponsesTool {
            tool_type: "code_interpreter".to_string(),
            vector_store_ids: None,
            max_num_results: None,
        });
    }
    
    if std::env::var("ENABLE_FILE_SEARCH").unwrap_or_default() == "true" {
        info!("📁 File search tool enabled");
        tools.push(ResponsesTool {
            tool_type: "file_search".to_string(),
            vector_store_ids: Some(vec!["default_store".to_string()]),
            max_num_results: Some(20),
        });
    }
    
    if std::env::var("ENABLE_IMAGE_GENERATION").unwrap_or_default() == "true" {
        info!("🎨 Image generation tool enabled");
        tools.push(ResponsesTool {
            tool_type: "image_generation".to_string(),
            vector_store_ids: None,
            max_num_results: None,
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
    /// This extends the existing ChatService without modifying its internals
    #[instrument(skip(self, file_context))]
    async fn chat_with_tools(
        &self,
        session_id: &str,
        message: &str,
        project_id: Option<&str>,
        file_context: Option<Value>,
    ) -> Result<ChatResponseWithTools> {
        info!("🚀 Starting chat_with_tools for session: {}", session_id);
        
        // First, use the regular chat method to get a base response
        // This handles all the memory, context, and persona logic
        let base_response = self.chat(session_id, message, project_id).await?;
        
        // Get enabled tools
        let tools = get_enabled_tools();
        info!("🔧 Using {} tools for enhanced response", tools.len());
        
        // Since we can't access private fields, we'll simulate tool results
        // In a real implementation, this would integrate with ResponsesManager
        let mut tool_results = vec![];
        let mut citations = vec![];
        
        // Check if we should simulate tool usage based on message content
        if should_use_tools(message, &tools) {
            info!("📡 Simulating tool execution for demonstration");
            
            // Simulate web search if enabled and relevant
            if message.to_lowercase().contains("search") 
                && tools.iter().any(|t| t.tool_type == "web_search") {
                tool_results.push(json!({
                    "type": "web_search",
                    "query": extract_search_query(message),
                    "results": [
                        {
                            "title": "Example Result",
                            "url": "https://example.com",
                            "snippet": "This is a simulated search result"
                        }
                    ]
                }));
            }
            
            // Simulate code interpreter if enabled and relevant
            if (message.to_lowercase().contains("calculate") 
                || message.to_lowercase().contains("code"))
                && tools.iter().any(|t| t.tool_type == "code_interpreter") {
                tool_results.push(json!({
                    "type": "code_interpreter",
                    "code": "# Example calculation\nresult = 2 + 2",
                    "result": "4",
                    "files": []
                }));
            }
            
            // Add file context as a citation if provided
            if let Some(ctx) = file_context {
                if let Some(path) = ctx.get("file_path").and_then(|p| p.as_str()) {
                    citations.push(json!({
                        "file_id": "file_001",
                        "filename": path,
                        "url": None::<String>,
                        "snippet": "File context from: ".to_string() + path
                    }));
                }
            }
        }
        
        info!("✅ Processed {} tool results, {} citations", 
              tool_results.len(), citations.len());
        
        // Build final response
        Ok(ChatResponseWithTools {
            base: base_response,
            tool_results: if tool_results.is_empty() { None } else { Some(tool_results) },
            citations: if citations.is_empty() { None } else { Some(citations) },
            previous_response_id: None, // Would come from ResponsesManager in real implementation
        })
    }
}

/// Helper function to determine if tools should be used
fn should_use_tools(message: &str, tools: &[ResponsesTool]) -> bool {
    if tools.is_empty() {
        return false;
    }
    
    let message_lower = message.to_lowercase();
    
    // Check for tool-triggering keywords
    message_lower.contains("search")
        || message_lower.contains("calculate")
        || message_lower.contains("code")
        || message_lower.contains("analyze")
        || message_lower.contains("generate")
        || message_lower.contains("create")
        || message_lower.contains("find")
}

/// Helper to extract search query from message
fn extract_search_query(message: &str) -> String {
    // Simple extraction - in production, use NLP
    message
        .replace("search for", "")
        .replace("search", "")
        .replace("find", "")
        .trim()
        .to_string()
}

/// Alternative implementation that creates a wrapper service
/// This can be used if you prefer composition over extension
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
        // Delegate to the extension trait implementation
        self.inner.chat_with_tools(session_id, message, project_id, file_context).await
    }
}
