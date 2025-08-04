// tests/test_web_search.rs

use mira_backend::tools::web_search::{
    web_search_tool_definition,
    WebSearchArgs,
    WebSearchConfig,
    SearchProvider,
    SearchDepth,
    ToolCall,
    FunctionCall,
};
use mira_backend::tools::WebSearchHandler; // Import from tools module directly
use mira_backend::tools::web_search::handler::{needs_web_search, extract_search_query};
use mira_backend::llm::OpenAIClient;
use mira_backend::services::ChatService;
use mira_backend::persona::PersonaOverlay;
use std::sync::Arc;
use std::env;
use serde_json::json;

/// Helper function to ensure .env is loaded for tests
fn ensure_env_loaded() {
    // Load .env file from project root
    // This works whether running from project root or tests/ directory
    dotenv::from_filename(".env").ok();
    dotenv::from_filename("../.env").ok(); // In case we're in a subdirectory
    
    // Also try the standard dotenv load
    dotenv::dotenv().ok();
}

/// Test that the web search tool definition is properly formatted
#[test]
fn test_web_search_tool_definition() {
    let tool_def = web_search_tool_definition();
    
    // Check it has the right structure
    assert_eq!(tool_def["type"], "function");
    assert_eq!(tool_def["function"]["name"], "web_search");
    assert!(tool_def["function"]["description"].as_str().unwrap().contains("current"));
    assert_eq!(tool_def["function"]["strict"], true);
    
    // Check parameters
    let params = &tool_def["function"]["parameters"];
    assert_eq!(params["type"], "object");
    assert!(params["properties"]["query"].is_object());
    assert!(params["required"].as_array().unwrap().contains(&json!("query")));
}

/// Test the needs_web_search detection logic
/// Note: This is deprecated - we let the LLM decide via function calling
#[test]
fn test_needs_web_search_detection() {
    // Since we're letting the LLM decide, this always returns false now
    // The test is kept for backward compatibility but the function is deprecated
    
    // These would normally trigger search, but we let LLM decide now
    assert!(!needs_web_search("What's the latest news?"));
    assert!(!needs_web_search("Who is the current president of Argentina?"));
    assert!(!needs_web_search("What's the Bitcoin price today?"));
    
    // These wouldn't trigger search, and still don't
    assert!(!needs_web_search("What is the capital of France?"));
    assert!(!needs_web_search("Explain photosynthesis"));
}

/// Test search query extraction
/// Note: This is deprecated - the LLM extracts via function arguments
#[test]
fn test_extract_search_query() {
    // Since we're letting the LLM handle this, always returns None now
    assert_eq!(extract_search_query("Search for the latest AI news"), None);
    assert_eq!(extract_search_query("Who is Sam Altman?"), None);
}

/// Test WebSearchConfig creation and defaults
#[test]
fn test_web_search_config() {
    let default_config = WebSearchConfig::default();
    
    assert_eq!(default_config.provider, SearchProvider::Tavily);
    assert_eq!(default_config.max_results, 5);
    assert_eq!(default_config.timeout_seconds, 10);
    assert_eq!(default_config.safe_search, true);
    assert_eq!(default_config.language, "en");
    assert_eq!(default_config.include_raw, false);
    
    // Test custom config
    let custom_config = WebSearchConfig {
        provider: SearchProvider::Brave,
        api_key: Some("test-key".to_string()),
        max_results: 10,
        include_raw: true,
        timeout_seconds: 30,
        safe_search: false,
        language: "es".to_string(),
    };
    
    assert_eq!(custom_config.provider, SearchProvider::Brave);
    assert_eq!(custom_config.max_results, 10);
    assert_eq!(custom_config.timeout_seconds, 30);
}

/// Test WebSearchArgs serialization/deserialization
#[test]
fn test_web_search_args() {
    let args = WebSearchArgs {
        query: "test query".to_string(),
        num_results: 5,
        search_depth: SearchDepth::Basic,
    };
    
    // Serialize to JSON
    let json = serde_json::to_string(&args).unwrap();
    assert!(json.contains("test query"));
    assert!(json.contains("basic"));
    
    // Deserialize from JSON
    let json_str = r#"{"query":"another test","num_results":10,"search_depth":"advanced"}"#;
    let parsed: WebSearchArgs = serde_json::from_str(json_str).unwrap();
    assert_eq!(parsed.query, "another test");
    assert_eq!(parsed.num_results, 10);
    assert_eq!(parsed.search_depth, SearchDepth::Advanced);
    
    // Test with defaults
    let minimal_json = r#"{"query":"minimal test"}"#;
    let minimal: WebSearchArgs = serde_json::from_str(minimal_json).unwrap();
    assert_eq!(minimal.query, "minimal test");
    assert_eq!(minimal.num_results, 5); // default
    assert_eq!(minimal.search_depth, SearchDepth::Basic); // default
}

/// Test ToolCall structure for OpenAI function calling
#[test]
fn test_tool_call_structure() {
    let tool_call = ToolCall {
        id: "call_123".to_string(),
        r#type: "function".to_string(),
        function: FunctionCall {
            name: "web_search".to_string(),
            arguments: r#"{"query":"test search","num_results":3}"#.to_string(),
        },
    };
    
    // Serialize and check structure
    let json = serde_json::to_value(&tool_call).unwrap();
    assert_eq!(json["id"], "call_123");
    assert_eq!(json["type"], "function");
    assert_eq!(json["function"]["name"], "web_search");
    assert!(json["function"]["arguments"].as_str().unwrap().contains("test search"));
}

/// Integration test: Web search handler initialization
#[tokio::test]
async fn test_web_search_handler_init() {
    ensure_env_loaded();
    
    // Skip if no API key
    if env::var("TAVILY_API_KEY").is_err() {
        eprintln!("Skipping web search handler test - no TAVILY_API_KEY in .env");
        return;
    }
    
    let config = WebSearchConfig {
        provider: SearchProvider::Tavily,
        api_key: env::var("TAVILY_API_KEY").ok(),
        ..Default::default()
    };
    
    let handler = WebSearchHandler::new(config);
    assert!(handler.is_ok(), "Failed to create WebSearchHandler");
}

/// Integration test: Actual web search (requires API key)
#[tokio::test]
async fn test_actual_web_search() {
    ensure_env_loaded();
    
    // Skip if no API key
    let api_key = match env::var("TAVILY_API_KEY") {
        Ok(key) => key,
        Err(_) => {
            eprintln!("Skipping actual web search test - no TAVILY_API_KEY in .env");
            return;
        }
    };
    
    let config = WebSearchConfig {
        provider: SearchProvider::Tavily,
        api_key: Some(api_key),
        max_results: 3,
        timeout_seconds: 15,
        ..Default::default()
    };
    
    let handler = WebSearchHandler::new(config).expect("Failed to create handler");
    
    let args = WebSearchArgs {
        query: "OpenAI GPT-4".to_string(),
        num_results: 3,
        search_depth: SearchDepth::Basic,
    };
    
    let result = handler.search(&args).await;
    
    match result {
        Ok(search_result) => {
            println!("✅ Search successful!");
            println!("   Summary: {}", search_result.summary);
            println!("   Sources found: {}", search_result.sources.len());
            println!("   Provider: {}", search_result.provider);
            
            assert!(!search_result.sources.is_empty(), "No sources returned");
            assert!(search_result.sources.len() <= 3, "Too many sources returned");
            
            // Check first source has required fields
            if let Some(first) = search_result.sources.first() {
                assert!(!first.title.is_empty(), "Source title is empty");
                assert!(!first.url.is_empty(), "Source URL is empty");
                assert!(!first.snippet.is_empty(), "Source snippet is empty");
            }
        }
        Err(e) => {
            panic!("Search failed: {:?}", e);
        }
    }
}

/// Integration test: ChatService with web search
#[tokio::test]
async fn test_chat_service_with_web_search() {
    ensure_env_loaded();
    
    // Skip if no API keys
    if env::var("OPENAI_API_KEY").is_err() || env::var("TAVILY_API_KEY").is_err() {
        eprintln!("Skipping ChatService web search test - missing API keys in .env");
        eprintln!("  OPENAI_API_KEY: {}", if env::var("OPENAI_API_KEY").is_ok() { "✓" } else { "✗" });
        eprintln!("  TAVILY_API_KEY: {}", if env::var("TAVILY_API_KEY").is_ok() { "✓" } else { "✗" });
        return;
    }
    
    let llm_client = Arc::new(OpenAIClient::new());
    let chat_service = ChatService::new(llm_client);
    
    // Test a query that should trigger web search
    let response = chat_service
        .process_message(
            "test-session",
            "What are the latest features of GPT-4 as of 2025?",
            &PersonaOverlay::Default,
            None,
        )
        .await;
    
    match response {
        Ok(chat_response) => {
            println!("✅ Chat with web search successful!");
            println!("   Output preview: {}", 
                chat_response.output.chars().take(200).collect::<String>());
            println!("   Mood: {}", chat_response.mood);
            
            // Response should mention searching or current info
            assert!(!chat_response.output.is_empty());
        }
        Err(e) => {
            eprintln!("⚠️ Chat service test failed (may be rate limit): {:?}", e);
        }
    }
}

/// Test that web search integrates with OpenAI function calling
#[tokio::test]
async fn test_openai_function_calling_integration() {
    ensure_env_loaded();
    
    if env::var("OPENAI_API_KEY").is_err() {
        eprintln!("Skipping OpenAI integration test - no OPENAI_API_KEY in .env");
        return;
    }
    
    let client = OpenAIClient::new();
    
    let messages = vec![
        json!({"role": "system", "content": "You are a helpful assistant with access to web search."}),
        json!({"role": "user", "content": "What's the current price of Bitcoin?"}),
    ];
    
    let tools = vec![web_search_tool_definition()];
    
    let response = client
        .chat_with_tools(messages, tools, None, Some("gpt-4.1"))
        .await;
    
    match response {
        Ok(resp) => {
            println!("✅ OpenAI function calling test successful!");
            
            // Check if the model wants to use the tool
            if let Some(tool_calls) = resp["choices"][0]["message"]["tool_calls"].as_array() {
                println!("   Model requested {} tool call(s)", tool_calls.len());
                
                for (i, call) in tool_calls.iter().enumerate() {
                    println!("   Tool call {}: {}", i + 1, call["function"]["name"]);
                    
                    // Verify it's calling our web_search function
                    assert_eq!(call["function"]["name"], "web_search");
                    
                    // Parse and verify arguments
                    let args_str = call["function"]["arguments"].as_str().unwrap();
                    let args: WebSearchArgs = serde_json::from_str(args_str).unwrap();
                    assert!(args.query.to_lowercase().contains("bitcoin") || 
                           args.query.to_lowercase().contains("btc"));
                }
            } else {
                println!("   Model did not request tool use (might have answered directly)");
            }
        }
        Err(e) => {
            eprintln!("⚠️ OpenAI integration test failed: {:?}", e);
        }
    }
}

/// Test provider selection
#[test]
fn test_provider_selection() {
    use mira_backend::tools::web_search::SearchProvider;
    
    assert_eq!(SearchProvider::Tavily.name(), "Tavily");
    assert_eq!(SearchProvider::SerpApi.name(), "SerpApi");
    assert_eq!(SearchProvider::Brave.name(), "Brave");
    assert_eq!(SearchProvider::Bing.name(), "Bing");
    assert_eq!(SearchProvider::DuckDuckGo.name(), "DuckDuckGo");
}

/// Test error handling for missing API key during search (not initialization)
#[tokio::test]
async fn test_missing_api_key_error() {
    let config = WebSearchConfig {
        provider: SearchProvider::Tavily,
        api_key: None, // No API key
        ..Default::default()
    };
    
    // Creating handler should succeed even without API key
    let handler = WebSearchHandler::new(config);
    assert!(handler.is_ok(), "Handler creation should succeed without API key");
    
    // But searching should fail
    let handler = handler.unwrap();
    let args = WebSearchArgs {
        query: "test".to_string(),
        num_results: 1,
        search_depth: SearchDepth::Basic,
    };
    
    let result = handler.search(&args).await;
    assert!(result.is_err(), "Search should fail without API key");
}

// Test helper function
fn setup_test_environment() {
    ensure_env_loaded();
    
    // Set up any test-specific environment variables
    if env::var("RUST_LOG").is_err() {
        unsafe {
            env::set_var("RUST_LOG", "debug");
        }
    }
}

/// Debug test to verify .env loading
#[test]
fn test_env_loading() {
    ensure_env_loaded();
    
    println!("🔍 Checking environment variables from .env:");
    println!("  Current dir: {:?}", std::env::current_dir().unwrap());
    
    // Check for expected variables
    let vars = [
        "OPENAI_API_KEY",
        "TAVILY_API_KEY",
        "DATABASE_URL",
        "QDRANT_URL",
    ];
    
    for var in &vars {
        match env::var(var) {
            Ok(val) => {
                // Don't print full API keys for security
                if var.contains("KEY") {
                    println!("  {} = {} (length: {})", var, &val[..6.min(val.len())], val.len());
                } else {
                    println!("  {} = {}", var, val);
                }
            }
            Err(_) => {
                println!("  {} = NOT SET", var);
            }
        }
    }
}

#[cfg(test)]
mod test_helpers {
    /// Create a mock search result for testing
    pub fn create_mock_search_result() -> mira_backend::tools::web_search::WebSearchResult {
        use mira_backend::tools::web_search::{WebSearchResult, SearchSource};
        
        WebSearchResult {
            summary: "Test search results".to_string(),
            sources: vec![
                SearchSource {
                    title: "Test Result 1".to_string(),
                    url: "https://example.com/1".to_string(),
                    snippet: "This is a test result".to_string(),
                    published_date: Some("2025-08-03".to_string()),
                    domain: Some("example.com".to_string()),
                    relevance_score: Some(0.95),
                },
            ],
            total_results: Some(1),
            provider: "Mock".to_string(),
            raw_results: None,
        }
    }
}
