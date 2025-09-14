// Refactored version - from 121 lines to ~40 lines

pub fn get_enabled_tools() -> Vec<Tool> {
    let mut tools = Vec::new();
    
    // Define tool configurations as data, not code
    let tool_configs = vec![
        (CONFIG.enable_web_search, create_web_search_tool()),
        (CONFIG.enable_code_interpreter, create_code_interpreter_tool()),
        (CONFIG.enable_file_search, create_file_search_tool()),
        (CONFIG.enable_load_file_context, create_load_file_context_tool()),
        // ... other tools
    ];
    
    // Add enabled tools
    for (enabled, tool) in tool_configs {
        if enabled {
            tools.push(tool);
        }
    }
    
    tools
}

// Each tool creation is now a separate, testable function
fn create_web_search_tool() -> Tool {
    Tool {
        tool_type: "web_search".to_string(),
        function: Some(FunctionDefinition {
            name: "web_search".to_string(),
            description: "Search the web for current information...".to_string(),
            parameters: web_search_params(),
        }),
        web_search: None,
        code_interpreter: None,
    }
}

fn web_search_params() -> Value {
    json!({
        "type": "object",
        "properties": {
            "query": {
                "type": "string",
                "description": "The search query to find relevant web content"
            },
            "search_type": {
                "type": "string",
                "enum": ["general", "news", "academic", "images"],
                "description": "Type of search to perform (optional, default: general)"
            }
        },
        "required": ["query"]
    })
}
