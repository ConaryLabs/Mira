macro_rules! define_tool {
    ($name:expr, $description:expr, $params:expr) => {
        Tool {
            tool_type: $name.to_string(),
            function: Some(FunctionDefinition {
                name: $name.to_string(),
                description: $description.to_string(),
                parameters: $params,
            }),
            web_search: None,
            code_interpreter: None,
        }
    };
}

pub fn get_enabled_tools() -> Vec<Tool> {
    vec![
        (CONFIG.enable_web_search, define_tool!(
            "web_search",
            "Search the web for current information...",
            json!({ /* params */ })
        )),
        // ... repeat for other tools
    ]
    .into_iter()
    .filter_map(|(enabled, tool)| if enabled { Some(tool) } else { None })
    .collect()
}
