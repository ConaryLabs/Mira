// src/tools/mod.rs

pub mod mira_import;
pub mod web_search;

// Re-export commonly used items from web_search
pub use web_search::{
    web_search_tool_definition,
    WebSearchArgs,
    WebSearchResult,
    WebSearchConfig,
    SearchProvider,
    WebSearchError,
    ToolCall,
    ToolCallResult,
};

// Re-export handler items properly
pub use web_search::handler::WebSearchHandler;
