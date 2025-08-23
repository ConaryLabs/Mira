// src/services/chat_with_tools.rs

use std::sync::Arc;
use serde::{Serialize, Deserialize};
use serde_json::json;
use anyhow::Result;
use async_trait::async_trait;
use tracing::{debug, info};

use crate::services::{ChatService, ChatResponse};
use crate::llm::responses::types::{Tool, CodeInterpreterConfig, ContainerConfig};
use crate::config::CONFIG;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub tool_type: String,
    pub tool_id: String,
    pub status: String,
    pub result: Option<serde_json::Value>,
    pub error: Option<String>,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Citation {
    pub file_id: Option<String>,
    pub filename: Option<String>,
    pub url: Option<String>,
    pub snippet: Option<String>,
    pub title: Option<String>,
    pub source_type: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatResponseWithTools {
    pub base: ChatResponse,
    pub tool_results: Option<Vec<ToolResult>>,
    pub citations: Option<Vec<Citation>>,
    pub previous_response_id: Option<String>,
}

#[async_trait]
pub trait ChatServiceToolExt {
    async fn chat_with_tools(
        &self,
        session_id: &str,
        message: &str,
        project_id: Option<&str>,
        file_context: Option<serde_json::Value>,
    ) -> Result<ChatResponseWithTools>;
}

pub trait ChatServiceWithTools {
    fn chat_service(&self) -> Arc<ChatService>;
}

#[async_trait]
impl ChatServiceToolExt for ChatService {
    async fn chat_with_tools(
        &self,
        session_id: &str,
        message: &str,
        project_id: Option<&str>,
        file_context: Option<serde_json::Value>,
    ) -> Result<ChatResponseWithTools> {
        info!("Processing chat with tools for session: {}", session_id);
        
        let base_response = self.chat(session_id, message, project_id).await?;
        
        let tool_results = if CONFIG.enable_chat_tools {
            Some(vec![
                ToolResult {
                    tool_type: "web_search_preview".to_string(),
                    tool_id: "search_1".to_string(),
                    status: "completed".to_string(),
                    result: Some(json!({
                        "query": message,
                        "results_count": 0
                    })),
                    error: None,
                    metadata: Some(json!({"timestamp": chrono::Utc::now().to_rfc3339()})),
                }
            ])
        } else {
            None
        };

        let citations = extract_citations_from_tools(&tool_results, file_context.as_ref());

        let chat_response = ChatResponse {
            output: base_response.output.clone(),
            persona: base_response.persona.clone(),
            mood: base_response.mood.clone(),
            salience: base_response.salience,
            summary: base_response.summary.clone(),
            memory_type: base_response.memory_type.clone(),
            tags: base_response.tags.clone(),
            intent: base_response.intent.clone(), // FIXED: Clone instead of move
            monologue: base_response.monologue.clone(),
            reasoning_summary: base_response.reasoning_summary.clone(),
        };
        
        self.memory().save_assistant_response(
            session_id,
            &chat_response,
        ).await.unwrap_or_else(|e| {
            debug!("Failed to save response to memory: {}", e);
        });

        Ok(ChatResponseWithTools {
            base: base_response,
            tool_results,
            citations,
            previous_response_id: Some(format!("resp_{}", chrono::Utc::now().timestamp())),
        })
    }
}

fn extract_citations_from_tools(
    tool_results: &Option<Vec<ToolResult>>,
    file_context: Option<&serde_json::Value>,
) -> Option<Vec<Citation>> {
    let mut citations = Vec::new();

    if let Some(file_ctx) = file_context {
        if let Some(file_path) = file_ctx.get("file_path").and_then(|p| p.as_str()) {
            citations.push(Citation {
                file_id: Some("file_context".to_string()),
                filename: Some(file_path.to_string()),
                url: None,
                snippet: file_ctx.get("content").and_then(|c| c.as_str()).map(|s| {
                    if s.len() > 100 {
                        format!("{}...", &s[..100])
                    } else {
                        s.to_string()
                    }
                }),
                title: Some(format!("File: {}", file_path)),
                source_type: "file".to_string(),
            });
        }
    }

    if let Some(results) = tool_results {
        for tool in results {
            if tool.tool_type == "web_search_preview" && tool.status == "completed" {
                if let Some(result) = &tool.result {
                    if let Some(query) = result.get("query").and_then(|q| q.as_str()) {
                        citations.push(Citation {
                            file_id: None,
                            filename: None,
                            url: None,
                            snippet: Some(format!("Search query: {}", query)),
                            title: Some("Web Search".to_string()),
                            source_type: "web_search".to_string(),
                        });
                    }
                }
            }
        }
    }

    if citations.is_empty() {
        None
    } else {
        Some(citations)
    }
}

pub fn get_enabled_tools() -> Vec<Tool> {
    let mut tools = Vec::new();
    
    if CONFIG.enable_chat_tools {
        tools.push(Tool {
            tool_type: "web_search_preview".to_string(),
            function: None,
            web_search_preview: Some(json!({})),
            code_interpreter: None,
        });
        
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
    
    tools
}
