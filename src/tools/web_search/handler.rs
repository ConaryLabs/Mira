// src/tools/web_search/handler.rs

use super::{WebSearchArgs, WebSearchResult, WebSearchError, WebSearchConfig, ToolCall, ToolCallResult};
use super::client::WebSearchClient;
use serde_json::json;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Handler for web search tool calls from OpenAI
pub struct WebSearchHandler {
    client: Arc<Mutex<WebSearchClient>>,
    config: WebSearchConfig,
}

impl WebSearchHandler {
    pub fn new(config: WebSearchConfig) -> Result<Self, WebSearchError> {
        let client = WebSearchClient::new(config.clone())?;
        Ok(Self {
            client: Arc::new(Mutex::new(client)),
            config,
        })
    }

    /// Process a tool call from OpenAI
    pub async fn handle_tool_call(&self, tool_call: &ToolCall) -> Result<ToolCallResult, WebSearchError> {
        if tool_call.function.name != "web_search" {
            return Err(WebSearchError::ApiError(format!(
                "Unknown function: {}",
                tool_call.function.name
            )));
        }

        // Parse arguments
        let args: WebSearchArgs = serde_json::from_str(&tool_call.function.arguments)
            .map_err(|e| WebSearchError::SerializationError(e))?;

        eprintln!("🔍 Web search requested: {}", args.query);

        // Execute search
        let result = self.search(&args).await?;

        // Format result for OpenAI
        let content = self.format_result_for_llm(&result);

        Ok(ToolCallResult::new(tool_call.id.clone(), content))
    }

    /// Execute a web search
    pub async fn search(&self, args: &WebSearchArgs) -> Result<WebSearchResult, WebSearchError> {
        let mut client = self.client.lock().await;
        client.search(args).await
    }

    /// Format search results for LLM consumption
    fn format_result_for_llm(&self, result: &WebSearchResult) -> String {
        let mut output = String::new();
        
        // Add summary if available
        if !result.summary.is_empty() {
            output.push_str(&format!("Summary: {}\n\n", result.summary));
        }

        // Add sources with citations
        output.push_str("Sources:\n");
        for (i, source) in result.sources.iter().enumerate() {
            output.push_str(&format!(
                "[{}] {} - {}\n{}\n",
                i + 1,
                source.title,
                source.url,
                source.snippet
            ));
            
            if let Some(date) = &source.published_date {
                output.push_str(&format!("Published: {}\n", date));
            }
            
            if i < result.sources.len() - 1 {
                output.push_str("\n");
            }
        }

        // Add metadata
        if let Some(total) = result.total_results {
            output.push_str(&format!("\nTotal results found: {}", total));
        }
        output.push_str(&format!("\nSearch provider: {}", result.provider));

        output
    }

    /// Format search results as JSON (alternative format)
    pub fn format_result_as_json(&self, result: &WebSearchResult) -> serde_json::Value {
        json!({
            "summary": result.summary,
            "sources": result.sources.iter().map(|s| json!({
                "title": s.title,
                "url": s.url,
                "snippet": s.snippet,
                "published_date": s.published_date,
                "relevance_score": s.relevance_score,
            })).collect::<Vec<_>>(),
            "total_results": result.total_results,
            "provider": result.provider,
        })
    }
}

/// Integration with OpenAI Chat Completions API
pub struct OpenAIWebSearchIntegration {
    handler: Arc<WebSearchHandler>,
}

impl OpenAIWebSearchIntegration {
    pub fn new(web_search_config: WebSearchConfig) -> Result<Self, WebSearchError> {
        let handler = Arc::new(WebSearchHandler::new(web_search_config)?);
        Ok(Self { handler })
    }

    /// Process all tool calls in a response
    pub async fn process_tool_calls(&self, tool_calls: Vec<ToolCall>) -> Vec<ToolCallResult> {
        let mut results = Vec::new();
        
        for tool_call in tool_calls {
            match self.handler.handle_tool_call(&tool_call).await {
                Ok(result) => results.push(result),
                Err(e) => {
                    eprintln!("Error handling tool call {}: {:?}", tool_call.id, e);
                    results.push(ToolCallResult::new(
                        tool_call.id,
                        format!("Error: {}", e),
                    ));
                }
            }
        }
        
        results
    }

    /// Build a message with tool results for OpenAI
    pub fn build_tool_message(&self, results: Vec<ToolCallResult>) -> serde_json::Value {
        let messages: Vec<serde_json::Value> = results.into_iter().map(|r| {
            json!({
                "role": r.role,
                "tool_call_id": r.tool_call_id,
                "content": r.content,
            })
        }).collect();
        
        json!(messages)
    }
}

/// Helper to determine if a query needs web search
/// DEPRECATED: We now let the LLM decide via function calling
/// Keeping this for backward compatibility but it just returns false
pub fn needs_web_search(_query: &str) -> bool {
    // Let the LLM decide through function calling
    // This is much more intelligent than keyword matching
    false
}

/// Helper to extract search query from user message
/// DEPRECATED: The LLM extracts this via function arguments
pub fn extract_search_query(_message: &str) -> Option<String> {
    // Let the LLM handle query extraction
    None
}
