// src/tools/web_search/mod.rs

pub mod types;
pub mod client;
pub mod handler;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// OpenAI Function Tool Definition for web_search - Updated for 2025 best practices
/// Using structured outputs with strict mode for guaranteed JSON compliance
pub fn web_search_tool_definition() -> Value {
    serde_json::json!({
        "type": "function",
        "function": {
            "name": "web_search",
            "description": "Search the web for current, real-time information. Use this tool when you need up-to-date data about current events, news, sports scores, technology updates, prices, or any topic that requires information beyond January 2025. The tool returns summarized results with sources.",
            "strict": true,  // Enable structured outputs for guaranteed JSON Schema compliance
            "parameters": {
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "The search query. Be specific and concise. Use 1-6 words for best results. Example: 'latest GPT-4.1 features' or 'Argentina president 2025'"
                    },
                    "num_results": {
                        "type": "integer",
                        "description": "Number of search results to return (1-10)",
                        "default": 5,
                        "minimum": 1,
                        "maximum": 10
                    },
                    "search_depth": {
                        "type": "string",
                        "enum": ["basic", "advanced"],
                        "description": "basic: quick search with snippets. advanced: fetches and reads full page content",
                        "default": "basic"
                    }
                },
                "required": ["query", "num_results", "search_depth"],  // OpenAI strict mode requires ALL properties listed
                "additionalProperties": false
            }
        }
    })
}

/// Arguments for the web_search function - matches OpenAI function schema
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WebSearchArgs {
    pub query: String,
    #[serde(default = "default_num_results")]
    pub num_results: i32,
    #[serde(default = "default_search_depth")]
    pub search_depth: SearchDepth,
}

fn default_num_results() -> i32 { 5 }
fn default_search_depth() -> SearchDepth { SearchDepth::Basic }

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum SearchDepth {
    Basic,
    Advanced,
}

/// Result from a web search - formatted for LLM consumption
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WebSearchResult {
    /// Summarized findings from the search
    pub summary: String,
    /// List of sources with citations
    pub sources: Vec<SearchSource>,
    /// Total number of results found
    pub total_results: Option<i32>,
    /// Search provider used
    pub provider: String,
    /// Raw results for debugging (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_results: Option<Vec<RawSearchResult>>,
}

/// Individual search source with citation info
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SearchSource {
    pub title: String,
    pub url: String,
    pub snippet: String,
    pub published_date: Option<String>,
    pub domain: Option<String>,
    pub relevance_score: Option<f32>,
}

/// Raw search result from the search provider
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RawSearchResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
    pub body: Option<String>,
    pub published_date: Option<String>,
    pub author: Option<String>,
    pub score: Option<f32>,
}

/// Error types for web search
#[derive(Debug, thiserror::Error)]
pub enum WebSearchError {
    #[error("Search API error: {0}")]
    ApiError(String),
    
    #[error("Network error: {0}")]
    NetworkError(#[from] reqwest::Error),
    
    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),
    
    #[error("Rate limit exceeded")]
    RateLimitExceeded,
    
    #[error("Invalid API key")]
    InvalidApiKey,
    
    #[error("No results found")]
    NoResults,
    
    #[error("Search timeout")]
    Timeout,
}

/// Configuration for web search
#[derive(Debug, Clone)]
pub struct WebSearchConfig {
    /// Which provider to use
    pub provider: SearchProvider,
    /// API key for the provider
    pub api_key: Option<String>,
    /// Maximum number of results to return
    pub max_results: usize,
    /// Whether to include raw results in response
    pub include_raw: bool,
    /// Timeout in seconds
    pub timeout_seconds: u64,
    /// Whether to use safe search
    pub safe_search: bool,
    /// Language for results
    pub language: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SearchProvider {
    Tavily,      // Best for AI agents, built for LLMs ($0.001 per search)
    SerpApi,     // Google results, more expensive ($0.01 per search)
    Bing,        // Microsoft's API, good coverage
    Brave,       // Privacy-focused, good for tech queries
    DuckDuckGo,  // Free but limited API
}

impl Default for WebSearchConfig {
    fn default() -> Self {
        Self {
            provider: SearchProvider::Tavily,  // Tavily is optimized for AI in 2025
            api_key: None,
            max_results: 5,
            include_raw: false,
            timeout_seconds: 10,
            safe_search: true,
            language: "en".to_string(),
        }
    }
}

/// Tool call representation for OpenAI API
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ToolCall {
    pub id: String,
    pub r#type: String,
    pub function: FunctionCall,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String,  // JSON string of arguments
}

/// Tool call result to send back to OpenAI
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ToolCallResult {
    pub tool_call_id: String,
    pub role: String,  // Always "tool"
    pub content: String,  // JSON or text result
}

impl ToolCallResult {
    pub fn new(tool_call_id: String, content: String) -> Self {
        Self {
            tool_call_id,
            role: "tool".to_string(),
            content,
        }
    }
}
